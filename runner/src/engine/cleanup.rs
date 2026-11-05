//! Delete engine versions the daemon no longer needs.
//!
//! `pidash update` leaves the previous engine version directory in place
//! (a daemon that resolved it at start may still be running it). Once the
//! daemon runs a newer engine and no run uses the older directories, they
//! can go. Not wired into the supervisor yet: the caller decides when no run
//! uses a directory and passes that in as `in_use`.

use std::io;
use std::path::{Path, PathBuf};

/// Remove every `<root>/<version>/` whose version is older than `running`
/// and for which `in_use` returns false. Entries that are not version
/// directories (staging leftovers, stray files, unparsable names) and the
/// running and newer versions are left alone. Returns the removed
/// directories.
///
/// A missing `root` is not an error (nothing installed, nothing to clean).
/// A failure to remove one directory is logged and does not stop the others;
/// it is retried on the next call.
pub fn remove_older_versions(
    root: &Path,
    running: &str,
    in_use: impl Fn(&Path) -> bool,
) -> io::Result<Vec<PathBuf>> {
    let running = super::parse_version(running).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("running engine version {running:?} is not a version"),
        )
    })?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut removed = Vec::new();
    for entry in entries {
        let entry = entry?;
        // symlink_metadata via file_type(): never follow a link out of root.
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(version) = entry.file_name().to_str().and_then(super::parse_version) else {
            continue;
        };
        if version >= running {
            continue;
        }
        let path = entry.path();
        if in_use(&path) {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => removed.push(path),
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "could not remove old engine version");
            }
        }
    }
    removed.sort();
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn version_dir(root: &Path, v: &str) -> PathBuf {
        let dir = root.join(v);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("pidash-agent-engine"), b"x").unwrap();
        dir
    }

    #[test]
    fn removes_only_versions_older_than_the_running_one() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let old = version_dir(root, "0.150.0");
        let older = version_dir(root, "0.99.2");
        let running = version_dir(root, "0.153.4");
        let newer = version_dir(root, "0.160.0");

        let removed = remove_older_versions(root, "0.153.4", |_| false).unwrap();
        assert_eq!(removed, {
            let mut v = vec![old.clone(), older.clone()];
            v.sort();
            v
        });
        assert!(!old.exists() && !older.exists());
        assert!(running.exists() && newer.exists());
    }

    #[test]
    fn versions_in_use_are_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let busy = version_dir(root, "0.150.0");
        let idle = version_dir(root, "0.151.0");

        let removed = remove_older_versions(root, "0.153.4", |p| p == busy).unwrap();
        assert_eq!(removed, vec![idle.clone()]);
        assert!(busy.exists());
        assert!(!idle.exists());
    }

    #[test]
    fn prerelease_orders_before_its_release() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pre = version_dir(root, "0.153.4-alpha.1");
        let removed = remove_older_versions(root, "0.153.4", |_| false).unwrap();
        assert_eq!(removed, vec![pre]);
    }

    #[test]
    fn non_version_entries_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let staging = root.join(".staging-1234");
        fs::create_dir_all(&staging).unwrap();
        fs::write(root.join("README.txt"), b"x").unwrap();
        let odd = root.join("latest");
        fs::create_dir_all(&odd).unwrap();

        assert!(
            remove_older_versions(root, "0.153.4", |_| false)
                .unwrap()
                .is_empty()
        );
        assert!(staging.exists() && odd.exists());
    }

    #[test]
    fn missing_root_is_nothing_to_do() {
        let tmp = tempfile::tempdir().unwrap();
        let removed =
            remove_older_versions(&tmp.path().join("absent"), "0.153.4", |_| false).unwrap();
        assert!(removed.is_empty());
    }

    #[test]
    fn unparsable_running_version_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        version_dir(tmp.path(), "0.1.0");
        assert!(remove_older_versions(tmp.path(), "override", |_| false).is_err());
        assert!(tmp.path().join("0.1.0").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_version_dirs_are_not_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = version_dir(outside.path(), "keep");
        std::os::unix::fs::symlink(&target, tmp.path().join("0.1.0")).unwrap();

        assert!(
            remove_older_versions(tmp.path(), "0.153.4", |_| false)
                .unwrap()
                .is_empty()
        );
        assert!(target.join("pidash-agent-engine").exists());
    }
}
