//! Shared Agent Client Protocol (ACP) driver. The runner acts as the ACP
//! *client* for agents that ship their own native ACP server on stdio
//! (Grok's `grok agent stdio`, Antigravity's `agy acp`): it performs the
//! `initialize` → `session/new` → `session/prompt` handshake over JSON-RPC,
//! auto-answers `session/request_permission`, and translates the streamed
//! `session/update` notifications plus the terminal `stopReason` into the
//! agent-agnostic [`crate::agent::BridgeEvent`] shape the daemon consumes.
//!
//! ACP is JSON-RPC 2.0 (<https://agentclientprotocol.com>), currently at
//! major version 1. Inbound frames are parsed with OpenClaw's shared
//! [`crate::openclaw::schema::AcpMessage`]; outbound requests are built in
//! [`schema`]. The persistent stdin-driven process wrapper is [`process`],
//! modelled on `codex::app_server::AppServer`.
//!
//! Per-agent differences live in [`AcpProfile`]: which argv to spawn, whether
//! the configured model rides on `session/new` (Grok's extra field) or is
//! applied through the session config options the server advertises
//! (Antigravity's Model picker, set via `session/set_config_option`), and
//! whether `interrupt` ends the turn through the protocol-level
//! `session/cancel` notification (Antigravity) or an OS signal (Grok,
//! unchanged from its pre-shared behaviour).
//!
//! [`bridge`] documents the remaining per-agent behaviour notes (approvals
//! bypass posture, resume granularity).

pub mod bridge;
pub mod process;
pub mod schema;

pub use bridge::{AcpProfile, Bridge, BridgeCursor, HandshakeAuthError};
pub use process::AcpProcess;
