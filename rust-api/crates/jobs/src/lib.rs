#![forbid(unsafe_code)]

//! Background jobs: Postgres queue, worker loop, scheduler loop.
//!
//! The worker plane for the Rust backend (F-09), mirroring the Celery
//! setup in `apps/api/pi_dash/celery.py`:
//!
//! - [`queue`]: the `rust_job_queue` table with transactional enqueue
//!   (inside the F-04 [`Transaction`][pidash_db::tx::Transaction]) and
//!   `SKIP LOCKED` claiming.
//! - [`worker`]: the poll loop with a handler registry. Every Pi Dash task
//!   name has a registered handler; an unknown name parks as failed
//!   (`unknown task <name>`), never forwarded to Python.
//! - [`schedule`] + [`scheduler`]: the beat-equivalent loop over the 26
//!   `celery.py` entries, singleton via advisory lock.
//! - [`task_message`]: the `.delay()`-shaped envelope a call site builds
//!   before enqueueing onto [`queue`].
//!
//! Port agents (D-07…D-10) register handlers in [`worker::Registry`];
//! this crate owns the mechanism, never task bodies.

/// Redis-backed cloud-agent admission cache (PIDASHCONV-1487).
pub mod admission;
/// Page-view publish envelopes (D-30, PIDASHCONV-293).
pub mod app_pages;
/// Assistant turn pipeline + stale-turn sweep (D-06, PIDASHCONV-254).
pub mod assistant;
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
/// Public domain-write hooks (OVL-16, PIDASHCONV-1660): OSS's own
/// transition / enqueue / move-drain bindings over explicit deps, for
/// any caller of `operations::*` and the domain writes. The api
/// `App*` and worker `Worker*` types wrap these.
pub mod hooks;
pub mod integrations;
/// Issue post-save orchestration (PIDASHCONV-1520): the completion
/// comment, the transition fire core, and the pool-direct ticker seams.
/// Moved here from `pidash-api::issue_transition` so a `WriteTxn`
/// after-commit step can call them; the api module keeps delegating
/// wrappers over its own drains.
pub mod issue_transition;
/// Iterative compact-JSON encoder (PIDASHCONV-626): stack-safe
/// `serde_json::to_vec` for the ~9900-deep payloads ports may enqueue.
pub mod json_compact;
/// License / instance-console job units (D-01, PIDASHCONV-119):
/// `instance_traces` plus the `configure_instance` / `register_instance`
/// command logic. Wired here (PIDASHCONV-809) so the ops `instance`
/// group can execute it; the module was merged unwired with wiring
/// deferred, and no consumer existed until now.
pub mod license;
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
pub mod orchestration;
/// OSS worker composition entry points (PIDASHCONV-918): the
/// [`register_oss_handlers`][oss_worker::register_oss_handlers] helper
/// plus the live seam providers both binaries share.
pub mod oss_worker;
pub mod pyformat;
pub mod queue;
/// Runner revoke/delete executor (OVL-06, PIDASHCONV-1544): the revoke
/// bundle as one public service call over the services drivers.
pub mod runner_enroll;
/// Runner task executors (D-15 L6, PIDASHCONV-539/540): the executing
/// half of `runner/tasks.py` over the L4/L5 planners — approval expiry
/// + runner/session sweeps (L6a) and dedupe + chat sweeps +
/// stall-terminal reconcile (L6b).
pub mod runner_runs;
pub mod schedule;
pub mod scheduler;
pub mod space;
pub mod task_message;
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

pub use oss_worker::{register_oss_handlers, OssWorkerCtx};
pub use pyformat::{format_eta, py_repr};
pub use queue::{
    JobRow, JobStatus, NewJob, DEFAULT_MAX_RETRIES, DEFAULT_QUEUE, DEFAULT_RETRY_DELAY_SECS,
    QUEUE_TABLE,
};
pub use schedule::{beat_schedule, BeatEntry, Cadence, Crontab, ScheduleError};
pub use scheduler::{default_schedule, BEAT_LOCK_KEY, BEAT_TICK, FAILED_RETENTION_SECS};
pub use task_message::TaskMessage;
pub use worker::{DispatchOutcome, Handler, HandlerError, Registry, Route, Verdict, WorkerConfig};

/// Every failure the jobs plane reports.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("bad schedule: {0}")]
    Schedule(#[from] ScheduleError),
}
