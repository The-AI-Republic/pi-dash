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
//! Resume support is wired: when `RunPayload.resume_thread_id` is set,
//! the runner spawns `claude --resume <session_id>` so Claude reattaches
//! to its prior on-disk session. The yield path uses the existing
//! `pi-dash-done` fenced-block channel with `status: "paused"` (cloud
//! parses it via `done_signal.ingest_into_run`).

pub mod bridge;
pub mod process;
pub mod schema;
