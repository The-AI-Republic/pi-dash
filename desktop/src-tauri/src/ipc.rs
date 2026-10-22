// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Desktop → managed-daemon IPC.
//!
//! The desktop app starts and supervises the bundled `pidash` daemon
//! (`managed_runner`), but until now it had no live connection to it. This
//! module is that connection: a thin wrapper over the shared
//! [`pidash_ipc::Client`] that talks to the managed daemon over its control
//! socket (a Unix socket on macOS/Linux, a named pipe on Windows, both handled
//! by `pidash_ipc::Client`).
//!
//! The daemon runs **per workspace**, so there is one socket per workspace
//! data dir. This side chooses where it lives ([`daemon_socket_path`]) and
//! hands that path to every `pidash` it spawns in `PIDASH_IPC_SOCKET`; the
//! daemon binds exactly there. Resolve it with [`ManagedPaths::daemon_socket`]
//! (via `ManagedPaths::for_workspace`) — never from `paths.runtime_dir`, the
//! shared `managed/runtime/` that only holds the model token.
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

/// The control socket of a daemon whose runtime dir is `runtime_dir`, when no
/// shorter place is available: `<runtime_dir>/pidash.sock`, the path the
/// daemon defaults to.
pub fn socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("pidash.sock")
}

/// The control socket for the daemon whose data lives in `data_dir`.
///
/// Not under `data_dir`: that tree is
/// `<app data>/managed/pidash/<workspace>/data`, and on macOS its prefix alone
/// is about as long as a Unix socket path may be (104 bytes), so a socket
/// bound there fails for nearly every user name. It goes in a short per-user
/// directory instead, named by a hash of `data_dir` so each workspace, install
/// and edition still has its own.
pub fn daemon_socket_path(data_dir: &Path) -> PathBuf {
    socket_in(short_socket_dir().as_deref(), data_dir)
}

fn socket_in(short_dir: Option<&Path>, data_dir: &Path) -> PathBuf {
    match short_dir {
        Some(dir) => pidash_ipc::short_socket_path(dir, data_dir),
        None => socket_path(&data_dir.join("runtime")),
    }
}

/// A short directory only this user can reach, or `None` to keep the socket
/// under the data dir.
///
/// macOS: the per-user temp dir (`/var/folders/…/T/`, inside the container for
/// a sandboxed build), whose length does not depend on the user name.
#[cfg(target_os = "macos")]
fn short_socket_dir() -> Option<PathBuf> {
    Some(std::env::temp_dir())
}

/// Linux: the per-user runtime dir (`/run/user/<uid>`). Without one there is
/// no short location that is already private to the user — `/tmp` is shared —
/// so the socket stays under the data dir.
#[cfg(all(unix, not(target_os = "macos")))]
fn short_socket_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    (dir.is_absolute() && dir.is_dir()).then_some(dir)
}

/// Windows: the path is only hashed into a named-pipe name, so its length
/// does not matter.
#[cfg(not(unix))]
fn short_socket_dir() -> Option<PathBuf> {
    None
}

/// Fetch a one-shot status snapshot from the managed daemon.
///
/// Returns `Err` when the daemon isn't running (socket absent / refused) or
/// answers with anything other than a `Status` frame — the caller decides
/// whether "no daemon" is an error or just "not signed in yet".
pub async fn managed_status(paths: &ManagedPaths) -> Result<StatusSnapshot, String> {
    status_over(&paths.daemon_socket()).await
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

    /// The managed data dir for `user` in `workspace` on macOS, where the app
    /// data dir is `~/Library/Application Support/<bundle id>`.
    fn macos_data_dir(root: &Path, user: &str, workspace: &str) -> PathBuf {
        root.join("Users")
            .join(user)
            .join("Library/Application Support/com.airepublic.pidash.oss/managed/pidash")
            .join(workspace)
            .join("data")
    }

    /// The longest names the path can be asked to carry: a 32-character login
    /// and a 48-character workspace slug.
    fn longest_data_dir(root: &Path) -> PathBuf {
        macos_data_dir(root, &"u".repeat(32), &"w".repeat(48))
    }

    #[test]
    fn socket_under_the_data_dir_overflows_for_an_ordinary_mac_user() {
        let socket = socket_in(None, &macos_data_dir(Path::new("/"), "alice", "acme"));
        // 104 bytes of `sun_path` on macOS, one of them the terminating NUL.
        assert!(socket.as_os_str().len() > 103, "{socket:?}");
    }

    #[test]
    fn short_socket_fits_macos_limit_for_the_longest_names() {
        let data_dir = longest_data_dir(Path::new("/"));
        for temp_dir in [
            // Direct download.
            "/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/",
            // Mac App Store: the sandbox appends the bundle id.
            "/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/com.airepublic.pidash.oss/",
            "/private/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/com.airepublic.pidash.oss/",
        ] {
            let socket = socket_in(Some(Path::new(temp_dir)), &data_dir);
            let len = socket.as_os_str().len();
            assert!(len <= 103, "{socket:?} is {len} bytes");
        }
    }

    #[test]
    fn short_socket_fits_linux_limit_for_the_longest_names() {
        let socket = socket_in(
            Some(Path::new("/run/user/4294967295")),
            &longest_data_dir(Path::new("/")),
        );
        let len = socket.as_os_str().len();
        assert!(len <= 107, "{socket:?} is {len} bytes");
    }

    #[test]
    fn each_workspace_gets_its_own_socket() {
        let dir = Path::new("/run/user/1000");
        let acme = socket_in(Some(dir), &macos_data_dir(Path::new("/"), "alice", "acme"));
        let other = socket_in(Some(dir), &macos_data_dir(Path::new("/"), "alice", "other"));
        assert_ne!(acme, other);
        assert_eq!(
            acme,
            socket_in(Some(dir), &macos_data_dir(Path::new("/"), "alice", "acme"))
        );
    }

    /// The real thing, on whatever OS runs the tests: under a data dir as deep
    /// as the desktop's, the socket the daemon would default to cannot be bound
    /// at all, and the short one can.
    #[test]
    fn os_binds_the_short_socket_where_the_data_dir_one_fails() {
        let root = tempfile::tempdir().unwrap();
        let short_dir = tempfile::tempdir_in("/tmp").unwrap();
        let data_dir = longest_data_dir(root.path());
        std::fs::create_dir_all(data_dir.join("runtime")).unwrap();

        let under_data_dir = socket_in(None, &data_dir);
        pidash_ipc::check_socket_path(&under_data_dir).expect_err("too long to bind");
        std::os::unix::net::UnixListener::bind(&under_data_dir).expect_err("the OS agrees");

        let short = socket_in(Some(short_dir.path()), &data_dir);
        pidash_ipc::check_socket_path(&short).expect("short enough to bind");
        std::os::unix::net::UnixListener::bind(&short).expect("the OS agrees");
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
