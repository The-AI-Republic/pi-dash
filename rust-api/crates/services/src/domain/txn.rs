//! The domain transaction: a SeaORM transaction plus the after-commit
//! queue and the hook registry.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, ExecResult,
    QueryResult, Statement, StatementBuilder, TransactionTrait, Value,
};

use super::error::DomainError;

/// One deferred after-commit step: async, because steps call async
/// transition and enqueue functions. Steps are `Send + Sync` so a
/// `&WriteTxn` can cross an `.await` inside the [`crate::domain::run`]
/// work future (captures are owned payloads and `Arc`s, so this binds
/// nothing in practice).
type CommitStep =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static>;

/// The deferred-step queue. The [`pidash_db::tx::AfterCommit`] semantics —
/// steps run in order, exactly once, after a successful commit; a rollback
/// or a dropped queue runs nothing — with async steps (see the
/// [`super`] module doc for why the drain is awaited).
///
/// Django correspondence (`django/db/transaction.py`,
/// `django/db/backends/base/base.py`; verified against Django 6.0.5):
///
/// * Order: registration order, like Django's `run_and_clear_commit_hooks`
///   (`base.py`), which pops callbacks FIFO.
/// * Failure: steps are infallible (`Output = ()`), so a failing effect
///   records its failure and returns — the drain continues with the next
///   step. That is Django's `robust=True` arm (log-and-continue). A step
///   that panics unwinds through the drain: the steps after it never run,
///   but the write stays committed, because the commit precedes the drain
///   — Django's default `robust=False` arm (`transaction.py:129`), where
///   the exception propagates and the rest are skipped. Queue only
///   infallible work; fallible effects (enqueues, drains) handle their
///   errors internally and warn (the `enqueue_completion_comment`
///   best-effort precedent).
/// * Scope: the queue belongs to the outermost commit only. Unlike Django,
///   which drops callbacks registered inside a rolled-back savepoint
///   (`savepoint_rollback`, `base.py`), `WriteTxn` has no nested scopes
///   (see below), so there is nothing to drop.
/// * No open transaction: Django runs `on_commit` immediately in autocommit
///   (`on_commit`, `base.py`). `domain::run` always opens a transaction,
///   so queued steps always run post-commit, awaited before `run` returns.
#[derive(Default)]
pub struct CommitQueue {
    steps: Vec<CommitStep>,
}

impl CommitQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue an async step for after commit (Django's `on_commit`).
    pub fn on_commit<F, Fut>(&mut self, step: F)
    where
        F: FnOnce() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.steps.push(Box::new(move || Box::pin(step())));
    }

    /// Queue a sync step for after commit. Small adapter over [`on_commit`]:
    /// most steps are async, the odd counter bump is not.
    pub fn on_commit_fn(&mut self, step: impl FnOnce() + Send + Sync + 'static) {
        self.on_commit(|| async move { step() });
    }

    /// Run every queued step in order, awaiting each. Consumes the queue,
    /// so steps run at most once.
    pub async fn drain(mut self) {
        for step in self.steps.drain(..) {
            step().await;
        }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// The trait-object hook registry. Hook traits (scheduler seeding, the
/// issue transition) live in `pidash-services` next to the domain code that
/// invokes them; implementations live wherever their dependencies live
/// (services, jobs, the api binary). Registration and lookup are typed by
/// the `Arc` payload — concrete or `dyn Trait` alike — so a missing hook
/// is `None`, never a mis-cast.
#[derive(Default)]
pub struct HookRegistry {
    hooks: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T: ?Sized + Send + Sync + 'static>(&mut self, hook: Arc<T>) {
        self.hooks.insert(TypeId::of::<Arc<T>>(), Box::new(hook));
    }

    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.hooks
            .get(&TypeId::of::<Arc<T>>())
            .and_then(|hook| hook.downcast_ref::<Arc<T>>())
            .cloned()
    }
}

