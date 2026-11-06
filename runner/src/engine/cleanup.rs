//! Delete engine versions the daemon no longer needs.
//!
//! `pidash update` leaves the previous engine version directory in place
//! (a daemon that resolved it at start may still be running it). Once the
//! daemon runs a newer engine and no run uses the older directories, they
//! can go. The daemon calls [`cleanup_after_start`] from its reconcile tick
//! (`daemon::engine_reconcile`) until it has run once with no run in flight;
//! a directory a live process still executes from is kept
//! ([`dir_in_use_by_a_process`]).

use std::io;
use std::path::{Path, PathBuf};

/// The daemon's wiring: with the engine it resolved at start, remove the
/// older versions beside it. `None` when there is nothing to do now: the
/// engine is a config override (no version directory), or `busy` (a run or
/// chat turn is in flight — the caller tries again later). `in_use` is asked
/// per directory, as for [`remove_older_versions`].
pub fn cleanup_after_start(
    engine: &super::locate::ResolvedEngine,
    busy: bool,
    in_use: impl Fn(&Path) -> bool,
) -> Option<io::Result<Cleanup>> {
    let version = engine.version.as_deref()?;
    let version_dir = engine.binary.parent()?;
    let root = version_dir.parent()?;
    if busy {
        return None;
    }
    Some(remove_older_versions(root, version, in_use))
}

/// Whether a running process executes a binary inside `dir` (Linux:
/// `/proc/<pid>/exe`). Elsewhere `false`; on Windows the rename that
/// [`remove_older_versions`] does before deleting fails for a directory
/// whose binary still runs, which keeps it.
pub fn dir_in_use_by_a_process(dir: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return false;
        };
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name
                .to_str()
                .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
            {
                continue;
            }
            if let Ok(exe) = std::fs::read_link(entry.path().join("exe"))
                && exe.starts_with(&dir)
            {
                return true;
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = dir;
        false
    }
}

/// What one [`remove_older_versions`] pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cleanup {
    /// Version directories removed.
    pub removed: Vec<PathBuf>,
    /// Older version directories still on disk: in use, or their removal
    /// failed. The caller tries again later while this is not empty.
    pub left: Vec<PathBuf>,
}

/// Prefix of a version directory being removed (see [`remove_older_versions`]).
const REMOVING_PREFIX: &str = ".removing-";

