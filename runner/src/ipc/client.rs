//! IPC transport client.
//!
//! Moved to the shared `pidash-ipc` crate (PDASHOSS01-158) and re-exported
//! here so `crate::ipc::client::*` call sites inside the runner are unchanged.
pub use pidash_ipc::client::*;
