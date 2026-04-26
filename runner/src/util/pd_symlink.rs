//! Optional `pd` shortcut installer.
//!
//! Run during `pidash configure` (both register and partial-edit paths).
//! If no `pd` command resolves on the user's `PATH`, drop a relative
//! symlink (`pd` → `pidash`) next to the running `pidash` binary so users
//! can type `pd <args>` instead of `pidash <args>`. If `pd` is already
//! taken, we leave it alone — silently shadowing whatever the user already
//! has on their `PATH` is the kind of surprise that generates support
//! tickets.
//!
//! ### Placement strategy
//!
//! We deliberately do **not** canonicalize `current_exe()` before picking
//! the parent dir. On Homebrew, `pidash` lives at
//! `/opt/homebrew/Cellar/pidash/<ver>/bin/pidash` with a brew-managed
//! symlink at `/opt/homebrew/bin/pidash` — only the latter is on `PATH`.
//! Canonicalizing would point us at the Cellar dir, which brew doesn't
//! expose. Using `current_exe()`'s un-canonicalized parent lands `pd` in
//! the same dir the user invoked `pidash` from, i.e. the dir that's
//! actually on `PATH`. Combining that with a *relative* symlink target
//! (`pd` → `pidash`) means brew / cargo upgrades that re-point the
//! `pidash` symlink are automatically followed by `pd` without any
//! re-installation on our part.
//!
//! Caveat: on Linux, `std::env::current_exe()` resolves
//! `/proc/self/exe` and is therefore always canonical. For cargo /
//! package-manager installs this is fine (binary lives directly in a
//! `PATH` dir). Linuxbrew users will hit the Homebrew/Cellar issue
//! described above — uncommon enough that we accept the shortcoming.
//!
//! ### Failure handling
//!
//! Best-effort: every failure (read-only fs, permission denied, Windows
//! without symlink privileges) collapses into a `PdAction` variant the
//! caller can log. Nothing here ever propagates an error out of `configure`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdAction {
    /// Newly created `pd` shortcut at this path.
    Created(PathBuf),
    /// `pd` already exists at the target path and resolves to our `pidash`.
    /// Idempotent re-run path — caller stays silent.
    AlreadyOurs(PathBuf),
    /// `pd` already exists somewhere on `PATH` (or at the target path
    /// pointing at something else); we declined to overwrite it.
    SkippedTaken { existing: PathBuf },
    /// Platform doesn't support what we tried (e.g. Windows symlink without
    /// admin / Developer Mode). Caller surfaces the message.
    SkippedUnsupported(String),
    /// Could not even compute where to place the shortcut, or the symlink
    /// syscall failed. Caller surfaces the message.
    Failed(String),
}

/// Best-effort install of a `pd` shortcut alongside the currently running
/// `pidash` binary. Never panics, never errors out — returns a `PdAction`
/// the caller can pretty-print.
pub fn try_install() -> PdAction {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return PdAction::Failed(format!("could not locate current exe: {e}")),
    };
    let parent = match exe.parent() {
        Some(p) => p.to_path_buf(),
        None => return PdAction::Failed("current exe has no parent dir".into()),
    };
    // The canonical pidash path is only used to recognize a `pd` symlink
    // we created on a previous run (idempotency check). Placement uses the
    // un-canonicalized parent — see module-level docs.
    let pidash_canon = std::fs::canonicalize(&exe).unwrap_or(exe);
    try_install_at(&parent, &pidash_canon, std::env::var_os("PATH").as_deref())
}

/// Inner, testable variant. Splits the `current_exe` lookup and the `PATH`
/// env read from the install logic so tests can drive the algorithm with a
/// tempdir + fake binary path + a synthetic `PATH` (e.g. an empty one to
/// hide a real `pd` that exists on the host running the test suite).
fn try_install_at(parent: &Path, pidash_canon: &Path, path_var: Option<&OsStr>) -> PdAction {
    let pd_name = pd_binary_name();
    let pd_candidate = parent.join(pd_name);

    if pd_candidate.symlink_metadata().is_ok() {
        if points_to(&pd_candidate, pidash_canon) {
            return PdAction::AlreadyOurs(pd_candidate);
        }
        return PdAction::SkippedTaken {
            existing: pd_candidate,
        };
    }

    if let Some(existing) = find_on_path(pd_name, path_var) {
        return PdAction::SkippedTaken { existing };
    }

    create_shortcut(&pd_candidate)
}

#[cfg(unix)]
fn create_shortcut(pd_candidate: &Path) -> PdAction {
    // Relative symlink: `pd` → `pidash` resolves within the same dir, so
    // brew/cargo upgrades that re-point `pidash` are automatically picked
    // up without us having to re-run.
    match std::os::unix::fs::symlink(pidash_link_target(), pd_candidate) {
        Ok(()) => PdAction::Created(pd_candidate.to_path_buf()),
        Err(e) => PdAction::Failed(format!(
            "could not create symlink at {}: {e}",
            pd_candidate.display()
        )),
    }
}

#[cfg(windows)]
fn create_shortcut(pd_candidate: &Path) -> PdAction {
    // Windows symlinks need admin rights or Developer Mode. We deliberately
    // do NOT fall back to `fs::copy`: a stale copy left behind after the
    // user upgrades `pidash` would be worse than no shortcut at all (silent
    // version skew, with no easy way for the user to know `pd` is stale).
    match std::os::windows::fs::symlink_file(pidash_link_target(), pd_candidate) {
        Ok(()) => PdAction::Created(pd_candidate.to_path_buf()),
        Err(e) => PdAction::SkippedUnsupported(format!(
            "Windows symlink failed (run as Administrator or enable Developer Mode): {e}"
        )),
    }
}

