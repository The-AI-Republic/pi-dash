//! Where the daemon's control socket lives, and whether it can live there.
//!
//! A Unix socket's path has to fit in `sockaddr_un.sun_path`: 104 bytes on
//! macOS and the BSDs, 108 on Linux, including the terminating NUL. A socket
//! bound under a daemon's data directory inherits that directory's whole
//! prefix, and the desktop app's is already about that long on its own
//! (`~/Library/Application Support/<bundle id>/managed/pidash/<workspace>/data`),
//! so the bind fails for nearly every macOS user. A host app that roots the
//! daemon somewhere deep therefore picks a short socket path itself
//! ([`short_socket_path`]) and hands it to the daemon in [`SOCKET_ENV`], so
//! both ends use one path and it no longer depends on how deep the data
//! layout is.

use std::path::{Path, PathBuf};

/// Environment variable carrying the control socket path a host app chose for
/// the daemon it starts. Every `pidash` invocation honours it, so the daemon,
/// the host, and the `pidash` CLI calls the agent makes all resolve the same
/// socket.
pub const SOCKET_ENV: &str = "PIDASH_IPC_SOCKET";

/// Longest socket path, in bytes, that fits in `sockaddr_un.sun_path` with its
/// terminating NUL.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub const MAX_SOCKET_PATH_BYTES: usize = 107;
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
pub const MAX_SOCKET_PATH_BYTES: usize = 103;

/// Reject a socket path the OS cannot bind or connect to.
///
/// Checked before binding so the failure names the cause and the limit,
/// instead of surfacing as the kernel's bare "path must be shorter than
/// SUN_LEN". Always `Ok` on Windows, where the path is only hashed into a
/// named-pipe name.
pub fn check_socket_path(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let len = path.as_os_str().as_bytes().len();
        if len > MAX_SOCKET_PATH_BYTES {
            anyhow::bail!(
                "control socket path {path:?} is {len} bytes; \
                 a Unix socket path is limited to {MAX_SOCKET_PATH_BYTES} on this platform"
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// A short socket path under `dir` for the daemon whose data lives in
/// `data_dir`.
///
/// The file name is a hash of `data_dir`, so every workspace, install and
/// edition keeps its own socket — the separation the per-data-dir runtime
/// directory existed to provide — while the length stays fixed at
/// `dir` + 18 bytes whatever the user name or workspace slug. Kept that
/// small on purpose: a sandboxed macOS app's temp dir is itself over 80 bytes
/// of the 103 available.
pub fn short_socket_path(dir: &Path, data_dir: &Path) -> PathBuf {
    let id = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        data_dir.to_string_lossy().as_bytes(),
    );
    let id = id.simple().to_string();
    dir.join(format!("pidash-{}", &id[..10]))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A path of exactly `len` bytes under `dir`.
    fn path_of_len(dir: &Path, len: usize) -> PathBuf {
        let prefix = dir.as_os_str().len() + 1;
        dir.join("s".repeat(len - prefix))
    }

    #[test]
    fn limit_matches_what_the_os_will_bind() {
        // Under /tmp rather than the default temp dir: macOS's own is already
        // half the limit.
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let fits = path_of_len(dir.path(), MAX_SOCKET_PATH_BYTES);
        let too_long = path_of_len(dir.path(), MAX_SOCKET_PATH_BYTES + 1);

        check_socket_path(&fits).expect("a path at the limit is accepted");
        std::os::unix::net::UnixListener::bind(&fits).expect("and the OS binds it");

        let err = check_socket_path(&too_long).expect_err("one byte more is rejected");
        assert!(err.to_string().contains("limited to"), "{err}");
        std::os::unix::net::UnixListener::bind(&too_long)
            .expect_err("and the OS refuses to bind it");
    }

    #[test]
    fn short_socket_path_is_stable_and_per_data_dir() {
        let dir = Path::new("/run/user/1000");
        let acme = short_socket_path(dir, Path::new("/home/a/managed/pidash/acme/data"));
        let again = short_socket_path(dir, Path::new("/home/a/managed/pidash/acme/data"));
        let other = short_socket_path(dir, Path::new("/home/a/managed/pidash/other/data"));
        assert_eq!(acme, again);
        assert_ne!(acme, other);
        assert_eq!(acme.parent(), Some(dir));
    }

    #[test]
    fn short_socket_path_length_ignores_the_data_dir() {
        let dir = Path::new("/run/user/1000");
        let short = short_socket_path(dir, Path::new("/d"));
        let long = short_socket_path(dir, &Path::new("/Users").join("u".repeat(400)));
        assert_eq!(short.as_os_str().len(), long.as_os_str().len());
        assert_eq!(short.as_os_str().len(), dir.as_os_str().len() + 18);
    }
}
