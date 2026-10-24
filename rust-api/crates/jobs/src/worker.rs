#![forbid(unsafe_code)]

//! Worker loop: claim jobs, dispatch, settle.
//!
//! Dispatch is ownership-based: a task name with a registered handler runs
//! locally. The Rust worker owns every Pi Dash task (the standalone rule),
//! so an unknown name is a loud error — the row parks as `failed` with
//! `unknown task <name>` and a log line is emitted. Nothing is forwarded to
//! a Python worker.
//!
//! A handler failure requeues with [`DEFAULT_RETRY_DELAY_SECS`][crate::queue]
//! unless the retry budget is spent, in which case the row parks as `failed`
//! with its error text.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::future::FutureExt as _;
use sqlx::postgres::PgListener;
use tokio::sync::{watch, Semaphore};

use crate::queue::{self, JobRow, NewJob, DEFAULT_QUEUE, DEFAULT_RETRY_DELAY_SECS};

/// What a handler decides for one claimed job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Done: the row is deleted.
    Ack,
    /// Run again after `delay_secs`: the row returns to `queued` with
    /// `attempts + 1` (or parks as `failed` when the budget is spent).
    Retry { delay_secs: u64 },
    /// No more attempts: park the row as `failed` with this error text.
    Fail { error: String },
}

/// How the worker resolves a task name. There is no Python plane any more
/// (the standalone rule): a name is either owned by a local handler
/// ([`Route::Local`]) or unknown ([`Route::Unknown`]), and an unknown name
/// parks as failed — never forwarded to a broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Local,
    Unknown,
}

/// Resolve `task` against `registry` (see [`Route`]).
pub fn route_for(registry: &Registry, task: &str) -> Route {
    if registry.owns(task) {
        Route::Local
    } else {
        Route::Unknown
    }
}

/// A handler error. Plain text: it becomes the row's `last_error`.
pub type HandlerError = String;

/// A local task handler: consumes a claimed row, returns a verdict.
pub type Handler = Arc<
    dyn Fn(JobRow) -> Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>>
        + Send
        + Sync,
>;

/// The local handler table. Every Pi Dash task name has a registered
/// handler; an unregistered name is a bug the worker parks loudly.
#[derive(Clone, Default)]
pub struct Registry {
    handlers: HashMap<String, Handler>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, task: impl Into<String>, handler: Handler) {
        self.handlers.insert(task.into(), handler);
    }

    pub fn get(&self, task: &str) -> Option<&Handler> {
        self.handlers.get(task)
    }

    /// True when a local handler owns `task`.
    pub fn owns(&self, task: &str) -> bool {
        self.handlers.contains_key(task)
    }
}

/// What settling does with one finished job. Pure: [`settle`] executes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settlement {
    Ack,
    Requeue { delay_secs: u64, error: String },
    Park { error: String },
}

/// Pure settlement decision: a handler verdict plus the retry budget.
pub fn settle_for(job: &JobRow, verdict: &Verdict, handler_error: Option<&str>) -> Settlement {
    match verdict {
        Verdict::Ack => Settlement::Ack,
        Verdict::Fail { error } => Settlement::Park {
            error: error.clone(),
        },
        Verdict::Retry { delay_secs } => {
            if job.retries_left() {
                Settlement::Requeue {
                    delay_secs: *delay_secs,
                    error: handler_error.unwrap_or("retry requested").to_owned(),
                }
            } else {
                Settlement::Park {
                    error: format!(
                        "retry budget spent ({} attempts): {}",
                        job.max_retries,
                        handler_error.unwrap_or("retry requested")
                    ),
                }
            }
        }
    }
}

/// What one dispatch did (observability for logs and tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    Acked,
    Requeued,
    Parked,
}

/// Worker tunables.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub queue: String,
    /// Owner label recorded in `claimed_by` (which worker holds the row).
    pub owner: String,
    /// Idle delay between empty polls; a found job repolls immediately.
    pub poll_interval: Duration,
    /// Jobs running side by side (the `worker --concurrency` flag).
    pub concurrency: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            queue: DEFAULT_QUEUE.to_owned(),
            owner: "pidash-rust".to_owned(),
            poll_interval: Duration::from_secs(1),
            concurrency: 4,
        }
    }
}

