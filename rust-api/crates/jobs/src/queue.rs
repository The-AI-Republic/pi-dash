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
//! A `running` row whose worker died before settlement is reclaimed past
//! [`STALE_CLAIM_LEASE_SECS`] by [`reclaim_stale_running`] (worker boot
//! plus the scheduler loop), so no row sits `running` forever.
//!
//! The tables are owned by the Rust backend and created at worker boot by
//! [`ensure_schema`]. Django stays schema owner for its own tables until
//! switchover — no Django migration is touched.
//!
//! Everything but the DDL and the notify-carrying enqueue goes through
//! sea-query builders (`DbBackend::Postgres.build` over
//! [`pidash_db::sea_orm_connection`], or `orm_tx::TxConn` inside the
//! claim transaction); rows decode by column name. The exempt
//! statements are pinned by unit tests against rendered SQL; the
//! builder tests pin the same shapes.

//! The setters and delete sweep bind a client-computed `now` (the
//! same instant Django's `timezone.now()` took), which keeps clock
//! skew inside one statement's lease math.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, TryGetable};
use sea_query::{
    extension::postgres::PgExpr, Alias, CaseStatement, Condition, DeleteStatement, Expr, Func,
    LockBehavior, LockType, Order, Query, SelectStatement, UpdateStatement,
};
use serde_json::{Map, Value};
use sqlx::{FromRow, PgPool};

use crate::json_compact;
use pidash_db::orm_tx::{into_sqlx, TxConn};
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

/// Postgres `LISTEN`/`NOTIFY` channel the worker wakes on (PIDASHCONV-476).
/// The payload is the queue name; the worker claims its own queue after any
/// wake, so a foreign-queue notification costs at most one empty claim.
pub const JOB_NOTIFY_CHANNEL: &str = "rust_job_queue";

/// How long a `running` claim may go unsettled before it is presumed
/// orphaned (PIDASHCONV-1327): one hour, far beyond any legitimate job.
/// [`reclaim_stale_running`] returns older rows to the queue (or parks
/// them once the budget is spent). It runs at worker boot and every
/// minute from the scheduler loop, so a dead worker's rows wait at most
/// about a minute past the lease.
pub const STALE_CLAIM_LEASE_SECS: f64 = 3600.0;

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

    /// Split out the Celery wire parts, mirroring the worker forward-path
    /// mapping (array args pass through, anything else wraps; object
    /// kwargs pass through, anything else empties). Fields move out via
    /// `replace` because [`NewJob`] implements [`Drop`].
    pub fn into_message_parts(mut self) -> (Vec<Value>, Map<String, Value>) {
        let args = match std::mem::replace(&mut self.args, Value::Null) {
            Value::Array(items) => items,
            other => vec![other],
        };
        let kwargs = match std::mem::replace(&mut self.kwargs, Value::Null) {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        (args, kwargs)
    }
}

impl Drop for NewJob {
    fn drop(&mut self) {
        // Iterative: payloads nest ~9900 deep and serde's `Map` drop
        // recurses through IndexMap/hashbrown glue (~8 frames per level),
        // overflowing 2MB workers past ~4000 levels (PIDASHCONV-626: the
        // exotic differential's PATCH deep-ok aborted here, after a
        // successful iterative bind). Fields are swapped out so the
        // derived remainder drops trivial scalars only.
        let args = std::mem::replace(&mut self.args, Value::Null);
        let kwargs = std::mem::replace(&mut self.kwargs, Value::Null);
        crate::json_compact::drop_value_deep(args);
        crate::json_compact::drop_value_deep(kwargs);
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

/// Advisory-lock key serializing the queue DDL at boot
/// (NEWPIDASH-156). Distinct from
/// [`crate::scheduler::BEAT_LOCK_KEY`] (the scheduler singleton) and
/// from the per-project keys derived in
/// `pidash_db::entity::issue_hooks`: the queue DDL lock is global to
/// the database, not per project.
pub const SCHEMA_LOCK_KEY: i64 = 474_002_156_156_156;

/// Statement taking the transaction-scoped [`SCHEMA_LOCK_KEY`] lock.
/// The lock is held until the DDL transaction commits, so concurrent
/// first-boots run the `CREATE TABLE IF NOT EXISTS` statements one at
/// a time (NEWPIDASH-156).
pub fn schema_lock_sql() -> &'static str {
    "SELECT pg_advisory_xact_lock($1)"
}

/// DDL for the queue table. Idempotent: safe to run on every worker boot.
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
    )"
}

