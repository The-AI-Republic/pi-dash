//! Shared desktop ↔ daemon IPC for Pi Dash.
//!
//! This crate holds the wire protocol (`protocol`), the cross-platform
//! transport client (`client`, Unix socket on macOS/Linux, named pipe on
//! Windows), and the leaf DTOs (`dto`) that the `Response` enum reaches
//! into. It was extracted from the runner so the desktop app — and the
//! direct-chat client in PDASHOSS01-159 — can talk to a running daemon
//! without depending on the whole runner crate (which pulls in a
//! GUI-hostile dependency tree and is deliberately excluded from the
//! runner's Cargo workspace via `exclude = ["desktop"]`).
//!
//! The runner re-exports every type from its original module path, so no
//! runner-internal call site changed. There is **no wire-format change**:
//! `IPC_VERSION` stays at 3. The first real protocol change will come from
//! PDASHOSS01-159's local-chat message types.

pub mod client;
pub mod dto;
pub mod protocol;

// Convenience flat re-exports so downstream clients can `use pidash_ipc::{...}`
// without threading through the submodule layout.
pub use client::Client;
pub use dto::*;
pub use protocol::*;

/// Windows named-pipe name derived deterministically from the socket path.
///
/// macOS/Linux bind a Unix socket at `<runtime_dir>/pidash.sock`; Windows
/// has no filesystem sockets, so the same path is hashed into a stable pipe
/// name both ends agree on. Kept here (rather than in `client`) because the
/// runner's IPC *server* needs the identical derivation.
#[cfg(windows)]
pub fn windows_pipe_name(path: &std::path::Path) -> String {
    let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, path.to_string_lossy().as_bytes());
    format!(r"\\.\pipe\pidash-{id}")
}
