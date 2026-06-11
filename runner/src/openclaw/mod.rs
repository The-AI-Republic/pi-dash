//! OpenClaw integration. Drives the OpenClaw coding agent over the Agent
//! Client Protocol (ACP) via the headless ACP client `acpx`
//! (`acpx --format json openclaw exec`) and translates the emitted ACP
//! JSON-RPC NDJSON into the agent-agnostic [`crate::agent::BridgeEvent`] shape
//! used by the daemon.
//!
//! OpenClaw's "app-server mode" equivalent is ACP, not a Codex-style
//! `app-server` subcommand. `acpx` performs the ACP handshake with OpenClaw
//! and dumps the raw protocol traffic, which keeps this bridge structurally
//! identical to the one-shot cursor-agent bridge.
//!
//! MVP limitations (tracked as follow-ups), see [`bridge`]:
//! - Approvals bypass: runs with `--approve-all`. Real per-tool approval
//!   requires driving `openclaw acp` natively against a running OpenClaw
//!   Gateway (so `session/request_permission` can be answered from
//!   `runner/src/approval`) instead of shelling out to `acpx`.
//! - One-shot per turn: `acpx ... exec` takes the prompt as an argv positional
//!   and runs the turn to completion, so each turn spawns a fresh subprocess.

pub mod bridge;
pub mod process;
pub mod schema;
