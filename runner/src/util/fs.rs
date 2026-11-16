use std::path::Path;

/// Create `path` and any missing parents with owner-only permissions
/// (0700 on Unix, mirroring `config::file`'s 0600 file policy).
///
/// Succeeds whether or not `path` already exists, and never changes an
/// existing directory's mode: a user-chosen working dir the operator
/// already made keeps whatever mode it has. Only directories the daemon
/// itself creates are affected. Windows relies on profile ACLs, so there
/// this is a plain `create_dir_all`.
pub fn create_dir_all_private(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(path)
    }
}
