//! Grok integration. Drives xAI's Grok CLI coding agent over the Agent
//! Client Protocol (ACP) by spawning its native ACP server (`grok agent
//! stdio`) and speaking JSON-RPC to it directly on stdin/stdout, translating
//! the emitted ACP traffic into the agent-agnostic
//! [`crate::agent::BridgeEvent`] shape used by the daemon.
//!
//! Unlike the OpenClaw bridge — which shells out to the third-party `acpx`
//! ACP *client* to drive the `openclaw exec` adapter — Grok is its own ACP
//! server, so this bridge acts as the ACP client itself: it performs the
//! `initialize` → `session/new` → `session/prompt` handshake and reads the
//! streamed `session/update` notifications plus the terminal `stopReason`.
//! This keeps Grok self-contained (it needs only the `grok` binary, not
//! `acpx`, which lists no Grok adapter) while reusing OpenClaw's ACP message
//! schema ([`crate::openclaw::schema::AcpMessage`]) for the load-bearing
//! frame-parse and turn-end detection.
//!
//! Because `grok agent stdio` is a long-lived JSON-RPC server (not a one-shot
//! process like `cursor-agent` / `acpx exec`), the process wrapper is modelled
//! on the persistent, stdin-driven `codex::app_server::AppServer` rather than
//! OpenClaw's argv-only one-shot.
//!
//! MVP limitations (tracked as follow-ups), see [`bridge`]:
//! - **Approvals bypass**: any `session/request_permission` the server sends is
//!   auto-approved, mirroring the cursor / Claude / OpenClaw bridges' bypass
//!   posture. Real per-tool approval (answered from `runner/src/approval`) is
//!   the documented next step.
//! - **Stateless turns**: each run performs a fresh `session/new`; a known
//!   resume session id is reused only as the run's thread-id seed, not to
//!   restore prior conversation state via `session/load`.

pub mod bridge;
pub mod process;
pub mod schema;
