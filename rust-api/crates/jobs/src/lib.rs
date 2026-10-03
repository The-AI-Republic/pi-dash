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
/// Page-view publish envelopes (D-30, PIDASHCONV-293).
pub mod app_pages;
/// Assistant turn pipeline + stale-turn sweep (D-06, PIDASHCONV-254).
pub mod assistant;
pub mod celery;
/// Creation + dispatch tasks (D-11 L6, PIDASHCONV-487): the executor-aware
/// creation seam, the lease-and-offer dispatch engine with transactional
/// enqueue, and the bounded run-event append. Lives at
/// `dispatch/dispatch.rs` via an explicit path — the single file the issue
/// owns, no `mod.rs`.
#[path = "dispatch/dispatch.rs"]
pub mod dispatch;
/// Execution tasks (D-11 L7, PIDASHCONV-488): claim/run/fail, queue
/// scan, stale sweep, managed wait expiry, and the model runtime.
/// Same explicit-path form as [`dispatch`] — the single file the issue
/// owns, no `mod.rs`.
#[path = "dispatch/execute.rs"]
pub mod dispatch_execute;
pub mod integrations;
/// Iterative compact-JSON encoder (PIDASHCONV-626): stack-safe
/// `serde_json::to_vec` for the ~9900-deep payloads ports may enqueue.
pub mod json_compact;
/// Loop turn dispatch: thread rotation + turn creation + run enqueue
/// (D-03, PIDASHCONV-157): same explicit-path form as [`loop_scan`].
#[path = "loop/dispatch.rs"]
pub mod loop_dispatch;
/// Loop per-target fire claim + re-check (D-03, PIDASHCONV-157): same
/// explicit-path form as [`loop_scan`].
#[path = "loop/fire.rs"]
pub mod loop_fire;
/// Loop auto-PM beat scanner (D-03, PIDASHCONV-156). `loop` is a Rust
/// keyword, so the module lives at `loop/scan.rs` via an explicit path —
/// the single file the issue owns, no `mod.rs`.
#[path = "loop/scan.rs"]
pub mod loop_scan;
pub mod queue;
pub mod schedule;
pub mod scheduler;
pub mod space;
pub mod tasks_cleanup;
/// Issue-export task wire (D-35, PIDASHCONV-381): Celery name, arg
/// binding, delay constructor, filenames, status + filter SQL.
pub mod tasks_export;
pub mod tasks_mail;
pub mod tasks_ticker;
pub mod tasks_webhooks;
/// api-v1 asset + intake task publishers (D-21, PIDASHCONV-417): Celery
/// wire surface for the four `.delay()` units, no task bodies.
pub mod v1_assets;
/// api-v1 cycles + modules task publishers (D-20, PIDASHCONV-310): Celery
/// wire surface for the eleven `.delay()` units, no task bodies.
pub mod v1_cycles_modules;
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
