pub mod client;
pub mod protocol;
pub mod server;

// The Windows named-pipe derivation moved to `pidash-ipc` alongside the
// transport client; the IPC *server* (still in the runner) needs the
// identical derivation, so re-export it under its original path.
#[cfg(windows)]
pub(crate) use pidash_ipc::windows_pipe_name;
