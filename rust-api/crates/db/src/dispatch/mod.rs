#![forbid(unsafe_code)]

//! Dispatch model layer: cloud_agent + managed_runner L2 read shapes (D-11, stage 5).
//!
//! Ports the dispatch-touched read surface of
//! `apps/api/pi_dash/runner/models.py` (reads only — the tables are
//! owned by D-13…D-15; this sub-issue ports no writes):
//!
//! * [`status`] — `AgentRunStatus` (`models.py:207-235`) +
//!   `AgentRunTrigger` (`models.py:237-250`), values verbatim.
//! * [`agent_run`] — `AgentRun` (`models.py:872-1035`) read shape:
//!   exactly the 22 columns dispatch touches, with defaults.
//! * [`event`] — `AgentRunEvent` (`models.py:1162-1175`) read shape.
//! * [`tool_call`] — `ToolCallStatus` (`models.py:1178-1184`) +
//!   `AgentRunToolCall` (`models.py:1187-1216`) read shape (15
//!   dispatch-touched columns).
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
pub mod event;
pub mod status;
pub mod tool_call;

pub use agent_run::AgentRun;
pub use event::AgentRunEvent;
pub use status::{AgentRunStatus, AgentRunTrigger};
pub use tool_call::{AgentRunToolCall, ToolCallStatus};