/// Readable text out of a caught panic payload: `panic!("{}", …)` and
/// `panic_any` with a `String` carry a `String`, `panic!("…")` and
/// `panic_any` with a literal a `&str`; anything else gets a fixed note
/// rather than a debug dump.
///
/// Takes the box itself, not `&(dyn Any + Send)`: `&Box<dyn Any>` coerces
/// to a trait object *of the box*, whose downcasts never match the
/// payload. Calling downcast on the box derefs to the payload's object.
fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "unknown panic payload".to_owned()
    }
}

/// A caught handler panic as a [`HandlerError`]: the settle path records
/// it in `last_error` and spends the retry budget like any failure.
fn panic_error(payload: &Box<dyn Any + Send>) -> HandlerError {
    format!("handler panicked: {}", panic_message(payload))
}

/// Dispatch one claimed job: run the local handler, then settle the row.
///
/// An unknown task name is a loud error: no handler owns it, so the row is
/// parked as `failed` with `unknown task <name>` (and a log line). The Rust
/// worker owns every Pi Dash task, so this only fires on a bug (an enqueue
/// of a name with no registered handler).
///
/// A panicking handler is settled, never stranded: both the synchronous
/// call and the polled future run under `catch_unwind`, and either panic
/// becomes a handler error (see [`settle_verdict`]).
pub async fn dispatch(
    pool: &sqlx::PgPool,
    registry: &Registry,
    job: JobRow,
) -> Result<DispatchOutcome, sqlx::Error> {
    let Some(handler) = registry.get(&job.task) else {
        let error = format!("unknown task {}", job.task);
        tracing::error!(
            task = job.task.as_str(),
            "unknown task; parking job as failed"
        );
        queue::fail(pool, job.id, &error).await?;
        return Ok(DispatchOutcome::Parked);
    };
    // A panicking handler settles like a failing one (PIDASHCONV-1327):
    // without the guard the spawned task unwinds, ack/retry/fail never
    // run, and the row sits `running` forever. Both the synchronous call
    // and the polled future are guarded — a panic in either becomes a
    // handler error, so `settle_for` retries or parks the row with
    // `last_error`. The guarded future is never polled again after a
    // catch, which is what makes the `AssertUnwindSafe` sound.
    let fut = match catch_unwind(AssertUnwindSafe(|| handler(job.clone()))) {
        Ok(fut) => fut,
        Err(payload) => {
            let error = panic_error(&payload);
            tracing::error!(task = job.task.as_str(), %error, "handler panicked");
            return settle_verdict(pool, &job, Err(error)).await;
        }
    };
    let verdict = match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(verdict) => verdict,
        Err(payload) => {
            let error = panic_error(&payload);
            tracing::error!(task = job.task.as_str(), %error, "handler panicked");
            Err(error)
        }
    };
    settle_verdict(pool, &job, verdict).await
}

/// Turn a handler's verdict (or failure, including a caught panic) into
/// its settlement: ack, requeue with the cause recorded, or park.
async fn settle_verdict(
    pool: &sqlx::PgPool,
    job: &JobRow,
    verdict: Result<Verdict, HandlerError>,
) -> Result<DispatchOutcome, sqlx::Error> {
    let (verdict, handler_error) = match verdict {
        Ok(verdict) => (verdict, None),
        Err(error) => (
            Verdict::Retry {
                delay_secs: DEFAULT_RETRY_DELAY_SECS,
            },
            Some(error),
        ),
    };
    // A handler failure spends the budget exactly like an explicit retry:
    // `settle_for` parks the row once the budget is gone.
    match settle_for(job, &verdict, handler_error.as_deref()) {
        Settlement::Ack => {
            queue::ack(pool, job.id).await?;
            Ok(DispatchOutcome::Acked)
        }
        Settlement::Requeue { delay_secs, error } => {
            queue::retry(
                pool,
                job.id,
                Utc::now() + chrono::Duration::seconds(delay_secs as i64),
                &error,
            )
            .await?;
            Ok(DispatchOutcome::Requeued)
        }
        Settlement::Park { error } => {
            queue::fail(pool, job.id, &error).await?;
            Ok(DispatchOutcome::Parked)
        }
    }
}

