//! Live model runtime for the cloud-agent execute plane
//! (PIDASHCONV-866; moved from the binary to this library module in
//! PIDASHCONV-918 so both binaries compose the same runtime).
//!
//! Ports pydantic-ai 1.107.0 `Agent.run` as constructed by
//! `cloud_agent/runtime.py:18-76` (`execute`): the Anthropic +
//! OpenAI-compatible provider lanes, the agent loop (retries, usage
//! limits, output validation), and the 15 tool executors with the
//! in-process GitHub leg. [`crate::oss_worker::execute_seams::LiveExecuteSeams`]
//! implements `ExecuteSeams::invoke_model` on top of this module.
//!
//! The behavioral contract is the `fx-disp-08-model-runtime` golden
//! (schemas, usage-extractor table, validation battery) plus the stub
//! probes recorded in the issue workpad; every quirk below cites its
//! probe. Layout:
//!
//! - [`schemas`]: the embedded golden (tool schemas, provider table).
//! - [`validation`]: lax argument/output validation + retry rendering.
//! - [`usage`]: the usage accumulator, limit checks, extraction.
//!
//! Later runs add the provider HTTP lanes, the agent loop, and the tool
//! executors behind the same `invoke` entry point.
//!
//! P2 (agent naming convention): the chat-engine files (`provider`,
//! `usage`, `schemas`, `validation`, `keys`, `github`) moved to
//! [`crate::cloud_agent`] and [`crate::integrations::github_app`].
//! The `pub use` shims below keep this retired execute-plane tree
//! compiling until P3 deletes the whole directory; nothing new must
//! import through them.

pub mod agent;
pub mod invoke;
pub mod tools_exec;

pub use crate::cloud_agent::keys;
pub use crate::cloud_agent::provider;
pub use crate::cloud_agent::usage;
pub use crate::cloud_agent::usage_table as schemas;
pub use crate::cloud_agent::validation;
pub use crate::integrations::github_app as github;