/// The query handle for a [`WriteTxn`]: [`ConnectionTrait`] without
/// [`TransactionTrait`], so entity calls, selects and [`WriteTxn`]'s own
/// `exec_*` helpers all run, but a nested `begin()` is a compile error.
///
/// Nested scopes are unsupported on purpose. SeaORM's nested `begin()`
/// (`TransactionTrait for DatabaseTransaction`, sea-orm 1.1.20
/// `database/transaction.rs`) issues a savepoint through sqlx's
/// transaction-depth counter, and Django drops `on_commit` callbacks
/// registered inside a rolled-back savepoint (`savepoint_rollback`,
/// `django/db/backends/base/base.py`) — but the [`CommitQueue`] cannot
/// observe a savepoint rollback, so steps queued inside one would wrongly
/// survive it. Until a `WriteTxn` savepoint API with queue scoping exists,
/// savepoints stay out: `SAVEPOINT`/`RELEASE`/`ROLLBACK TO` SQL through
/// [`WriteTxn::exec_raw`] is likewise forbidden.
///
/// `Copy`: the handle is a shared borrow, so sub-seams (the move's
/// handoff-creation seam) can hold it alongside the owning store.
#[derive(Clone, Copy)]
pub struct TxnConn<'t> {
    inner: &'t DatabaseTransaction,
}

#[async_trait::async_trait]
impl ConnectionTrait for TxnConn<'_> {
    fn get_database_backend(&self) -> DbBackend {
        self.inner.get_database_backend()
    }

    async fn execute(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        self.inner.execute(stmt).await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        self.inner.execute_unprepared(sql).await
    }

    async fn query_one(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        self.inner.query_one(stmt).await
    }

    async fn query_all(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        self.inner.query_all(stmt).await
    }
}

/// A SeaORM transaction plus its after-commit queue and hook registry.
///
/// The queue is outermost-commit-only (see [`CommitQueue`]): this type has
/// no nested constructor, and [`WriteTxn::connection`] exposes queries
/// without transaction control, so savepoint-scoped work cannot be
/// expressed by accident.
pub struct WriteTxn {
    txn: Option<DatabaseTransaction>,
    queue: CommitQueue,
    hooks: HookRegistry,
}

impl WriteTxn {
    pub async fn begin(db: &DatabaseConnection) -> Result<Self, DomainError> {
        Ok(Self {
            txn: Some(db.begin().await?),
            queue: CommitQueue::new(),
            hooks: HookRegistry::new(),
        })
    }

