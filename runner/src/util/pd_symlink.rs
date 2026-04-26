//! Optional `pd` shortcut installer.
//!
//! Run once during `pidash configure`'s initial register flow. If no `pd`
//! command resolves on the user's `PATH`, drop a symlink (Unix) or file
//! copy (Windows fallback) next to the running `pidash` binary so users can
//! type `pd <args>` instead of `pidash <args>`. If `pd` is already taken,
//! we leave it alone — silently shadowing whatever the user already has on
//! their `PATH` is the kind of surprise that generates support tickets.
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
    /// Could not even compute where to place the shortcut, or the symlink /
    /// copy syscall failed. Caller surfaces the message.
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
    // Canonicalize so a `/usr/local/bin/pidash` symlink that points into the
    // Cellar still resolves to the real binary, which is what we want the
    // `pd` shortcut to point at.
    let pidash_path = std::fs::canonicalize(&exe).unwrap_or(exe);
    let parent = match pidash_path.parent() {
        Some(p) => p.to_path_buf(),
        None => return PdAction::Failed("current exe has no parent dir".into()),
    };
    try_install_at(&parent, &pidash_path, std::env::var_os("PATH").as_deref())
}

/// Inner, testable variant. Splits the `current_exe` lookup and the `PATH`
/// env read from the install logic so tests can drive the algorithm with a
/// tempdir + fake binary path + a synthetic `PATH` (e.g. an empty one to
/// hide a real `pd` that exists on the host running the test suite).
fn try_install_at(parent: &Path, pidash_path: &Path, path_var: Option<&OsStr>) -> PdAction {
    let pd_name = pd_binary_name();
    let pd_candidate = parent.join(pd_name);

    if pd_candidate.symlink_metadata().is_ok() {
        if points_to(&pd_candidate, pidash_path) {
            return PdAction::AlreadyOurs(pd_candidate);
        }
        return PdAction::SkippedTaken {
            existing: pd_candidate,
        };
    }

    if let Some(existing) = find_on_path(pd_name, path_var) {
        return PdAction::SkippedTaken { existing };
    }

    create_shortcut(pidash_path, &pd_candidate)
}

#[cfg(unix)]
fn create_shortcut(pidash_path: &Path, pd_candidate: &Path) -> PdAction {
    match std::os::unix::fs::symlink(pidash_path, pd_candidate) {
        Ok(()) => PdAction::Created(pd_candidate.to_path_buf()),
        Err(e) => PdAction::Failed(format!(
            "could not create symlink at {}: {e}",
            pd_candidate.display()
        )),
    }
}

#[cfg(windows)]
fn create_shortcut(pidash_path: &Path, pd_candidate: &Path) -> PdAction {
    // Windows symlinks need either admin rights or Developer Mode. Fall
    // back to a copy so the shortcut still works for non-admin users; they
    // just don't get the "tracks the original" property of a symlink, which
    // matters less since the binary path rarely moves between releases.
    match std::os::windows::fs::symlink_file(pidash_path, pd_candidate) {
        Ok(()) => PdAction::Created(pd_candidate.to_path_buf()),
        Err(_) => match std::fs::copy(pidash_path, pd_candidate) {
            Ok(_) => PdAction::Created(pd_candidate.to_path_buf()),
            Err(e) => PdAction::SkippedUnsupported(format!(
                "could not symlink or copy to {}: {e}",
                pd_candidate.display()
            )),
        },
    }
}

fn pd_binary_name() -> &'static str {
    if cfg!(windows) { "pd.exe" } else { "pd" }
}

/// True if `link` is a symlink whose canonicalized target is `pidash_path`.
/// Used to detect "we already installed this on a previous run."
fn points_to(link: &Path, pidash_path: &Path) -> bool {
    let Ok(meta) = link.symlink_metadata() else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    let Ok(resolved) = std::fs::canonicalize(link) else {
        return false;
    };
    let pidash_canon = std::fs::canonicalize(pidash_path).unwrap_or_else(|_| pidash_path.to_path_buf());
    resolved == pidash_canon
}

/// Walk the supplied `PATH` value looking for a binary called `name`.
/// Returns the first hit so the caller can tell the user *what* is
/// shadowing the shortcut.
fn find_on_path(name: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    let path = path_var?;
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(name);
        if candidate.symlink_metadata().is_ok() {
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

    /// Helper: build a tempdir with a fake `pidash` binary inside.
    fn fake_install() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bin_name = if cfg!(windows) { "pidash.exe" } else { "pidash" };
        let bin = tmp.path().join(bin_name);
        fs::write(&bin, b"#!/bin/sh\necho fake\n").expect("write bin");
        (tmp, bin)
    }

    #[test]
    fn creates_symlink_when_pd_is_absent() {
        let (tmp, bin) = fake_install();
        let path = empty_path();

        let action = try_install_at(tmp.path(), &bin, Some(path.as_os_str()));
        match action {
            PdAction::Created(p) => assert_eq!(p, tmp.path().join(pd_binary_name())),
            other => panic!("expected Created, got {other:?}"),
        }
        let resolved = fs::canonicalize(tmp.path().join(pd_binary_name())).unwrap();
        assert_eq!(resolved, fs::canonicalize(&bin).unwrap());
    }

    #[test]
    fn idempotent_when_pd_already_points_to_pidash() {
        let (tmp, bin) = fake_install();
        let path = empty_path();

        let _ = try_install_at(tmp.path(), &bin, Some(path.as_os_str()));
        let action = try_install_at(tmp.path(), &bin, Some(path.as_os_str()));
        match action {
            PdAction::AlreadyOurs(p) => assert_eq!(p, tmp.path().join(pd_binary_name())),
            other => panic!("expected AlreadyOurs, got {other:?}"),
        }
    }

    #[test]
    fn skips_when_pd_at_target_points_elsewhere() {
        let (tmp, bin) = fake_install();
        let path = empty_path();

        let other = tmp.path().join(pd_binary_name());
        fs::write(&other, b"someone else's pd\n").unwrap();

        let action = try_install_at(tmp.path(), &bin, Some(path.as_os_str()));
        match action {
            PdAction::SkippedTaken { existing } => assert_eq!(existing, other),
            other => panic!("expected SkippedTaken, got {other:?}"),
        }
    }

    #[test]
    fn skips_when_pd_lives_elsewhere_on_path() {
        let (tmp, bin) = fake_install();
        // Decoy `pd` in a separate dir, exposed via `PATH`.
        let elsewhere = tempfile::tempdir().expect("tempdir2");
        let decoy = elsewhere.path().join(pd_binary_name());
        fs::write(&decoy, b"another pd\n").unwrap();
        let path: OsString = elsewhere.path().as_os_str().to_owned();

        let action = try_install_at(tmp.path(), &bin, Some(path.as_os_str()));
        match action {
            PdAction::SkippedTaken { existing } => assert_eq!(existing, decoy),
            other => panic!("expected SkippedTaken, got {other:?}"),
        }
    }
}
