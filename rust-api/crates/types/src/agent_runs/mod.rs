//! Dispatch pure layer: agent-run executor + Pi Dash Agent L1 types
//! (D-11, stage 5; trimmed by P3 step A, which deleted the retired
//! run's `error` and `output` modules).
//!
//! Ports the exception-free bottom of the dispatch domain:
//!
//! * [`executor`] — `core/agent_execution.py:7-55` (`AgentExecutorKind`,
//!   `MACHINE_EXECUTORS`, `DEPRECATED_EXECUTORS`, `normalize_executor_write`,
//!   `get_default_agent_executor`).
//! * [`managed_runner`] — `managed_runner/errors.py:15-45`
//!   (`PidashAgentReason`, `PidashAgentUnavailable`).
//!
//! Fixture id replayed by the unit tests alongside each module:
//! `rust-api/fixtures/dispatch/fx-disp-01-types.golden.json` (FX-DISP-01).

#[allow(clippy::module_inception)]
pub mod executor;
pub mod pidash_agent;
pub use executor::{
    get_default_agent_executor, normalize_executor_write, AgentExecutorKind, DEPRECATED_EXECUTORS,
    MACHINE_EXECUTORS,
};
pub use pidash_agent::{PidashAgentReason, PidashAgentUnavailable};