    /// The query handle for entity calls and [`ConnectionTrait`] queries:
    /// [`TxnConn`] deliberately omits [`TransactionTrait`], so a nested
    /// `begin()` (a savepoint the [`CommitQueue`] could not observe) is a
    /// compile error. Panics once the transaction has finished, like
    /// [`pidash_db::tx::Transaction::inner`].
    pub fn connection(&self) -> TxnConn<'_> {
        TxnConn {
            inner: self.txn.as_ref().expect("transaction already finished"),
        }
    }

    /// Queue an async step for after commit (Django's `on_commit`).
    pub fn on_commit<F, Fut>(&mut self, step: F)
    where
        F: FnOnce() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.queue.on_commit(step);
    }

    /// Queue a sync step for after commit.
    pub fn on_commit_fn(&mut self, step: impl FnOnce() + Send + Sync + 'static) {
        self.queue.on_commit_fn(step);
    }

    pub fn hooks(&self) -> &HookRegistry {
        &self.hooks
    }

    pub fn hooks_mut(&mut self) -> &mut HookRegistry {
        &mut self.hooks
    }

    /// Run an existing sea-query builder on the transaction (design D5).
    pub async fn exec_builder<S: StatementBuilder>(&self, stmt: &S) -> Result<ExecResult, DbErr> {
        let statement = DbBackend::Postgres.build(stmt);
        self.connection().execute(statement).await
    }

    /// Run raw SQL with bound values on the transaction (design D5):
    /// advisory locks, `SELECT … FOR UPDATE`, surviving SQL constants.
    pub async fn exec_raw<T>(&self, sql: &str, values: T) -> Result<ExecResult, DbErr>
    where
        T: IntoIterator<Item = Value>,
    {
        let statement = Statement::from_sql_and_values(DbBackend::Postgres, sql, values);
        self.connection().execute(statement).await
    }

    /// `SELECT pg_advisory_xact_lock($1)`: the transaction-scoped
    /// cooperative lock the create paths take (the sqlx
    /// `SELECT pg_advisory_xact_lock($1)` precedent in the api handlers),
    /// released automatically at commit or rollback.
    pub async fn advisory_xact_lock(&self, key: i64) -> Result<(), DbErr> {
        self.exec_raw("SELECT pg_advisory_xact_lock($1)", [Value::from(key)])
            .await?;
        Ok(())
    }

    /// Commit, then drain the queued steps in order. Steps never run when
    /// the commit itself fails.
    pub async fn commit(mut self) -> Result<(), DomainError> {
        let txn = self.txn.take().expect("transaction already finished");
        txn.commit().await?;
        self.queue.drain().await;
        Ok(())
    }

    /// Roll back and discard the queued steps without running them.
    pub async fn rollback(mut self) -> Result<(), DomainError> {
        let txn = self.txn.take().expect("transaction already finished");
        txn.rollback().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[tokio::test]
    async fn queued_steps_drain_in_order_exactly_once() {
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let mut queue = CommitQueue::new();
        for step in ["first", "second"] {
            let log = Arc::clone(&log);
            queue.on_commit(move || async move {
                log.lock().expect("lock").push(step);
            });
        }
        let log_sync = Arc::clone(&log);
        queue.on_commit_fn(move || log_sync.lock().expect("lock").push("third"));
        assert_eq!(queue.len(), 3);
        queue.drain().await;
        assert_eq!(*log.lock().expect("lock"), vec!["first", "second", "third"]);
    }

    /// A panicking step unwinds the drain (Django's default
    /// `robust=False`: the steps after it never run). The write stays
    /// committed — the panic happens after the commit — which the live
    /// test below proves with a real row.
    #[tokio::test]
    async fn panicking_step_unwinds_drain_and_skips_the_rest() {
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let mut queue = CommitQueue::new();
        let log_panic = Arc::clone(&log);
        queue.on_commit(move || async move {
            log_panic.lock().expect("lock").push("before-panic");
            panic!("step failure unwinds the drain");
        });
        let log_skipped = Arc::clone(&log);
        queue.on_commit(move || async move {
            log_skipped.lock().expect("lock").push("skipped");
        });
        let failed = tokio::spawn(queue.drain()).await;
        let error = failed.expect_err("drain unwinds");
        assert!(error.is_panic());
        assert_eq!(*log.lock().expect("lock"), vec!["before-panic"]);
    }

    #[tokio::test]
    async fn dropped_queue_runs_nothing() {
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        {
            let mut queue = CommitQueue::new();
            let log = Arc::clone(&log);
            queue.on_commit(|| async move {
                log.lock().expect("lock").push("lost");
            });
            drop(queue);
        }
        assert!(log.lock().expect("lock").is_empty());
        assert!(CommitQueue::new().is_empty());
    }

    struct SeedingHook {
        calls: Mutex<Vec<String>>,
    }

    struct OtherHook;

    #[test]
    fn hook_registry_round_trips_by_type() {
        let mut registry = HookRegistry::new();
        assert!(registry.get::<SeedingHook>().is_none());
        let hook = Arc::new(SeedingHook {
            calls: Mutex::new(Vec::new()),
        });
        registry.register(Arc::clone(&hook));
        let found = registry.get::<SeedingHook>().expect("registered hook");
        found.calls.lock().expect("lock").push("seed".to_owned());
        assert_eq!(hook.calls.lock().expect("lock").as_slice(), ["seed"]);
        assert!(registry.get::<OtherHook>().is_none());
    }

    /// Live-db half: commit drains, rollback drops, and the advisory lock
    /// runs on the SeaORM transaction. Like every other live-db test in the
    /// workspace it skips when `DATABASE_URL` is unset and runs for real
    /// otherwise.
    async fn live_db() -> Option<DatabaseConnection> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to scratch DATABASE_URL");
        Some(pidash_db::sea_orm_connection(&pool))
    }

    #[tokio::test]
    async fn commit_drains_and_advisory_lock_runs() {
        let Some(db) = live_db().await else { return };
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let ctx = crate::domain::WriteCtx::default();
        let stepped = Arc::clone(&log);
        crate::domain::run(&ctx, &db, |txn| {
            Box::pin(async move {
                txn.advisory_xact_lock(0x5eed_1520)
                    .await
                    .map_err(DomainError::Db)?;
                txn.on_commit(move || async move {
                    stepped.lock().expect("lock").push("drained");
                });
                Ok::<(), DomainError>(())
            })
        })
        .await
        .expect("commit");
        assert_eq!(*log.lock().expect("lock"), vec!["drained"]);
    }

    /// A step that fails internally (records the failure, returns `()`)
    /// never rolls the write back: the commit precedes the drain, and the
    /// steps after it still run (Django's `robust=True` arm).
    #[tokio::test]
    async fn failing_step_never_rolls_back_the_write() {
        let Some(db) = live_db().await else { return };
        db.execute(Statement::from_string(
            DbBackend::Postgres,
            "CREATE TABLE IF NOT EXISTS domain_writetxn_probe \
             (id UUID PRIMARY KEY, note TEXT NOT NULL)"
                .to_owned(),
        ))
        .await
        .expect("probe table");
        let id = uuid::Uuid::new_v4();
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let ctx = crate::domain::WriteCtx::default();
        let failed = Arc::clone(&log);
        let ran = Arc::clone(&log);
        crate::domain::run(&ctx, &db, |txn| {
            Box::pin(async move {
                txn.exec_raw(
                    "INSERT INTO domain_writetxn_probe (id, note) VALUES ($1, $2)",
                    [Value::from(id), Value::from("committed")],
                )
                .await
                .map_err(DomainError::Db)?;
                txn.on_commit(move || async move {
                    failed.lock().expect("lock").push("failed-effect");
                });
                txn.on_commit(move || async move {
                    ran.lock().expect("lock").push("next-step");
                });
                Ok::<(), DomainError>(())
            })
        })
        .await
        .expect("commit");
        assert_eq!(
            *log.lock().expect("lock"),
            vec!["failed-effect", "next-step"]
        );
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT note FROM domain_writetxn_probe WHERE id = $1",
                [Value::from(id)],
            ))
            .await
            .expect("read back")
            .expect("committed row");
        let note: String = row.try_get("", "note").expect("note decodes");
        assert_eq!(note, "committed");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM domain_writetxn_probe WHERE id = $1",
            [Value::from(id)],
        ))
        .await
        .expect("cleanup");
    }

    #[tokio::test]
    async fn rollback_drops_queue_and_answers_work_error() {
        let Some(db) = live_db().await else { return };
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let ctx = crate::domain::WriteCtx::default();
        let stepped = Arc::clone(&log);
        let result = crate::domain::run(&ctx, &db, |txn| {
            Box::pin(async move {
                txn.on_commit(move || async move {
                    stepped.lock().expect("lock").push("lost");
                });
                Err::<(), DomainError>(DomainError::Work("nope".to_owned()))
            })
        })
        .await;
        assert!(matches!(result, Err(DomainError::Work(message)) if message == "nope"));
        assert!(log.lock().expect("lock").is_empty());
    }
}
