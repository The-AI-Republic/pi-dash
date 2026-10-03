//! Runner-runs Celery tasks (D-15, stage 5).
//!
//! Ports `apps/api/pi_dash/runner/tasks.py` (PIDASHCONV-539/540, FX-RUN-07):
//!
//! * [`sweeps_runs`] — approval expiry + runner/session sweeps
//!   (`tasks.py:42-202`, L6a, PIDASHCONV-539).
//! * [`sweeps_chat`] — the dedupe sweeps, the chat sweeps, the stall
//!   watchdog and the terminal-effects pair (`tasks.py:205-353`, L6b,
//!   PIDASHCONV-540).
//!
//! Beat entries stay Django-owned (F-09 transcription already covers the
//! schedule); this layer ports task bodies only. Like the mail tasks,
//! registration does not flip the live worker — the domain gate flips
//! ownership after the proxy pass, so until then every name here still
//! routes to `PythonOwned` (see [`crate::worker::route_for`]).

pub mod sweeps_chat;
pub mod sweeps_runs;

pub use sweeps_chat::{
    register_runner_runs_tasks, RECONCILE_STALLED_RUNS_TASK, RECONCILE_TERMINAL_EFFECTS_TASK,
    SWEEP_AGENT_CHAT_STATE_TASK, SWEEP_CHAT_MESSAGE_DEDUPE_TASK, SWEEP_RUN_MESSAGE_DEDUPE_TASK,
    TERMINAL_EFFECTS_TASK,
};
pub use sweeps_runs::{
    register_sweeps_runs_tasks, EXPIRE_TASK, MARK_OFFLINE_TASK, SWEEP_IDLE_TASK, SWEEP_STALE_TASK,
    SWEEP_STREAMS_TASK,
};

/// Every Celery task name this domain owns (L6a + L6b).
pub const TASK_NAMES: [&str; 11] = [
    EXPIRE_TASK,
    MARK_OFFLINE_TASK,
    SWEEP_IDLE_TASK,
    SWEEP_STALE_TASK,
    SWEEP_STREAMS_TASK,
    SWEEP_RUN_MESSAGE_DEDUPE_TASK,
    SWEEP_CHAT_MESSAGE_DEDUPE_TASK,
    SWEEP_AGENT_CHAT_STATE_TASK,
    RECONCILE_STALLED_RUNS_TASK,
    TERMINAL_EFFECTS_TASK,
    RECONCILE_TERMINAL_EFFECTS_TASK,
];

/// True for the task names in [`TASK_NAMES`]. The worker forwards them to
/// the Python plane until the domain gate flips ownership.
pub fn is_runner_runs_task(task: &str) -> bool {
    TASK_NAMES.contains(&task)
}