/// DDL for the claim index. Separate statement from [`ensure_schema_sql`]:
/// Postgres rejects multiple commands in one prepared statement, so
/// [`ensure_schema`] executes each DDL on its own.
pub fn ensure_claim_index_sql() -> &'static str {
    "CREATE INDEX IF NOT EXISTS rust_job_queue_claim_idx \
        ON rust_job_queue (status, visible_at, id)"
}

/// Create both queue tables. Runs at worker boot, never from requests.
///
/// The three DDL statements run inside one transaction under the
/// [`SCHEMA_LOCK_KEY`] advisory lock (NEWPIDASH-156): Postgres does
/// not make `CREATE TABLE IF NOT EXISTS` safe under concurrency — two
/// sessions that both see no table race on the catalogue insert and
/// the loser fails with 23505 (`pg_class_relname_nsp_index`) or 42P07
/// (`duplicate_table`). The lock serializes concurrent first-boots
/// (compose starting api + worker, several api replicas), so each
/// later booter runs its `IF NOT EXISTS` statements only after the
/// winner has committed and is a no-op. Covers the schedule table
/// too: its DDL is the third statement below.
pub async fn ensure_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // raw-sql: advisory xact locks have no SeaORM/sea-query builder form (orm-migration.md §6); the key is bound, never interpolated.
    sqlx::query(schema_lock_sql())
        .bind(SCHEMA_LOCK_KEY)
        .execute(&mut *tx)
        .await?;
    // raw-sql: Rust-owned DDL (CREATE TABLE/INDEX IF NOT EXISTS) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the queue table has no entity by design.
    sqlx::query(ensure_schema_sql()).execute(&mut *tx).await?;
    // raw-sql: Rust-owned DDL (CREATE TABLE/INDEX IF NOT EXISTS) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the queue table has no entity by design.
    sqlx::query(ensure_claim_index_sql())
        .execute(&mut *tx)
        .await?;
    // raw-sql: Rust-owned DDL passthrough (schedule schema) is an allowed exception per orm-migration.md §6: sea-query cannot express DDL, and the queue table has no entity by design.
    sqlx::query(crate::schedule::ensure_schedule_schema_sql())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn enqueue_sql() -> &'static str {
    // Push-wake (PIDASHCONV-476): the INSERT carries a `pg_notify` side
    // effect in the same prepared statement, so the executor-generic core
    // keeps working inside open transactions and the notification only
    // delivers at commit — a wake always sees a committed row.
    "WITH ins AS (\
        INSERT INTO rust_job_queue (celery_id, task, args, kwargs, queue, visible_at, max_retries) \
        VALUES ($1, $2, $3, $4, $5, COALESCE($6, now()), $7) \
        RETURNING id \
     ), wake AS (\
        SELECT pg_notify('rust_job_queue', $5) \
     ) \
     SELECT ins.id FROM ins, wake"
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
    // raw-sql: the enqueue CTE carries a pg_notify LISTEN/NOTIFY push-wake in one prepared statement; sea-query has no CTE-with-side-effect form, and notify is an allowed exception per orm-migration.md §6.
    let id: i64 = sqlx::query_scalar(enqueue_sql())
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&job.task)
        .bind(DeepJson(&job.args))
        .bind(DeepJson(&job.kwargs))
        .bind(&job.queue)
        .bind(job.visible_at)
        .bind(job.max_retries)
        .fetch_one(executor)
        .await?;
    Ok(id)
}

/// A `serde_json::Value` bind that serializes iteratively
/// (PIDASHCONV-626): sqlx's stock `Value` encoding recurses through
/// `serde_json::to_writer` and aborts the worker thread past ~4000
/// nesting levels, while ports may legitimately enqueue ~9900-deep
/// payloads. The bytes are identical to stock encoding (JSONB version
/// byte + compact text) for every input.
struct DeepJson<'a>(&'a Value);

