#![forbid(unsafe_code)]

//! Background jobs: Postgres queue, worker loop, scheduler loop,
//! Celery-format publisher.
//!
//! The worker plane for the Rust backend (F-09), mirroring the Celery
//! setup in `apps/api/pi_dash/celery.py`:
//!
//! - [`queue`]: the `rust_job_queue` table with transactional enqueue
//!   (inside the F-04 [`Transaction`][pidash_db::tx::Transaction]) and
//!   `SKIP LOCKED` claiming.
//! - [`worker`]: the poll loop with a handler registry. Unregistered task
//!   names are still Python-owned and forward to RabbitMQ.
//! - [`schedule`] + [`scheduler`]: the beat-equivalent loop over the 26
//!   `celery.py` entries, singleton via advisory lock.
//! - [`celery`] + [`amqp`]: Celery protocol v2 message construction and
//!   the AMQP publisher for coexistence with the Python workers.
//!
//! Port agents (D-07…D-10) register handlers in [`worker::Registry`];
//! this crate owns the mechanism, never task bodies.

pub mod amqp;
pub mod celery;
pub mod queue;
pub mod schedule;
pub mod scheduler;
pub mod worker;

pub use amqp::{AmqpConfig, AmqpError, Publisher, CELERY_EXCHANGE, CELERY_ROUTING_KEY};
pub use celery::{format_eta, py_repr, AmqpProperties, CeleryTaskMessage};
pub use queue::{
    JobRow, JobStatus, NewJob, DEFAULT_MAX_RETRIES, DEFAULT_QUEUE, DEFAULT_RETRY_DELAY_SECS,
    QUEUE_TABLE,
};
pub use schedule::{beat_schedule, BeatEntry, Cadence, Crontab, ScheduleError};
pub use scheduler::{default_schedule, BEAT_LOCK_KEY, BEAT_TICK, FAILED_RETENTION_SECS};
pub use worker::{DispatchOutcome, Handler, HandlerError, Registry, Route, Verdict, WorkerConfig};

/// Every failure the jobs plane reports.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("broker error: {0}")]
    Amqp(#[from] AmqpError),
    #[error("bad schedule: {0}")]
    Schedule(#[from] ScheduleError),
}
