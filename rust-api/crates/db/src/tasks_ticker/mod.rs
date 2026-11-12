//! Agent ticker + scheduler domain surface (D-10, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/issue_agent_ticker.py` and
//! `apps/api/pi_dash/db/models/scheduler.py` for the db layer:
//!
//! * [`models`] — `IssueAgentTicker` (+ budget/interval/jitter helpers),
//!   `TickerDisarmReason`, `Scheduler`, `SchedulerBinding`,
//!   `SchedulerSource`, `OutcomeMode`, `OUTCOME_MODE_DIRECTIVES`
//!   (PIDASHCONV-206). Scanners and fire paths belong to PIDASHCONV-207;
//!   the rrule engine belongs to its own layer issue.

pub mod models;

pub use models::{
    issue_agent_ticker::IssueAgentTicker, jitter_seconds, outcome_mode_directive,
    pool_size_or_default, resolve_project_interval, scheduler::Scheduler,
    scheduler_binding::SchedulerBinding, OnDelete, OutcomeMode, SchedulerSource,
    TickerDisarmReason, DEFAULT_INTERVAL_SECONDS, DEFAULT_MAX_TICKS, INFINITE_MAX_TICKS,
    JITTER_FRACTION, LAST_ERROR_MAX_LEN, OUTCOME_MODE_DIRECTIVES,
};