/// Run the worker loop until `shutdown` flips to true: claim due jobs and
/// dispatch them with at most `concurrency` in flight. Claim errors are
/// logged with a backoff tick rather than killing the daemon (a transient
/// database failover must not crash-loop the worker; sqlx reconnects the
/// pool on its own). Setting `shutdown` stops polling; in-flight jobs run
/// to settlement before return.
pub async fn run_worker(
    pool: sqlx::PgPool,
    registry: Registry,
    config: WorkerConfig,
    mut shutdown: watch::Receiver<bool>,
) {
    let semaphore = Arc::new(Semaphore::new(config.concurrency.max(1)));
    // Push-wake (PIDASHCONV-476): LISTEN on the enqueue channel so an idle
    // worker claims in ms instead of sleeping out the poll interval. A
    // missed notification only delays a job until the next poll — the sleep
    // below stays as the fallback, and claiming is unchanged.
    let mut listener = match PgListener::connect_with(&pool).await {
        Ok(mut listener) => match listener.listen(queue::JOB_NOTIFY_CHANNEL).await {
            Ok(()) => Some(listener),
            Err(error) => {
                tracing::warn!(%error, "job notify listen failed; using poll fallback");
                None
            }
        },
        Err(error) => {
            tracing::warn!(%error, "job notify listener unavailable; using poll fallback");
            None
        }
    };
    // Boot reclaim (PIDASHCONV-1327): a previous worker may have died
    // mid-job (panic, SIGKILL, crash, redeploy), leaving rows `running`
    // that `claim` will never pick up again. Reclaim them before the
    // first claim; the scheduler loop repeats this every minute for
    // workers that die while already running. A reclaim error must not
    // stop the boot — the loop below (and the scheduler) retries.
    match queue::reclaim_stale_running(&pool, queue::STALE_CLAIM_LEASE_SECS).await {
        Ok(0) => {}
        Ok(reclaimed) => tracing::info!(reclaimed, "worker reclaimed stale running jobs"),
        Err(error) => tracing::warn!(%error, "worker boot reclaim failed; retrying later"),
    }
    loop {
        if *shutdown.borrow() {
            break;
        }
        match queue::claim(&pool, &config.queue, &config.owner).await {
            Err(error) => {
                tracing::warn!(%error, "job claim failed; retrying");
                tokio::select! {
                    _ = shutdown.wait_for(|stop| *stop) => break,
                    _ = tokio::time::sleep(config.poll_interval) => {}
                }
            }
            Ok(None) => {
                if let Some(listener) = listener.as_mut() {
                    tokio::select! {
                        _ = shutdown.wait_for(|stop| *stop) => break,
                        _ = tokio::time::sleep(config.poll_interval) => {}
                        // Any wake re-claims; a foreign-queue notification
                        // costs at most one empty claim. A listener error
                        // just wakes the loop — the claim below plus the
                        // poll sleep keep the worker correct.
                        _ = listener.recv() => {}
                    }
                } else {
                    tokio::select! {
                        _ = shutdown.wait_for(|stop| *stop) => break,
                        _ = tokio::time::sleep(config.poll_interval) => {}
                    }
                }
            }
            Ok(Some(job)) => {
                let permit = semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .expect("semaphore never closes");
                let pool = pool.clone();
                let registry = registry.clone();
                // The handle is deliberately unchecked: settlement happens
                // inside `dispatch` (a handler panic is caught there and
                // settled with `last_error`), and only a dead process can
                // still strand a row — which boot + scheduler reclaim
                // heals (PIDASHCONV-1327). The drain below still waits for
                // every in-flight job via the semaphore.
                tokio::spawn(async move {
                    let _permit = permit;
                    let outcome = dispatch(&pool, &registry, job).await;
                    if let Err(error) = outcome {
                        tracing::warn!(%error, "job settlement failed");
                    }
                });
            }
        }
    }
    // Drain: wait for every in-flight job to settle before returning.
    let permits = config.concurrency.max(1) as u32;
    let _ = semaphore.acquire_many(permits).await;
    tracing::info!("worker loop stopped");
}