impl sqlx::Type<sqlx::Postgres> for DeepJson<'_> {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <serde_json::Value as sqlx::Type<sqlx::Postgres>>::type_info()
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for DeepJson<'q> {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        buf.push(1);
        buf.extend(json_compact::to_compact_vec(self.0));
        Ok(sqlx::encode::IsNull::No)
    }
}

/// One claimed row, decoded off a SeaORM `QueryResult` (column names,
/// order-independent).
fn map_job_row(row: &QueryResult) -> Result<JobRow, sqlx::Error> {
    fn get<T: TryGetable>(row: &QueryResult, column: &str) -> Result<T, sqlx::Error> {
        row.try_get::<T>("", column).map_err(into_sqlx)
    }
    Ok(JobRow {
        id: get(row, "id")?,
        celery_id: get(row, "celery_id")?,
        task: get(row, "task")?,
        args: get(row, "args")?,
        kwargs: get(row, "kwargs")?,
        queue: get(row, "queue")?,
        status: get(row, "status")?,
        attempts: get(row, "attempts")?,
        max_retries: get(row, "max_retries")?,
        visible_at: get(row, "visible_at")?,
        claimed_at: get(row, "claimed_at")?,
        claimed_by: get(row, "claimed_by")?,
        created_at: get(row, "created_at")?,
        last_error: get(row, "last_error")?,
    })
}

/// The claim select: oldest due row for `queue`, locked
/// `FOR UPDATE SKIP LOCKED` for the claim transaction.
fn claim_select(queue: &str) -> SelectStatement {
    let t = Alias::new(QUEUE_TABLE);
    let mut sel = Query::select();
    for col in [
        "id",
        "celery_id",
        "task",
        "args",
        "kwargs",
        "queue",
        "status",
        "attempts",
        "max_retries",
        "visible_at",
        "claimed_at",
        "claimed_by",
        "created_at",
        "last_error",
    ] {
        sel.column((t.clone(), Alias::new(col)));
    }
    sel.from(t.clone())
        .cond_where(
            Condition::all()
                .add(Expr::col((t.clone(), Alias::new("queue"))).eq(queue))
                .add(Expr::col((t.clone(), Alias::new("status"))).eq("queued"))
                .add(
                    Expr::col((t.clone(), Alias::new("visible_at")))
                        .lte(Expr::expr(Func::cust(Alias::new("now")))),
                ),
        )
        .order_by((t.clone(), Alias::new("visible_at")), Order::Asc)
        .order_by((t.clone(), Alias::new("id")), Order::Asc)
        .limit(1)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);
    sel
}

/// The claim write: mark the row `running` for `owner` at the server
/// clock.
fn claim_update(id: i64, owner: &str) -> UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new(QUEUE_TABLE))
        .values([
            (Alias::new("status"), Expr::val("running").into()),
            (
                Alias::new("claimed_at"),
                Expr::expr(Func::cust(Alias::new("now"))).into(),
            ),
            (Alias::new("claimed_by"), Expr::val(owner.to_owned()).into()),
        ])
        .and_where(Expr::col(Alias::new("id")).eq(id));
    stmt
}

/// Claim the oldest due job for `owner`, holding its row lock only for
/// the claim transaction. Concurrent workers never see the same row:
/// `SKIP LOCKED` skips rows locked by another claim.
pub async fn claim(pool: &PgPool, queue: &str, owner: &str) -> Result<Option<JobRow>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let job: Option<JobRow> = {
        let conn = TxConn::new(&mut tx);
        let found = conn
            .query_one(DbBackend::Postgres.build(&claim_select(queue)))
            .await
            .map_err(into_sqlx)?;
        let job = found.map(|row| map_job_row(&row)).transpose()?;
        if let Some(ref job) = job {
            conn.execute(DbBackend::Postgres.build(&claim_update(job.id, owner)))
                .await
                .map_err(into_sqlx)?;
        }
        job
    };
    tx.commit().await?;
    Ok(job)
}

