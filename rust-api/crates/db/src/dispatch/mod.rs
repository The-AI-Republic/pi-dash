#![forbid(unsafe_code)]

//! Dispatch model layer: cloud_agent + managed_runner L2 read shapes (D-11, stage 5).
//!
//! Ports the dispatch-touched read surface of
//! `apps/api/pi_dash/runner/models.py` (reads only — the tables are
//! owned by D-13…D-15; this sub-issue ports no writes):
//!
//! * [`status`] — `AgentRunStatus` (`models.py:207-235`) +
//!   `AgentRunTrigger` (`models.py:237-253`), values verbatim.
//! * [`agent_run`] — `AgentRun` (`models.py:872-1035`) read shape:
//!   exactly the 22 columns dispatch touches, with defaults.
//!
//! (P3 step B deleted `tool_call` with the retired run tools; run
//! rows and their tool calls now serialize from `runner_runs`.)
//!
//! Sibling L3+ issues add their own files under their own modules;
//! `executor_kind` reuses the L1
//! [`pidash_types::dispatch::AgentExecutorKind`] port (PIDASHCONV-482).
//!
//! Fixture id replayed by the unit tests alongside each module:
//! `rust-api/fixtures/dispatch/fx-disp-02-models.golden.json` (FX-DISP-02).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod agent_run;
pub mod status;

pub use agent_run::AgentRun;
pub use status::{AgentRunStatus, AgentRunTrigger};