/// Remove every `<root>/<version>/` whose version is older than `running`
/// and for which `in_use` returns false. Entries that are not version
/// directories (staging leftovers, stray files, unparsable names) and the
/// running and newer versions are left alone.
///
/// Each directory is first renamed to `<root>/.removing-<version>` and only
/// then deleted. On Windows a directory whose engine binary still runs can't
/// be renamed, so the rename is the in-use probe there: the directory is
/// kept whole instead of being half deleted. A `.removing-*` directory left
/// by an earlier failed delete is deleted again.
///
/// A missing `root` is not an error (nothing installed, nothing to clean).
/// A failure on one directory does not stop the others; it is reported in
/// [`Cleanup::left`] so the caller retries.
pub fn remove_older_versions(
    root: &Path,
    running: &str,
    in_use: impl Fn(&Path) -> bool,
) -> io::Result<Cleanup> {
    let running = super::parse_version(running).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("running engine version {running:?} is not a version"),
        )
    })?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Cleanup::default()),
        Err(e) => return Err(e),
    };

    let mut out = Cleanup::default();
    for entry in entries {
        let entry = entry?;
        // symlink_metadata via file_type(): never follow a link out of root.
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(rest) = name.strip_prefix(REMOVING_PREFIX) {
            // An earlier delete got this far only; it is ours to finish.
            if super::parse_version(rest).is_some() {
                match std::fs::remove_dir_all(&path) {
                    Ok(()) => out.removed.push(path),
                    Err(err) => {
                        tracing::warn!(path = %path.display(), error = %err, "could not remove old engine version");
                        out.left.push(path);
                    }
                }
            }
            continue;
        }
        let Some(version) = super::parse_version(name) else {
            continue;
        };
        if version >= running {
            continue;
        }
        if in_use(&path) {
            out.left.push(path);
            continue;
        }
        let doomed = root.join(format!("{REMOVING_PREFIX}{name}"));
        if let Err(err) = std::fs::rename(&path, &doomed) {
            tracing::warn!(path = %path.display(), error = %err, "old engine version is still in use; kept");
            out.left.push(path);
            continue;
        }
        match std::fs::remove_dir_all(&doomed) {
            Ok(()) => out.removed.push(path),
            Err(err) => {
                tracing::warn!(path = %doomed.display(), error = %err, "could not remove old engine version");
                out.left.push(doomed);
            }
        }
    }
    out.removed.sort();
    out.left.sort();
    Ok(out)
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

        let removed = remove_older_versions(root, "0.153.4", |_| false)
            .unwrap()
            .removed;
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

        let out = remove_older_versions(root, "0.153.4", |p| p == busy).unwrap();
        assert_eq!(out.removed, vec![idle.clone()]);
        assert_eq!(out.left, vec![busy.clone()]);
        assert!(busy.exists());
        assert!(!idle.exists());
    }

    #[test]
    fn prerelease_orders_before_its_release() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pre = version_dir(root, "0.153.4-alpha.1");
        let removed = remove_older_versions(root, "0.153.4", |_| false)
            .unwrap()
            .removed;
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
                .removed
                .is_empty()
        );
        assert!(staging.exists() && odd.exists());
    }

    #[test]
    fn daemon_wiring_removes_old_versions_beside_the_resolved_engine() {
        use super::super::locate::{EngineSource, Layout, ResolvedEngine};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("lib/pidash/engine");
        let old = version_dir(&root, "0.150.0");
        let running = version_dir(&root, "0.153.4");
        let engine = ResolvedEngine {
            binary: running.join("pidash-agent-engine"),
            version: Some("0.153.4".into()),
            source: EngineSource::Installed(Layout::UnixLib),
        };
        // A run in flight: nothing yet.
        assert!(cleanup_after_start(&engine, true, |_| false).is_none());
        assert!(old.exists());
        // A process still executing the old engine keeps it.
        let kept = cleanup_after_start(&engine, false, |d| d == old)
            .unwrap()
            .unwrap();
        assert!(kept.removed.is_empty() && old.exists());
        assert_eq!(kept.left, vec![old.clone()]);
        // Idle: the old version goes, the running one stays.
        let removed = cleanup_after_start(&engine, false, |_| false)
            .unwrap()
            .unwrap()
            .removed;
        assert_eq!(removed, vec![old.clone()]);
        assert!(!old.exists() && running.exists());
        // A config override has no version directory to clean beside.
        let custom = ResolvedEngine {
            binary: running.join("pidash-agent-engine"),
            version: None,
            source: EngineSource::ConfigOverride,
        };
        assert!(cleanup_after_start(&custom, false, |_| false).is_none());
    }

    #[test]
    fn a_half_removed_version_is_finished_on_the_next_pass() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let leftover = version_dir(root, ".removing-0.140.0");
        let running = version_dir(root, "0.153.4");
        let out = remove_older_versions(root, "0.153.4", |_| false).unwrap();
        assert_eq!(out.removed, vec![leftover.clone()]);
        assert!(out.left.is_empty());
        assert!(!leftover.exists() && running.exists());
        // Not a removal of ours: left alone.
        let other = version_dir(root, ".removing-stuff");
        remove_older_versions(root, "0.153.4", |_| false).unwrap();
        assert!(other.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_running_test_binary_counts_as_in_use() {
        let exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert!(dir_in_use_by_a_process(exe.parent().unwrap()));
        let tmp = tempfile::tempdir().unwrap();
        assert!(!dir_in_use_by_a_process(tmp.path()));
    }

    #[test]
    fn missing_root_is_nothing_to_do() {
        let tmp = tempfile::tempdir().unwrap();
        let out = remove_older_versions(&tmp.path().join("absent"), "0.153.4", |_| false).unwrap();
        assert_eq!(out, Cleanup::default());
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
                .removed
                .is_empty()
        );
        assert!(target.join("pidash-agent-engine").exists());
    }
}
