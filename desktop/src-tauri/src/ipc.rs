// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Desktop → managed-daemon IPC.
//!
//! The desktop app starts and supervises the bundled `pidash` daemon
//! (`managed_runner`), but until now it had no live connection to it. This
//! module is that connection: a thin wrapper over the shared
//! [`pidash_ipc::Client`] that talks to the managed daemon over its control
//! socket (`<runtime_dir>/pidash.sock` — a Unix socket on macOS/Linux, a
//! named pipe on Windows, both handled by `pidash_ipc::Client`).
//!
//! The socket path matches the daemon's own `RunnerPaths::ipc_socket_path`
//! (`<runtime_dir>/pidash.sock`) — the managed tree's `runtime/` dir is the
//! same one the daemon binds under, so no extra path plumbing is needed.
//!
//! Scope note (PDASHOSS01-158): this lands the *capability* — the desktop
//! crate depends on `pidash-ipc` (not the runner) and can round-trip
//! `StatusGet`. The live status / approvals surface is deliberately out of
//! scope; the user sees agent activity through the chat UI (PDASHOSS01-159),
//! which is the real consumer of this helper.
#![allow(dead_code)] // Consumed by PDASHOSS01-159 (direct local chat); no caller in-tree yet.

use std::path::{Path, PathBuf};

use pidash_ipc::protocol::{Request, Response, StatusSnapshot};

use crate::managed_runner::ManagedPaths;

/// The managed daemon's control socket, derived from the managed runtime dir.
///
/// Kept byte-identical to the daemon's `RunnerPaths::ipc_socket_path` so both
/// ends agree without a shared constant crossing the crate boundary.
pub fn socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("pidash.sock")
}

/// Fetch a one-shot status snapshot from the managed daemon.
///
/// Returns `Err` when the daemon isn't running (socket absent / refused) or
/// answers with anything other than a `Status` frame — the caller decides
/// whether "no daemon" is an error or just "not signed in yet".
pub async fn managed_status(paths: &ManagedPaths) -> Result<StatusSnapshot, String> {
    status_over(&socket_path(&paths.runtime_dir)).await
}

/// Connect to a daemon at `socket` and complete a single `StatusGet`
/// round-trip. Split out from [`managed_status`] so it can be driven against
/// a stub socket in tests.
async fn status_over(socket: &Path) -> Result<StatusSnapshot, String> {
    let mut client = pidash_ipc::Client::connect(socket)
        .await
        .map_err(|e| format!("connecting to managed daemon: {e}"))?;
    match client
        .call(Request::StatusGet)
        .await
        .map_err(|e| format!("StatusGet failed: {e}"))?
    {
        Response::Status(snapshot) => Ok(snapshot),
        Response::Error(err) => Err(format!("daemon error {}: {}", err.code, err.message)),
        other => Err(format!("unexpected response to StatusGet: {other:?}")),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use pidash_ipc::protocol::{DaemonInfo, StatusSnapshot};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    fn sample_snapshot() -> StatusSnapshot {
        StatusSnapshot {
            daemon: DaemonInfo {
                cloud_url: "https://example.test".into(),
                connected: true,
                uptime_secs: 7,
                update: None,
            },
            runners: vec![],
        }
    }

    /// The desktop crate, depending only on `pidash-ipc`, can connect to a
    /// daemon over the real transport and complete a `StatusGet` round-trip.
    /// The stub speaks the exact newline-framed JSON the daemon's IPC server
    /// does, so this exercises the shared protocol + client end to end.
    #[tokio::test]
    async fn status_get_round_trips_over_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket = socket_path(dir.path());

        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            // Read one request line.
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let req: Request = serde_json::from_str(line.trim()).unwrap();
            assert!(
                matches!(req, Request::StatusGet),
                "expected StatusGet, got {req:?}"
            );
            // Answer with a Status frame, newline-framed.
            let resp = Response::Status(sample_snapshot());
            let mut bytes = serde_json::to_vec(&resp).unwrap();
            bytes.push(b'\n');
            reader.get_mut().write_all(&bytes).await.unwrap();
            reader.get_mut().flush().await.unwrap();
        });

        let snapshot = status_over(&socket).await.expect("StatusGet round-trip");
        assert_eq!(snapshot.daemon.cloud_url, "https://example.test");
        assert!(snapshot.daemon.connected);
        assert_eq!(snapshot.daemon.uptime_secs, 7);

        server.await.unwrap();
    }

    /// A missing socket surfaces as an error, not a panic — the "daemon not
    /// running yet" path the caller must handle.
    #[tokio::test]
    async fn status_over_missing_socket_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let socket = socket_path(dir.path()); // never bound
        assert!(status_over(&socket).await.is_err());
    }
}
