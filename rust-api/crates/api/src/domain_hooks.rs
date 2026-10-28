#![forbid(unsafe_code)]

//! The api binary's task-enqueue hook (ORM-03, PIDASHCONV-1521 part A).
//!
//! [`AppTaskEnqueueHook`] is the [`TaskEnqueueHook`][pidash_services::domain::activity::TaskEnqueueHook]
//! binding over `pidash_jobs::queue::enqueue`. Handlers that drive domain
//! writes register it on the [`WriteTxn`][pidash_services::domain::WriteTxn]
//! hook registry (or hold it directly for pool-based bulk calls) so domain
//! chains can enqueue tasks without depending on the jobs crate.

use std::future::Future;
use std::pin::Pin;

use pidash_jobs::queue::{self, NewJob};
use pidash_jobs::TaskMessage;
use pidash_services::domain::activity::TaskEnqueueHook;
use serde_json::{Map, Value};
use sqlx::PgPool;

/// `task.delay(*args, **kwargs)` through the jobs kernel: unported tasks
/// forward Celery-format, like every other port. Enqueue failures log and
/// the response stands (Django's broker publish is equally fire-and-forget
/// from the view's perspective). Moved verbatim from the app-issue
/// handlers' local `enqueue_task`.
pub struct AppTaskEnqueueHook {
    pool: PgPool,
}

impl AppTaskEnqueueHook {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl TaskEnqueueHook for AppTaskEnqueueHook {
    fn enqueue_task(
        &self,
        task: &str,
        args: Vec<Value>,
        kwargs: Map<String, Value>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let pool = self.pool.clone();
        let message = TaskMessage::new(task, args, kwargs);
        Box::pin(async move {
            let job = NewJob::new(
                message.task.clone(),
                Value::Array(message.args.clone()),
                Value::Object(message.kwargs.clone()),
            );
            if let Err(error) = queue::enqueue(&pool, &job).await {
                tracing::warn!(%error, task = message.task.as_str(), "task enqueue failed; response stands");
            }
        })
    }
}
