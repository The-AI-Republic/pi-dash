//! Dispatch pure layer: cloud_agent + managed_runner L1 types (D-11, stage 5).
//!
//! Ports the exception-free bottom of the dispatch domain:
//!
//! * [`error`] — `cloud_agent/errors.py:7-44` (`sanitize_error`,
//!   `_is_usage_limit`, `classify_error`).
//! * [`output`] — `cloud_agent/output.py:1-13` (`CloudAgentOutput`).
//! * [`executor`] — `core/agent_execution.py:7-31` (`AgentExecutorKind`,
//!   `MACHINE_EXECUTORS`, `get_default_agent_executor` only).
//! * [`managed_runner`] — `managed_runner/errors.py:15-45`
//!   (`ManagedRunnerReason`, `ManagedRunnerUnavailable`).
//!
//! Fixture id replayed by the unit tests alongside each module:
//! `rust-api/fixtures/dispatch/fx-disp-01-types.golden.json` (FX-DISP-01).

pub mod error;
pub mod executor;
pub mod managed_runner;
pub mod output;

pub use error::MAX_ERROR_TEXT_CHARS;
pub use error::{classify_error, is_usage_limit, sanitize_error, ErrorCode, UsageLimitExceeded};
pub use executor::{get_default_agent_executor, AgentExecutorKind, MACHINE_EXECUTORS};
pub use managed_runner::{ManagedRunnerReason, ManagedRunnerUnavailable};
pub use output::{CloudAgentOutput, Outcome, OutputValidationError};
pub use output::{
    MAX_EVIDENCE_CHARS, MAX_EVIDENCE_ITEMS, MAX_LIMITATION_CHARS, MAX_LIMITATION_ITEMS,
    MAX_SUMMARY_CHARS,
};
