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
pub mod pipe_id;
pub mod protocol;
#[cfg(windows)]
pub mod windows_pipe;

// Convenience flat re-exports so downstream clients can `use pidash_ipc::{...}`
// without threading through the submodule layout.
pub use client::Client;
pub use dto::*;
pub use protocol::*;

/// Windows named-pipe name from the stored random pipe ID.
///
/// macOS/Linux bind a Unix socket at `<runtime_dir>/pidash.sock`; Windows
/// has no filesystem sockets. The pipe name was previously
/// `UUIDv5(socket path)` — computable by any local user who knows the
/// path, so another account could squat `\\.\pipe\pidash-<id>` and
/// keep the daemon down until logon (PIDESKAPP-236).
///
/// It is now `pidash-<stored-uuid>` where the UUID is a fresh random
/// v4 value persisted in `<runtime_dir>/pipe-id` (owner-only). The
/// path argument's parent is the runtime dir; if it has no parent we
/// fall back to the path itself. A file-system failure degrades to a
/// fresh ephemeral UUID (still unguessable, not the deterministic
/// hash) rather than the predictable one.
#[cfg(windows)]
pub fn windows_pipe_name(path: &std::path::Path) -> String {
    let runtime = path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.to_path_buf());
    match pipe_id::read_or_create_pipe_id(&runtime) {
        Ok(id) => pipe_id::windows_pipe_name_from_id(id),
        Err(_) => format!(r"\\.\\pipe\pidash-{}", uuid::Uuid::new_v4()),
    }
}
