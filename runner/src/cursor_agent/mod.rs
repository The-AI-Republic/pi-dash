//! Cursor Agent integration. Drives `cursor-agent --print --output-format
//! stream-json` and translates emitted stream-json events into the
//! agent-agnostic [`crate::agent::BridgeEvent`] shape used by the daemon.
//!
//! MVP limitations (tracked as follow-ups):
//! - Approvals bypass: runs with `--force` (allow commands unless explicitly
//!   denied). Wiring a real approval prompt is out of scope for the first pass,
//!   mirroring the Claude Code bridge's `bypassPermissions` posture.
//! - One-shot per turn: cursor-agent print mode takes the prompt as a
//!   positional argument and exits after emitting `result`, so each turn spawns
//!   a fresh subprocess (reusing the prior `--resume` chat id for continuity)
//!   rather than feeding turns over stdin.

pub mod bridge;
pub mod process;
pub mod schema;