/// The ack delete: the finished row is removed (no result backend).
fn ack_delete(id: i64) -> DeleteStatement {
    let mut stmt = Query::delete();
    stmt.from_table(Alias::new(QUEUE_TABLE))
        .and_where(Expr::col(Alias::new("id")).eq(id));
    stmt
}

/// Acknowledge a finished job: the row is deleted (no result backend).
pub async fn ack(pool: &PgPool, id: i64) -> Result<(), sqlx::Error> {
    let db = pidash_db::sea_orm_connection(pool);
    db.execute(DbBackend::Postgres.build(&ack_delete(id)))
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// The retry write: requeue with `attempts + 1` computed server-side.
fn retry_update(id: i64, visible_at: DateTime<Utc>, last_error: &str) -> UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new(QUEUE_TABLE))
        .values([
            (Alias::new("status"), Expr::val("queued").into()),
            (
                Alias::new("attempts"),
                Expr::col(Alias::new("attempts")).add(1),
            ),
            (
                Alias::new("visible_at"),
                Expr::val(visible_at.fixed_offset()).into(),
            ),
            (
                Alias::new("claimed_at"),
                Expr::val(Option::<DateTime<Utc>>::None).into(),
            ),
            (
                Alias::new("claimed_by"),
                Expr::val(Option::<String>::None).into(),
            ),
            (
                Alias::new("last_error"),
                Expr::val(last_error.to_owned()).into(),
            ),
        ])
        .and_where(Expr::col(Alias::new("id")).eq(id));
    stmt
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
    let db = pidash_db::sea_orm_connection(pool);
    db.execute(DbBackend::Postgres.build(&retry_update(id, visible_at, last_error)))
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// The fail write: park the row as `failed` with claim cleared.
fn fail_update(id: i64, last_error: &str) -> UpdateStatement {
    let mut stmt = Query::update();
    stmt.table(Alias::new(QUEUE_TABLE))
        .values([
            (Alias::new("status"), Expr::val("failed").into()),
            (
                Alias::new("claimed_at"),
                Expr::val(Option::<DateTime<Utc>>::None).into(),
            ),
            (
                Alias::new("claimed_by"),
                Expr::val(Option::<String>::None).into(),
            ),
            (
                Alias::new("last_error"),
                Expr::val(last_error.to_owned()).into(),
            ),
        ])
        .and_where(Expr::col(Alias::new("id")).eq(id));
    stmt
}

/// Park a job as failed after its retries are exhausted. The row stays
/// for diagnosis until [`purge_failed`] removes it.
pub async fn fail(pool: &PgPool, id: i64, last_error: &str) -> Result<(), sqlx::Error> {
    let db = pidash_db::sea_orm_connection(pool);
    db.execute(DbBackend::Postgres.build(&fail_update(id, last_error)))
        .await
        .map_err(into_sqlx)?;
    Ok(())
}

/// The reclaim cutoff (`now() - lease`): the `claimed_at` bound,
/// computed client-side so the lease math stays inside one value
/// (Django's `timezone.now()` is client-computed too, so rules and
/// probes that pin `now - lease` keep reading the same instant).
fn lease_cutoff(lease_secs: f64, now: DateTime<Utc>) -> Result<DateTime<Utc>, sqlx::Error> {
    if !lease_secs.is_finite() {
        return Err(sqlx::Error::Protocol("seconds must be finite".to_owned()));
    }
    let millis = (lease_secs * 1000.0) as i64;
    now.checked_sub_signed(ChronoDuration::milliseconds(millis))
        .ok_or_else(|| sqlx::Error::Protocol("lease outside the representable range".to_owned()))
}