/// Enqueue a handler-owned job from anywhere with a pool handle.
/// Request paths inside a transaction use
/// [`enqueue_in`][queue::enqueue_in] instead.
pub async fn enqueue(pool: &sqlx::PgPool, job: &NewJob) -> Result<i64, sqlx::Error> {
    queue::enqueue(pool, job).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(attempts: i32, max_retries: i32) -> JobRow {
        JobRow {
            id: 1,
            celery_id: "task-id-1".to_owned(),
            task: "t".to_owned(),
            args: json!([]),
            kwargs: json!({}),
            queue: "celery".to_owned(),
            status: "running".to_owned(),
            attempts,
            max_retries,
            visible_at: Utc::now(),
            claimed_at: None,
            claimed_by: None,
            created_at: Utc::now(),
            last_error: None,
        }
    }

    fn handler_for(verdict: Result<Verdict, HandlerError>) -> Handler {
        Arc::new(move |_job: JobRow| {
            let verdict = verdict.clone();
            Box::pin(async move { verdict }) as Pin<Box<dyn Future<Output = _> + Send>>
        })
    }

    /// A handler that panics synchronously when invoked (before returning
    /// its future). The never coercion keeps this warning-clean: `panic!`
    /// diverges, so no unreachable arm is needed.
    fn panicking_handler_sync(message: &'static str) -> Handler {
        Arc::new(
            move |_job: JobRow|
                   -> Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>> {
                panic!("{message}")
            },
        )
    }

    /// Diverges with `message`, typed as a handler verdict so both
    /// panicking handlers below stay tail-expression clean (no
    /// unreachable arm for `panic!` to trip `-D warnings` on).
    fn panic_never(message: &'static str) -> Result<Verdict, HandlerError> {
        panic!("{message}")
    }

    /// A handler that panics when its future is first polled.
    fn panicking_handler_async(message: &'static str) -> Handler {
        Arc::new(move |_job: JobRow| {
            Box::pin(async move { panic_never(message) }) as Pin<Box<dyn Future<Output = _> + Send>>
        })
    }

    async fn live_pool() -> sqlx::PgPool {
        let url =
            std::env::var("DATABASE_URL").expect("export DATABASE_URL for the live jobs tests");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect scratch database");
        crate::queue::ensure_schema(&pool)
            .await
            .expect("ensure queue schema");
        pool
    }

    async fn fetch_state(pool: &sqlx::PgPool, id: i64) -> Option<(String, i32, Option<String>)> {
        sqlx::query_as("SELECT status, attempts, last_error FROM rust_job_queue WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .expect("fetch row state")
    }

    async fn wait_for_state(
        pool: &sqlx::PgPool,
        id: i64,
        want: &str,
        what: &str,
    ) -> (String, i32, Option<String>) {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            match fetch_state(pool, id).await {
                Some(state) if state.0 == want => return state,
                Some(_) if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Some(state) => panic!("timed out waiting for {what}; last state: {state:?}"),
                None => panic!("row {id} vanished while waiting for {what}"),
            }
        }
    }

    #[test]
    fn only_registered_tasks_are_owned() {
        let mut registry = Registry::new();
        assert!(!registry.owns("pi_dash.bgtasks.loop.scan_due_targets"));
        registry.register("t", handler_for(Ok(Verdict::Ack)));
        assert!(registry.owns("t"));
        assert!(!registry.owns("other"));
    }

    #[test]
    fn settlement_honors_verdict_and_budget() {
        let fresh = row(0, 3);
        assert_eq!(settle_for(&fresh, &Verdict::Ack, None), Settlement::Ack);
        assert_eq!(
            settle_for(&fresh, &Verdict::Retry { delay_secs: 60 }, None),
            Settlement::Requeue {
                delay_secs: 60,
                error: "retry requested".to_owned()
            }
        );
        assert_eq!(
            settle_for(
                &fresh,
                &Verdict::Fail {
                    error: "boom".to_owned()
                },
                None
            ),
            Settlement::Park {
                error: "boom".to_owned()
            }
        );
        // Spent budget converts a retry into a park, keeping the cause.
        let spent = row(3, 3);
        assert_eq!(
            settle_for(&spent, &Verdict::Retry { delay_secs: 60 }, Some("io down")),
            Settlement::Park {
                error: "retry budget spent (3 attempts): io down".to_owned()
            }
        );
        // A handler error becomes the requeue cause.
        assert_eq!(
            settle_for(&fresh, &Verdict::Retry { delay_secs: 180 }, Some("io down")),
            Settlement::Requeue {
                delay_secs: 180,
                error: "io down".to_owned()
            }
        );
    }

    #[test]
    fn panic_payloads_render_as_text() {
        // `panic!("…")` carries a &str, `panic!("{}", …)` a String.
        let literal = catch_unwind(AssertUnwindSafe(|| panic!("boom-lit"))).unwrap_err();
        assert_eq!(panic_message(&literal), "boom-lit");
        let formatted = catch_unwind(AssertUnwindSafe(|| panic!("{}", "boom-fmt"))).unwrap_err();
        assert_eq!(panic_message(&formatted), "boom-fmt");
        let string = catch_unwind(AssertUnwindSafe(|| {
            std::panic::panic_any("boom-string".to_owned())
        }))
        .unwrap_err();
        assert_eq!(panic_message(&string), "boom-string");
        let borrowed =
            catch_unwind(AssertUnwindSafe(|| std::panic::panic_any("boom-str"))).unwrap_err();
        assert_eq!(panic_message(&borrowed), "boom-str");
        let exotic = catch_unwind(AssertUnwindSafe(|| std::panic::panic_any(42_i32))).unwrap_err();
        assert_eq!(panic_message(&exotic), "unknown panic payload");
        assert!(panic_error(&literal).starts_with("handler panicked: boom-lit"));
    }

    #[test]
    fn worker_config_defaults_match_cli() {
        let config = WorkerConfig::default();
        assert_eq!(config.queue, "celery");
        assert_eq!(config.concurrency, 4);
        assert_eq!(config.poll_interval, Duration::from_secs(1));
    }

    /// Push-wake pacing (PIDASHCONV-476): an idle worker claims a newly
    /// enqueued job in ms, without sleeping out `poll_interval` (5s here, so
    /// a poll-bound worker fails this test at the 2s timeout).
    /// Needs a live Postgres — export `DATABASE_URL` on its own line first.
    /// Ignored by default so CI without a database stays green.
    #[tokio::test]
    #[ignore = "needs live Postgres via DATABASE_URL"]
    async fn notify_wake_claims_without_poll_delay() {
        let url =
            std::env::var("DATABASE_URL").expect("export DATABASE_URL for the live notify test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect scratch database");
        crate::queue::ensure_schema(&pool)
            .await
            .expect("ensure queue schema");
        // A unique queue: no other worker or leftover row can interfere.
        let queue = format!("test-notify-{}", uuid::Uuid::new_v4());
        let (fired_tx, fired_rx) = tokio::sync::oneshot::channel::<()>();
        let fired_tx = Arc::new(tokio::sync::Mutex::new(Some(fired_tx)));
        let handler: Handler = Arc::new(move |_job: JobRow| {
            let fired_tx = fired_tx.clone();
            Box::pin(async move {
                if let Some(tx) = fired_tx.lock().await.take() {
                    let _ = tx.send(());
                }
                Ok(Verdict::Ack)
            }) as Pin<Box<dyn Future<Output = _> + Send>>
        });
        let mut registry = Registry::new();
        registry.register("test.notify.probe", handler);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(run_worker(
            pool.clone(),
            registry,
            WorkerConfig {
                queue: queue.clone(),
                owner: "notify-probe".to_owned(),
                poll_interval: Duration::from_secs(5),
                concurrency: 1,
            },
            shutdown_rx,
        ));
        // Let the worker settle into its idle wait before enqueueing.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let started = std::time::Instant::now();
        crate::queue::enqueue(
            &pool,
            &crate::queue::NewJob {
                task: "test.notify.probe".to_owned(),
                args: serde_json::json!([]),
                kwargs: serde_json::json!({}),
                queue,
                visible_at: None,
                max_retries: crate::queue::DEFAULT_MAX_RETRIES,
            },
        )
        .await
        .expect("enqueue probe job");
        let claimed = tokio::time::timeout(Duration::from_secs(2), fired_rx).await;
        let elapsed = started.elapsed();
        let _ = shutdown_tx.send(true);
        worker.await.expect("worker joins after shutdown");
        let _ = claimed.expect("worker must claim via push-wake, not the 5s poll");
        assert!(
            elapsed < Duration::from_secs(2),
            "claim took {elapsed:?}; push-wake should fire in ms"
        );
    }

    /// Panic settlement (PIDASHCONV-1327): a panicking handler — whether it
    /// panics synchronously on invocation or asynchronously on first poll —
    /// settles like a failing one: the row returns to `queued` with
    /// `attempts + 1` and the panic message in `last_error`, instead of
    /// sitting `running` forever. Needs a live Postgres; ignored by default.
    #[tokio::test]
    #[ignore = "needs live Postgres via DATABASE_URL"]
    async fn panicking_handler_requeues_with_last_error() {
        let pool = live_pool().await;
        for (name, handler, message) in [
            (
                "sync",
                panicking_handler_sync("boom-sync-1327"),
                "boom-sync-1327",
            ),
            (
                "async",
                panicking_handler_async("boom-async-1327"),
                "boom-async-1327",
            ),
        ] {
            let queue = format!("test-panic-{name}-{}", uuid::Uuid::new_v4());
            let mut registry = Registry::new();
            registry.register("test.panic.probe", handler);
            let id = crate::queue::enqueue(
                &pool,
                &crate::queue::NewJob {
                    task: "test.panic.probe".to_owned(),
                    args: serde_json::json!([]),
                    kwargs: serde_json::json!({}),
                    queue: queue.clone(),
                    visible_at: None,
                    max_retries: crate::queue::DEFAULT_MAX_RETRIES,
                },
            )
            .await
            .expect("enqueue probe job");
            let job = crate::queue::claim(&pool, &queue, "panic-probe")
                .await
                .expect("claim")
                .expect("job claimed");
            assert_eq!(job.id, id);
            let outcome = dispatch(&pool, &registry, job)
                .await
                .expect("dispatch settles");
            assert_eq!(
                outcome,
                DispatchOutcome::Requeued,
                "{name} panic must settle"
            );
            let (status, attempts, last_error) =
                fetch_state(&pool, id).await.expect("row still present");
            assert_eq!(status, "queued", "{name} panic must requeue, not strand");
            assert_eq!(attempts, 1, "{name} panic must spend one attempt");
            let error = last_error.expect("panic must be recorded");
            assert!(
                error.contains("handler panicked") && error.contains(message),
                "{name} last_error must carry the panic: {error}"
            );
        }
    }

    /// Panic settlement on a spent budget (PIDASHCONV-1327): with no
    /// retries left the panicking row parks as `failed`, keeping both the
    /// budget note and the panic message. Needs a live Postgres; ignored
    /// by default.
    #[tokio::test]
    #[ignore = "needs live Postgres via DATABASE_URL"]
    async fn panicking_handler_parks_when_budget_spent() {
        let pool = live_pool().await;
        let queue = format!("test-panic-spent-{}", uuid::Uuid::new_v4());
        let mut registry = Registry::new();
        registry.register(
            "test.panic.probe",
            panicking_handler_async("boom-spent-1327"),
        );
        let id = crate::queue::enqueue(
            &pool,
            &crate::queue::NewJob {
                task: "test.panic.probe".to_owned(),
                args: serde_json::json!([]),
                kwargs: serde_json::json!({}),
                queue: queue.clone(),
                visible_at: None,
                max_retries: 0,
            },
        )
        .await
        .expect("enqueue probe job");
        let job = crate::queue::claim(&pool, &queue, "panic-probe")
            .await
            .expect("claim")
            .expect("job claimed");
        let outcome = dispatch(&pool, &registry, job)
            .await
            .expect("dispatch settles");
        assert_eq!(outcome, DispatchOutcome::Parked);
        let (status, attempts, last_error) = fetch_state(&pool, id).await.expect("row present");
        assert_eq!(status, "failed");
        assert_eq!(attempts, 0, "a park must not spend an attempt");
        let error = last_error.expect("panic must be recorded");
        assert!(
            error.contains("retry budget spent") && error.contains("boom-spent-1327"),
            "parked panic must keep budget note and cause: {error}"
        );
    }

    /// Worker survival (PIDASHCONV-1327): the issue's repro — a panicking
    /// job under `run_worker` settles (no retries here, so `failed` with
    /// `last_error`) and the worker goes on to ack the next job. Needs a
    /// live Postgres; ignored by default.
    #[tokio::test]
    #[ignore = "needs live Postgres via DATABASE_URL"]
    async fn run_worker_survives_panicking_handler() {
        let pool = live_pool().await;
        let queue = format!("test-survive-{}", uuid::Uuid::new_v4());
        let mut registry = Registry::new();
        registry.register(
            "test.panic.probe",
            panicking_handler_async("boom-worker-1327"),
        );
        registry.register("test.healthy.probe", handler_for(Ok(Verdict::Ack)));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(run_worker(
            pool.clone(),
            registry,
            WorkerConfig {
                queue: queue.clone(),
                owner: "survive-probe".to_owned(),
                poll_interval: Duration::from_millis(50),
                concurrency: 1,
            },
            shutdown_rx,
        ));
        let panic_id = crate::queue::enqueue(
            &pool,
            &crate::queue::NewJob {
                task: "test.panic.probe".to_owned(),
                args: serde_json::json!([]),
                kwargs: serde_json::json!({}),
                queue: queue.clone(),
                visible_at: None,
                max_retries: 0,
            },
        )
        .await
        .expect("enqueue panicking job");
        let (status, _, last_error) =
            wait_for_state(&pool, panic_id, "failed", "panicking row to settle").await;
        assert_eq!(status, "failed");
        let error = last_error.expect("panic must be recorded");
        assert!(
            error.contains("handler panicked") && error.contains("boom-worker-1327"),
            "unexpected last_error: {error}"
        );
        // The worker is still alive: a healthy job published after the
        // panic is claimed and acked (the row is deleted).
        let healthy_id = crate::queue::enqueue(
            &pool,
            &crate::queue::NewJob {
                task: "test.healthy.probe".to_owned(),
                args: serde_json::json!([]),
                kwargs: serde_json::json!({}),
                queue: queue.clone(),
                visible_at: None,
                max_retries: crate::queue::DEFAULT_MAX_RETRIES,
            },
        )
        .await
        .expect("enqueue healthy job");
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if fetch_state(&pool, healthy_id).await.is_none() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not survive the panic: healthy job never acked"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let _ = shutdown_tx.send(true);
        worker.await.expect("worker joins after shutdown");
    }

    /// Boot reclaim (PIDASHCONV-1327): a row left `running` with a stale
    /// `claimed_at` (a worker that died mid-job) is reclaimed when a worker
    /// boots, then processed normally — here the unknown name parks it as
    /// `failed` with its cause. Needs a live Postgres; ignored by default.
    #[tokio::test]
    #[ignore = "needs live Postgres via DATABASE_URL"]
    async fn run_worker_boot_reclaims_stale_running_row() {
        let pool = live_pool().await;
        let queue = format!("test-reclaim-{}", uuid::Uuid::new_v4());
        let task = format!("test.unknown.{}", uuid::Uuid::new_v4());
        let id = crate::queue::enqueue(
            &pool,
            &crate::queue::NewJob {
                task: task.clone(),
                args: serde_json::json!([]),
                kwargs: serde_json::json!({}),
                queue: queue.clone(),
                visible_at: None,
                max_retries: crate::queue::DEFAULT_MAX_RETRIES,
            },
        )
        .await
        .expect("enqueue probe job");
        // A dead worker's footprint: claimed long ago, never settled.
        let claimed = crate::queue::claim(&pool, &queue, "dead-worker")
            .await
            .expect("claim")
            .expect("job claimed");
        assert_eq!(claimed.id, id);
        sqlx::query(
            "UPDATE rust_job_queue \
             SET claimed_at = now() - make_interval(hours => 2), claimed_by = 'dead-worker' \
             WHERE id = $1",
        )
        .bind(id)
        .execute(&pool)
        .await
        .expect("backdate claim");
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = tokio::spawn(run_worker(
            pool.clone(),
            Registry::new(),
            WorkerConfig {
                queue: queue.clone(),
                owner: "reclaim-probe".to_owned(),
                poll_interval: Duration::from_millis(50),
                concurrency: 1,
            },
            shutdown_rx,
        ));
        let (status, attempts, last_error) =
            wait_for_state(&pool, id, "failed", "stale row to be reclaimed and parked").await;
        assert_eq!(status, "failed");
        assert!(
            attempts >= 1,
            "reclaim must spend an attempt, got {attempts}"
        );
        let error = last_error.expect("parked row must carry its cause");
        assert!(
            error.contains("unknown task"),
            "reclaimed row must be processed normally, got: {error}"
        );
        let _ = shutdown_tx.send(true);
        worker.await.expect("worker joins after shutdown");
    }
}
