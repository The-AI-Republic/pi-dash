//! Runner-runs Celery tasks (D-15, stage 5).
//!
//! Ports `apps/api/pi_dash/runner/tasks.py` (PIDASHCONV-539/540, FX-RUN-07):
//!
//! * [`sweeps_runs`] — approval expiry + runner/session sweeps
//!   (`tasks.py:42-202`, L6a, PIDASHCONV-539).
//!
//! L6b (PIDASHCONV-540) adds the `sweeps_chat` module beside it and extends
//! [`TASK_NAMES`]; the merge keeps both sides. Like the mail tasks,
//! registration does not flip the live worker — the domain gate flips
//! ownership after the proxy pass, so until then every name here still
//! routes to `PythonOwned` (see [`crate::worker::route_for`]).

pub mod sweeps_runs;

pub use sweeps_runs::{
    register_sweeps_runs_tasks, EXPIRE_TASK, MARK_OFFLINE_TASK, SWEEP_IDLE_TASK, SWEEP_STALE_TASK,
    SWEEP_STREAMS_TASK,
};

/// Every Celery task name this domain owns so far (L6a; L6b extends the
/// list when `sweeps_chat` lands).
pub const TASK_NAMES: [&str; 5] = [
    EXPIRE_TASK,
    MARK_OFFLINE_TASK,
    SWEEP_IDLE_TASK,
    SWEEP_STALE_TASK,
    SWEEP_STREAMS_TASK,
];

/// True for the task names in [`TASK_NAMES`]. The worker forwards them to
/// the Python plane until the domain gate flips ownership.
pub fn is_runner_runs_task(task: &str) -> bool {
    TASK_NAMES.contains(&task)
}