fn pd_binary_name() -> &'static str {
    if cfg!(windows) { "pd.exe" } else { "pd" }
}

fn pidash_link_target() -> &'static str {
    if cfg!(windows) { "pidash.exe" } else { "pidash" }
}

/// True if `link` is a symlink whose canonicalized target equals
/// `pidash_canon`. Used to detect "we already installed this on a previous
/// run" — anything else (regular file, dir, dangling symlink, symlink to a
/// different binary) is left alone.
fn points_to(link: &Path, pidash_canon: &Path) -> bool {
    let Ok(meta) = link.symlink_metadata() else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    let Ok(resolved) = std::fs::canonicalize(link) else {
        return false;
    };
    resolved == pidash_canon
}

/// Walk the supplied `PATH` value looking for a binary called `name`.
/// Returns the first hit so the caller can tell the user *what* is
/// shadowing the shortcut. Skips directory entries (a dir named `pd`
/// wouldn't shadow execution, so it's not a real conflict).
fn find_on_path(name: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    let path = path_var?;
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(name);
        if let Ok(meta) = candidate.symlink_metadata()
            && !meta.is_dir()
        {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;

    /// Empty PATH so `find_on_path` can't see a real `pd` shadowing the
    /// host. Avoids mutating `std::env`, which the crate forbids
    /// (`#![forbid(unsafe_code)]`).
    fn empty_path() -> OsString {
        OsString::new()
    }

    /// Helper: build a tempdir with a fake `pidash` binary inside, and
    /// return both the tempdir and the canonical path of the binary
    /// (`try_install_at` expects the canonical pidash path for idempotency
    /// comparison).
    fn fake_install() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bin_name = if cfg!(windows) { "pidash.exe" } else { "pidash" };
        let bin = tmp.path().join(bin_name);
        fs::write(&bin, b"#!/bin/sh\necho fake\n").expect("write bin");
        let bin_canon = fs::canonicalize(&bin).expect("canonicalize bin");
        (tmp, bin, bin_canon)
    }

    #[test]
    fn creates_symlink_when_pd_is_absent() {
        let (tmp, bin, bin_canon) = fake_install();
        let path = empty_path();

        let action = try_install_at(tmp.path(), &bin_canon, Some(path.as_os_str()));
        match action {
            PdAction::Created(p) => assert_eq!(p, tmp.path().join(pd_binary_name())),
            other => panic!("expected Created, got {other:?}"),
        }
        // The symlink resolves to our fake binary…
        let resolved = fs::canonicalize(tmp.path().join(pd_binary_name())).unwrap();
        assert_eq!(resolved, fs::canonicalize(&bin).unwrap());
        // …and on Unix the link target is the relative name "pidash", so
        // it tracks brew/cargo binary moves without re-installation.
        #[cfg(unix)]
        {
            let link_target = fs::read_link(tmp.path().join(pd_binary_name())).unwrap();
            assert_eq!(link_target, PathBuf::from(pidash_link_target()));
        }
    }

    #[test]
    fn idempotent_when_pd_already_points_to_pidash() {
        let (tmp, _bin, bin_canon) = fake_install();
        let path = empty_path();

        let _ = try_install_at(tmp.path(), &bin_canon, Some(path.as_os_str()));
        let action = try_install_at(tmp.path(), &bin_canon, Some(path.as_os_str()));
        match action {
            PdAction::AlreadyOurs(p) => assert_eq!(p, tmp.path().join(pd_binary_name())),
            other => panic!("expected AlreadyOurs, got {other:?}"),
        }
    }

    #[test]
    fn skips_when_pd_at_target_points_elsewhere() {
        let (tmp, _bin, bin_canon) = fake_install();
        let path = empty_path();

        let other = tmp.path().join(pd_binary_name());
        fs::write(&other, b"someone else's pd\n").unwrap();

        let action = try_install_at(tmp.path(), &bin_canon, Some(path.as_os_str()));
        match action {
            PdAction::SkippedTaken { existing } => assert_eq!(existing, other),
            other => panic!("expected SkippedTaken, got {other:?}"),
        }
    }

    #[test]
    fn skips_when_pd_lives_elsewhere_on_path() {
        let (tmp, _bin, bin_canon) = fake_install();
        let elsewhere = tempfile::tempdir().expect("tempdir2");
        let decoy = elsewhere.path().join(pd_binary_name());
        fs::write(&decoy, b"another pd\n").unwrap();
        let path: OsString = elsewhere.path().as_os_str().to_owned();

        let action = try_install_at(tmp.path(), &bin_canon, Some(path.as_os_str()));
        match action {
            PdAction::SkippedTaken { existing } => assert_eq!(existing, decoy),
            other => panic!("expected SkippedTaken, got {other:?}"),
        }
    }

    #[test]
    fn ignores_directory_named_pd_on_path() {
        let (tmp, _bin, bin_canon) = fake_install();
        // PATH contains a dir whose only entry called `pd` is itself a
        // subdirectory. A subdir can't shadow execution, so we should
        // proceed with creating the symlink.
        let elsewhere = tempfile::tempdir().expect("tempdir2");
        fs::create_dir(elsewhere.path().join(pd_binary_name())).unwrap();
        let path: OsString = elsewhere.path().as_os_str().to_owned();

        let action = try_install_at(tmp.path(), &bin_canon, Some(path.as_os_str()));
        match action {
            PdAction::Created(p) => assert_eq!(p, tmp.path().join(pd_binary_name())),
            other => panic!("expected Created, got {other:?}"),
        }
    }
}
