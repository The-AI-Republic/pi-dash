//! IPC wire protocol.
//!
//! The definitions moved to the shared `pidash-ipc` crate (PDASHOSS01-158)
//! so the desktop app and the direct-chat client can speak the daemon's IPC
//! without depending on the whole runner. They are re-exported here so every
//! `crate::ipc::protocol::*` call site inside the runner is unchanged.
pub use pidash_ipc::protocol::*;
