//! Claude Code agent integration. Drives `claude --print --output-format
//! stream-json` as a one-turn subprocess per run and translates the emitted
//! stream-json events into the agent-agnostic [`crate::agent::BridgeEvent`]
//! shape used by the daemon.
//!
//! MVP limitations (tracked as follow-ups):
//! - Approvals bypass: runs with `--permission-mode bypassPermissions`.
//!   Wiring Claude's `--permission-prompt-tool` into the approval router
//!   requires a small MCP server bridge; out of scope for the first pass.
//!
//! Every run starts a fresh Claude session — the runner does not pass
//! `--resume`. State carries between runs via the issue's workpad comment,
//! the comment thread, and the repo. See
//! `.ai_design/ticking_optimization/design.md`.

pub mod bridge;
pub mod process;
pub mod schema;
