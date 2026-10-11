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

pub mod agent;
pub mod github;
pub mod invoke;
pub mod keys;
pub mod provider;
pub mod schemas;
pub mod tools_exec;
pub mod usage;
pub mod validation;