/// The reclaim write: stale `running` rows return to `queued` with
/// `attempts + 1` and an immediate `visible_at` while budget lasts,
/// else park as `failed` with attempts untouched. One statement, so
/// concurrent reclaimers cannot double-move a row.
fn reclaim_update(cutoff: DateTime<Utc>) -> UpdateStatement {
    let budget_left = Condition::all()
        .add(Expr::col(Alias::new("attempts")).lt(Expr::col(Alias::new("max_retries"))));
    let mut stmt = Query::update();
    stmt.table(Alias::new(QUEUE_TABLE))
        .values([
            (
                Alias::new("status"),
                Expr::expr(
                    CaseStatement::new()
                        .case(budget_left.clone(), "queued")
                        .finally("failed"),
                )
                .into(),
            ),
            (
                Alias::new("attempts"),
                Expr::expr(
                    CaseStatement::new()
                        .case(
                            budget_left.clone(),
                            Expr::col(Alias::new("attempts")).add(1),
                        )
                        .finally(Expr::col(Alias::new("attempts"))),
                )
                .into(),
            ),
            (
                Alias::new("visible_at"),
                Expr::expr(
                    CaseStatement::new()
                        .case(budget_left, Expr::expr(Func::cust(Alias::new("now"))))
                        .finally(Expr::col(Alias::new("visible_at"))),
                )
                .into(),
            ),
            (
                Alias::new("claimed_at"),
                Expr::val(Option::<DateTime<Utc>>::None).into(),
            ),
            (
                Alias::new("claimed_by"),
                Expr::val(Option::<String>::None).into(),
            ),
            (
                Alias::new("last_error"),
                Expr::val("reclaimed stale running job claimed by ").concatenate(Func::coalesce([
                    Expr::col(Alias::new("claimed_by")).into(),
                    Expr::val("unknown").into(),
                ])),
            ),
        ])
        .and_where(
            Condition::all()
                .add(Expr::col(Alias::new("status")).eq("running"))
                .add(Expr::col(Alias::new("claimed_at")).lt(Expr::val(cutoff.fixed_offset())))
                .into(),
        );
    stmt
}

/// Reclaim rows orphaned by a worker that died before settlement (panic
/// without the dispatch guard, SIGKILL, OOM, crash, redeploy): every
/// `running` row claimed longer ago than `lease_secs` returns to `queued`
/// with `attempts + 1` and an immediate `visible_at` — or parks as
/// `failed` once the retry budget is spent, exactly like [`retry`] and
/// [`fail`]. Single statement, so concurrent workers reclaiming at once
/// are safe (the loser matches nothing). Returns the rows moved.
pub async fn reclaim_stale_running(pool: &PgPool, lease_secs: f64) -> Result<u64, sqlx::Error> {
    let db = pidash_db::sea_orm_connection(pool);
    let cutoff = lease_cutoff(lease_secs, Utc::now())?;
    let moved = db
        .execute(DbBackend::Postgres.build(&reclaim_update(cutoff)))
        .await
        .map_err(into_sqlx)?;
    Ok(moved.rows_affected())
}

/// The purge delete: `failed` rows created before the retention
/// cutoff go away.
fn purge_delete(cutoff: DateTime<Utc>) -> DeleteStatement {
    let mut stmt = Query::delete();
    stmt.from_table(Alias::new(QUEUE_TABLE)).and_where(
        Condition::all()
            .add(Expr::col(Alias::new("status")).eq("failed"))
            .add(Expr::col(Alias::new("created_at")).lt(Expr::val(cutoff.fixed_offset())))
            .into(),
    );
    stmt
}

