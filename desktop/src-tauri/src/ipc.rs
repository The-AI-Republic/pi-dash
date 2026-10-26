// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Desktop → daemon IPC.
//!
//! A thin wrapper over the shared [`pidash_ipc::Client`] that talks to the
//! shared `pidash` daemon (see `shared_daemon`) over its control socket — a
//! Unix socket on macOS/Linux, a named pipe on Windows, both handled by
//! `pidash_ipc::Client`.
//!
//! Which socket: the daemon's default one, as the installed CLI resolves it
//! (`shared_daemon::socket`). The app no longer runs private per-workspace
//! daemons with re-rooted data dirs, so there is exactly one.

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use pidash_ipc::protocol::{Request, Response, StatusSnapshot};

/// A daemon's control socket inside its runtime dir, for tests that bind a
/// stub daemon. Kept byte-identical to the daemon's `Paths::ipc_socket_path`;
/// the app itself gets the shared daemon's socket from the CLI.
#[cfg(test)]
pub fn socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("pidash.sock")
}

/// Connect to the daemon at `socket` and complete a single `StatusGet`
/// round-trip.
///
/// Returns `Err` when no daemon is running there (socket absent / refused) or
/// it answers with anything other than a `Status` frame — the caller decides
/// whether "no daemon" is an error or just "not started yet".
pub async fn daemon_status(socket: &Path) -> Result<StatusSnapshot, String> {
    let mut client = pidash_ipc::Client::connect(socket)
        .await
        .map_err(|e| format!("connecting to the pidash daemon: {e}"))?;
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

        let snapshot = daemon_status(&socket).await.expect("StatusGet round-trip");
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
        assert!(daemon_status(&socket).await.is_err());
    }
}
