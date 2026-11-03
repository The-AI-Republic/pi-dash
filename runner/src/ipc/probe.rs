//! Daemon reachability probes that keep their diagnostics.
//!
//! `pidash status` and the restart verifier used to collapse every IPC
//! failure into "not reachable", which made a CLI looking at the wrong
//! socket indistinguishable from a stopped daemon (PDASHOSS01-311). These
//! helpers return the attempted socket and the underlying error, and can
//! detect a daemon still listening where pre-PDASHOSS01-230 binaries put it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::ipc::client::Client;
use crate::ipc::protocol::{Request, Response, StatusSnapshot};
use crate::util::paths::Paths;

/// Ask the daemon listening at `socket` for its status. Every error names
/// the socket, so callers can print it as-is.
pub async fn status_at(socket: &Path) -> Result<StatusSnapshot> {
    let mut client = Client::connect(socket).await?;
    let response = client
        .call(Request::StatusGet)
        .await
        .with_context(|| format!("StatusGet over IPC at {socket:?}"))?;
    match response {
        Response::Status(s) => Ok(s),
        // A running daemon that explicitly reports an error is very
        // different from "socket unreachable" — keep the two apart.
        Response::Error(e) => bail!("daemon at {socket:?} reported error: {}", e.message),
        other => bail!("unexpected IPC response from {socket:?}: {other:?}"),
    }
}

/// A daemon answering on the legacy `<data_dir>/runtime` socket instead of
/// this CLI's socket: one started by a pidash that predates
/// PDASHOSS01-230 from a service unit exporting the default
/// `PIDASH_DATA_DIR`. A `pidash restart` with the current binary moves it.
pub async fn legacy_daemon(paths: &Paths) -> Option<(PathBuf, StatusSnapshot)> {
    let legacy = paths.legacy_ipc_socket_path()?;
    let snapshot = status_at(&legacy).await.ok()?;
    Some((legacy, snapshot))
}

/// Human-readable explanation of why no daemon answered at `socket`.
pub fn unreachable_detail(
    socket: &Path,
    error: &anyhow::Error,
    legacy_socket: Option<&Path>,
) -> String {
    let mut out = format!("IPC socket: {}\nIPC error: {error:#}", socket.display());
    if let Some(legacy) = legacy_socket {
        out.push_str(&format!(
            "\nA daemon is answering at {} instead: it runs a pidash binary older \
             than this CLI (one that isolated the socket when the service unit exports \
             PIDASH_DATA_DIR). Restart the service on the current binary \
             (`pidash restart`) to move it to the shared socket.",
            legacy.display()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_detail_names_socket_and_error() {
        let err = anyhow::anyhow!("No such file or directory (os error 2)")
            .context("connecting to runner IPC at \"/run/user/1000/pidash/pidash.sock\"");
        let detail = unreachable_detail(Path::new("/run/user/1000/pidash/pidash.sock"), &err, None);
        assert!(detail.contains("IPC socket: /run/user/1000/pidash/pidash.sock"));
        // `{:#}` keeps the root cause, which is what tells "no socket" apart
        // from "permission denied" or "connection refused".
        assert!(detail.contains("No such file or directory"));
        assert!(!detail.contains("older pidash"));
    }

    #[test]
    fn unreachable_detail_points_at_a_legacy_daemon() {
        let err = anyhow::anyhow!("connection refused");
        let legacy = Path::new("/home/u/.local/share/pidash/runtime/pidash.sock");
        let detail = unreachable_detail(
            Path::new("/run/user/1000/pidash/pidash.sock"),
            &err,
            Some(legacy),
        );
        assert!(detail.contains(&legacy.display().to_string()));
        assert!(detail.contains("pidash restart"));
    }

    #[tokio::test]
    async fn status_at_a_missing_socket_reports_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let socket = tmp.path().join("pidash.sock");
        let err = status_at(&socket).await.unwrap_err();
        assert!(format!("{err:#}").contains(&socket.display().to_string()));
    }
}