/// Delete failures older than `retention_secs`.
pub async fn purge_failed(pool: &PgPool, retention_secs: f64) -> Result<u64, sqlx::Error> {
    let db = pidash_db::sea_orm_connection(pool);
    let cutoff = lease_cutoff(retention_secs, Utc::now())?;
    let removed = db
        .execute(DbBackend::Postgres.build(&purge_delete(cutoff)))
        .await
        .map_err(into_sqlx)?;
    Ok(removed.rows_affected())
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
            schema_lock_sql(),
            ensure_schema_sql(),
            ensure_claim_index_sql(),
            enqueue_sql(),
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
        let sql = claim_select("celery").to_string(sea_query::PostgresQueryBuilder);
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains(r#""visible_at" <= now()"#), "{sql}");
        assert!(
            sql.contains(
                r#"ORDER BY "rust_job_queue"."visible_at" ASC, "rust_job_queue"."id" ASC LIMIT 1"#
            ),
            "order+limit: {sql}"
        );
        assert!(
            sql.contains(
                r#""rust_job_queue"."queue" = 'celery' AND "rust_job_queue"."status" = 'queued'"#
            ),
            "filters: {sql}"
        );
        // The full row rides back for the worker render (no aliasing:
        // one table means no output-name collisions).
        assert!(sql.contains(r#""celery_id""#), "cols: {sql}");
        assert!(sql.contains(r#""last_error""#), "cols: {sql}");
    }

    #[test]
    fn claim_update_names_running_and_owner() {
        let sql = claim_update(7, "worker-1").to_string(sea_query::PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "rust_job_queue" SET"#), "{sql}");
        assert!(sql.contains(r#""status" = 'running'"#), "{sql}");
        assert!(sql.contains(r#""claimed_at" = now()"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = 'worker-1'"#), "{sql}");
        assert!(sql.contains(r#""id" = 7"#), "{sql}");
    }

    #[test]
    fn retry_increments_attempts_server_side() {
        let now = Utc::now();
        let sql = retry_update(9, now, "boom").to_string(sea_query::PostgresQueryBuilder);
        assert!(sql.contains(r#""status" = 'queued'"#), "{sql}");
        assert!(
            sql.contains(r#""attempts" = "attempts" + 1"#),
            "server-side +1: {sql}"
        );
        assert!(sql.contains(r#""claimed_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = NULL"#), "{sql}");
        assert!(sql.contains(r#""last_error" = 'boom'"#), "{sql}");
        assert!(sql.contains(r#""id" = 9"#), "{sql}");
    }

    #[test]
    fn fail_parks_with_claim_cleared() {
        let sql = fail_update(4, "dead").to_string(sea_query::PostgresQueryBuilder);
        assert!(sql.contains(r#""status" = 'failed'"#), "{sql}");
        assert!(sql.contains(r#""claimed_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = NULL"#), "{sql}");
        // `fail` spends no attempt.
        assert!(!sql.contains(r#""attempts""#), "{sql}");
        assert!(sql.contains(r#""last_error" = 'dead'"#), "{sql}");
    }

    #[test]
    fn ack_deletes_by_id() {
        let sql = ack_delete(11).to_string(sea_query::PostgresQueryBuilder);
        assert!(
            sql.contains(r#"DELETE FROM "rust_job_queue" WHERE "id" = 11"#),
            "{sql}"
        );
    }

    #[test]
    fn purge_deletes_only_old_failures() {
        let cutoff = Utc::now() - ChronoDuration::hours(2);
        let sql = purge_delete(cutoff).to_string(sea_query::PostgresQueryBuilder);
        assert!(
            sql.contains(
                r#"DELETE FROM "rust_job_queue" WHERE "status" = 'failed' AND "created_at" < "#
            ),
            "{sql}"
        );
    }

    #[test]
    fn lease_cutoff_rejects_non_finite_and_bounds() {
        let now = Utc::now();
        assert!(lease_cutoff(f64::INFINITY, now).is_err());
        assert!(lease_cutoff(f64::NEG_INFINITY, now).is_err());
        assert!(lease_cutoff(f64::NAN, now).is_err());
        let cutoff = lease_cutoff(3600.0, now).expect("finite lease bounds");
        assert_eq!(cutoff, now - ChronoDuration::seconds(3600));
        // A negative lease bounds in the future (same as make_interval).
        let future = lease_cutoff(-60.0, now).expect("negative lease bounds");
        assert_eq!(future, now + ChronoDuration::seconds(60));
    }

    #[test]
    fn enqueue_is_visible_immediately_by_default() {
        assert!(enqueue_sql().contains("COALESCE($6, now())"));
    }

    #[test]
    fn enqueue_carries_notify_wake_in_same_statement() {
        // The worker's push-wake depends on this: one prepared statement
        // (the executor-generic core also runs inside open transactions),
        // notifying the channel the worker listens on with the queue name.
        assert!(enqueue_sql().contains("pg_notify"));
        assert!(enqueue_sql().contains(JOB_NOTIFY_CHANNEL));
        assert!(enqueue_sql().contains("RETURNING id"));
    }

    #[test]
    fn reclaim_only_touches_stale_running_rows() {
        // PIDASHCONV-1327: fresh claims and non-running rows must never
        // move; the budget branch mirrors `retry` (requeue + 1) and
        // `fail` (park, attempts untouched).
        let cutoff = Utc::now() - ChronoDuration::hours(1);
        let sql = reclaim_update(cutoff).to_string(sea_query::PostgresQueryBuilder);
        assert!(sql.contains(r#""status" = 'running'"#), "{sql}");
        assert!(sql.contains(r#""claimed_at" < "#), "cutoff bound: {sql}");
        assert!(
            sql.contains(
                r#"CASE WHEN ("attempts" < "max_retries") THEN 'queued' ELSE 'failed' END"#
            ),
            "status budget branch: {sql}"
        );
        assert!(
            sql.contains(
                r#"CASE WHEN ("attempts" < "max_retries") THEN "attempts" + 1 ELSE "attempts" END"#
            ),
            "attempts budget branch: {sql}"
        );
        assert!(
            sql.contains(r#"THEN now() ELSE "visible_at" END"#),
            "visible_at branch: {sql}"
        );
        assert!(sql.contains(r#""claimed_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = NULL"#), "{sql}");
        assert!(
            sql.contains(
                r#"'reclaimed stale running job claimed by ' || COALESCE("claimed_by", 'unknown')"#
            ),
            "cause concat: {sql}"
        );
    }

    /// Reclaim settlement (PIDASHCONV-1327): a stale `running` row with
    /// budget left returns to `queued` with `attempts + 1` and the cause
    /// recorded; a stale row with a spent budget parks as `failed`; fresh
    /// `running` rows and `queued` rows are untouched. Needs a live
    /// Postgres; ignored by default.
    #[tokio::test]
    #[ignore = "needs live Postgres via DATABASE_URL"]
    async fn reclaim_stale_running_restores_queued_or_failed() {
        let url =
            std::env::var("DATABASE_URL").expect("export DATABASE_URL for the live queue tests");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect scratch database");
        ensure_schema(&pool).await.expect("ensure queue schema");
        let tag = uuid::Uuid::new_v4();
        let queue = format!("test-reclaim-fn-{tag}");
        async fn seed(
            pool: &sqlx::PgPool,
            queue: &str,
            task: &str,
            max_retries: i32,
            attempts: i32,
            claim_age: &str,
        ) -> i64 {
            let job = NewJob {
                task: task.to_owned(),
                args: json!([]),
                kwargs: json!({}),
                queue: queue.to_owned(),
                visible_at: None,
                max_retries,
            };
            let id = enqueue(pool, &job).await.expect("enqueue probe job");
            // One statement: a parallel test's boot reclaim must see the
            // final attempts value, never a half-set row.
            sqlx::query(
                "UPDATE rust_job_queue \
                 SET status = 'running', attempts = $3, \
                     claimed_at = now() - $2::interval, \
                     claimed_by = 'dead-worker' \
                 WHERE id = $1",
            )
            .bind(id)
            .bind(claim_age)
            .bind(attempts)
            .execute(pool)
            .await
            .expect("mark running with age");
            id
        }
        let stale_id = seed(
            &pool,
            &queue,
            "test.stale",
            DEFAULT_MAX_RETRIES,
            0,
            "2 hours",
        )
        .await;
        let fresh_id = seed(
            &pool,
            &queue,
            "test.fresh",
            DEFAULT_MAX_RETRIES,
            0,
            "1 minute",
        )
        .await;
        let spent_id = seed(
            &pool,
            &queue,
            "test.spent",
            DEFAULT_MAX_RETRIES,
            DEFAULT_MAX_RETRIES,
            "2 hours",
        )
        .await;
        let queued_id = enqueue(
            &pool,
            &NewJob {
                task: "test.queued".to_owned(),
                args: json!([]),
                kwargs: json!({}),
                queue: queue.clone(),
                visible_at: None,
                max_retries: DEFAULT_MAX_RETRIES,
            },
        )
        .await
        .expect("enqueue queued probe");
        // No exact moved-count assertion: reclaim is global, so a
        // parallel test's worker boot may move these rows first (or this
        // call moves another test's stale rows) — the per-row end states
        // below are deterministic either way.
        reclaim_stale_running(&pool, STALE_CLAIM_LEASE_SECS)
            .await
            .expect("reclaim");
        async fn state(pool: &sqlx::PgPool, id: i64) -> (String, i32, Option<String>) {
            sqlx::query_as("SELECT status, attempts, last_error FROM rust_job_queue WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .expect("fetch row state")
        }
        let (status, attempts, last_error) = state(&pool, stale_id).await;
        assert_eq!(status, "queued");
        assert_eq!(attempts, 1);
        let error = last_error.expect("reclaim records its cause");
        assert!(
            error.contains("reclaimed stale running job claimed by dead-worker"),
            "unexpected last_error: {error}"
        );
        let (status, attempts, _) = state(&pool, fresh_id).await;
        assert_eq!((status.as_str(), attempts), ("running", 0));
        let (status, attempts, last_error) = state(&pool, spent_id).await;
        assert_eq!(status, "failed");
        assert_eq!(attempts, DEFAULT_MAX_RETRIES, "a park spends no attempt");
        assert!(last_error
            .expect("park records its cause")
            .contains("reclaimed stale"));
        let (status, _, _) = state(&pool, queued_id).await;
        assert_eq!(status, "queued");
    }

    /// Concurrent first-boot (NEWPIDASH-156): several tasks run
    /// [`ensure_schema`] at once against a fresh scratch schema; every
    /// one must succeed. Without the advisory-lock serialization in
    /// `ensure_schema`, concurrent `CREATE TABLE IF NOT EXISTS` races
    /// on the catalogue: two sessions both see no table, race on the
    /// `pg_class` insert and the loser fails with 23505
    /// (`pg_class_relname_nsp_index`, here on `rust_job_queue_id_seq`).
    /// Needs a live Postgres; ignored by default. Uses a dedicated
    /// schema so the table drop below never races another live queue
    /// test on the shared tables.
    #[tokio::test]
    #[ignore = "needs live Postgres via DATABASE_URL"]
    async fn ensure_schema_concurrent_first_boot_all_succeed() {
        use std::str::FromStr;

        let url =
            std::env::var("DATABASE_URL").expect("export DATABASE_URL for the live queue tests");
        let bootstrap = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect scratch database");
        // A fresh schema per run: the tables must not already exist.
        let schema = format!("race_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&bootstrap)
            .await
            .expect("create test schema");
        bootstrap.close().await;

        // A pool pinned to the test schema: the unqualified queue DDL
        // lands there through `search_path`.
        let options = sqlx::postgres::PgConnectOptions::from_str(&url)
            .expect("parse DATABASE_URL")
            .options([("search_path", schema.as_str())]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .expect("connect test pool");

        // Several fresh rounds: each drops both tables, then releases
        // every task on one barrier so they all enter `ensure_schema`
        // together. A single 23505 in any round fails the test.
        for round in 0..5 {
            sqlx::query("DROP TABLE IF EXISTS rust_job_queue, rust_job_schedule")
                .execute(&pool)
                .await
                .expect("drop queue tables for fresh round");
            let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(8));
            let mut tasks = Vec::new();
            for _ in 0..8 {
                let pool = pool.clone();
                let barrier = barrier.clone();
                tasks.push(tokio::spawn(async move {
                    barrier.wait().await;
                    ensure_schema(&pool).await
                }));
            }
            let mut failures = Vec::new();
            for (i, task) in tasks.into_iter().enumerate() {
                match task.await.expect("spawned task") {
                    Ok(()) => {}
                    Err(error) => failures.push(format!("task {i}: {error:?}")),
                }
            }
            assert!(
                failures.is_empty(),
                "round {round}: concurrent ensure_schema must all succeed: {failures:?}"
            );
            for table in ["rust_job_queue", "rust_job_schedule"] {
                let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
                    .bind(table)
                    .fetch_one(&pool)
                    .await
                    .expect("check table exists");
                assert!(
                    exists,
                    "round {round}: {table} must exist after ensure_schema"
                );
            }
        }
    }
}
