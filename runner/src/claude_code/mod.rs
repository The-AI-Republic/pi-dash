//! Claude Code agent integration. Drives `claude --print --input-format
//! stream-json --output-format stream-json` and translates emitted stream-json
//! events into the agent-agnostic [`crate::agent::BridgeEvent`] shape used by
//! the daemon.
//!
//! MVP limitations (tracked as follow-ups):
//! - Approvals bypass: runs with `--permission-mode bypassPermissions`.
//!   Wiring Claude's `--permission-prompt-tool` into the approval router
//!   requires a small MCP server bridge; out of scope for the first pass.
//!
//! Async issue-assignment runs still use one subprocess per run. Direct chat
//! keeps one stream-json subprocess alive for the selected chat session and
//! uses `--resume` when the cloud has a prior Claude session id.

pub mod bridge;
pub mod process;
pub mod schema;
