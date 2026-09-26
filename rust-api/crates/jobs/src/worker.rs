#![forbid(unsafe_code)]

//! Worker loop: claim jobs, dispatch, settle.
//!
//! Dispatch is ownership-based (the coexistence rule from the inventory
//! §7: per-task-group routing keeps rollback routing-only). A task name
//! with a registered handler runs locally; any other name is still owned
//! by the Python plane and is forwarded to RabbitMQ in Celery protocol v2
//! ([`crate::amqp`]) so the existing workers execute it unchanged.
//!
//! Nothing is ever dropped: a failed forward or a handler failure
//! requeues with [`DEFAULT_RETRY_DELAY_SECS`][crate::queue] unless the
//! retry budget is spent, in which case the row parks as `failed` with
//! its error text.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{watch, Semaphore};

use crate::amqp::Publisher;
use crate::celery::CeleryTaskMessage;
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

/// A handler error. Plain text: it becomes the row's `last_error`.
pub type HandlerError = String;

/// A local task handler: consumes a claimed row, returns a verdict.
pub type Handler = Arc<
    dyn Fn(JobRow) -> Pin<Box<dyn Future<Output = Result<Verdict, HandlerError>> + Send>>
        + Send
        + Sync,
>;

/// The local handler table. Starts empty: at F-09 every task name is
/// still Python-owned; the D-07…D-10 ports register handlers and flip
/// ownership one task group at a time.
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

/// Where one job goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// A local handler owns the task name.
    Local,
    /// The Python plane still owns it: forward over AMQP.
    PythonOwned,
}

/// Pure routing decision, unit-tested without a database.
pub fn route_for(registry: &Registry, task: &str) -> Route {
    if registry.owns(task) {
        Route::Local
    } else {
        Route::PythonOwned
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
    Forwarded,
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

/// Forward one Python-owned job to the broker. Without a publisher (no
/// broker configured) the job requeues with the default delay: degraded
/// coexistence, never a silent drop.
async fn forward(
    pool: &sqlx::PgPool,
    publisher: Option<&Publisher>,
    job: &JobRow,
) -> Result<DispatchOutcome, sqlx::Error> {
    let message = CeleryTaskMessage {
        id: job.celery_id.clone(),
        task: job.task.clone(),
        args: match &job.args {
            serde_json::Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        },
        kwargs: match &job.kwargs {
            serde_json::Value::Object(map) => map.clone(),
            _ => serde_json::Map::new(),
        },
        retries: 0,
        eta: None,
        expires: None,
        timelimit: (None, None),
        root_id: None,
        parent_id: None,
        origin: None,
    };
    match publisher {
        None => {
            queue::retry(
                pool,
                job.id,
                Utc::now() + chrono::Duration::seconds(DEFAULT_RETRY_DELAY_SECS as i64),
                "no AMQP publisher configured; requeued",
            )
            .await?;
            Ok(DispatchOutcome::Requeued)
        }
        Some(publisher) => match publisher.publish(&message).await {
            Ok(()) => {
                queue::ack(pool, job.id).await?;
                Ok(DispatchOutcome::Forwarded)
            }
            Err(error) => {
                queue::retry(
                    pool,
                    job.id,
                    Utc::now() + chrono::Duration::seconds(DEFAULT_RETRY_DELAY_SECS as i64),
                    &format!("AMQP publish failed: {error}"),
                )
                .await?;
                Ok(DispatchOutcome::Requeued)
            }
        },
    }
}

/// Dispatch one claimed job: run the local handler or forward to Python,
/// then settle the row.
pub async fn dispatch(
    pool: &sqlx::PgPool,
    registry: &Registry,
    publisher: Option<&Publisher>,
    job: JobRow,
) -> Result<DispatchOutcome, sqlx::Error> {
    if route_for(registry, &job.task) == Route::PythonOwned {
        return forward(pool, publisher, &job).await;
    }
    let handler = registry
        .get(&job.task)
        .expect("routed Local, so a handler exists");
    let verdict = handler(job.clone()).await;
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
    match settle_for(&job, &verdict, handler_error.as_deref()) {
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
    publisher: Option<Publisher>,
    config: WorkerConfig,
    mut shutdown: watch::Receiver<bool>,
) {
    let semaphore = Arc::new(Semaphore::new(config.concurrency.max(1)));
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
                tokio::select! {
                    _ = shutdown.wait_for(|stop| *stop) => break,
                    _ = tokio::time::sleep(config.poll_interval) => {}
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
                // One channel per publish (opened inside `publish`), shared
                // connection: failure isolation without reconnecting.
                let publisher = publisher.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let outcome = dispatch(&pool, &registry, publisher.as_ref(), job).await;
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

    #[test]
    fn unregistered_tasks_route_to_python() {
        let registry = Registry::new();
        assert_eq!(
            route_for(&registry, "pi_dash.bgtasks.loop.scan_due_targets"),
            Route::PythonOwned
        );
    }

    #[test]
    fn registered_tasks_route_local() {
        let mut registry = Registry::new();
        registry.register("t", handler_for(Ok(Verdict::Ack)));
        assert_eq!(route_for(&registry, "t"), Route::Local);
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
    fn worker_config_defaults_match_cli() {
        let config = WorkerConfig::default();
        assert_eq!(config.queue, "celery");
        assert_eq!(config.concurrency, 4);
        assert_eq!(config.poll_interval, Duration::from_secs(1));
    }
}
