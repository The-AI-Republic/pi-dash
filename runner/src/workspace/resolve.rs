use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::util::fs::create_dir_all_private;
use crate::workspace::git;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
}

#[derive(Debug, Clone)]
pub enum Resolution {
    /// Ordinary task folder: Git is optional, and existing files are retained.
    Directory(PathBuf),
    ExistingRepo(PathBuf),
}

/// Resolve the directory the agent runs in.
///
/// Git is *context*, never a gate. The platform hands the agent a directory
/// plus the repository facts it knows (url, base branch, work branch — see the
/// prompt's repo context) and the agent decides what, if anything, to do with
/// Git. PDASHOSS01-136 already removed platform-side branch checkout on this
/// reasoning; PDASHOSS01-279 removes the last platform checkout, the clone
/// bootstrap.
///
/// `repo_url` is therefore intentionally ignored for directory setup: the
/// platform never runs `git clone` on the user's behalf. An empty directory
/// stays empty and an existing repository is left untouched. The only failure
/// left is a working dir that cannot be used as a directory at all.
pub async fn resolve(
    working_dir: &Path,
    _repo_url: Option<&str>,
) -> Result<Resolution, ResolveError> {
    // Owner-only when the daemon creates it (it holds agent work products);
    // an existing working dir's mode is never changed.
    if let Err(e) = create_dir_all_private(working_dir) {
        return Err(ResolveError::Io(e));
    }
    let dir = working_dir.to_path_buf();
    // `create_dir_all_private` succeeds on an existing directory whatever
    // its mode, so it does not prove the directory is usable. Probe it: a working dir the
    // runner cannot read is the one thing that genuinely has no recovery — the
    // agent could not work there either.
    if let Err(e) = std::fs::read_dir(working_dir) {
        return Err(ResolveError::Io(e));
    }
    if git::is_git_repo(working_dir) {
        return Ok(Resolution::ExistingRepo(dir));
    }
    Ok(Resolution::Directory(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn detects_existing_repo() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let r = resolve(tmp.path(), None).await.unwrap();
        assert!(matches!(r, Resolution::ExistingRepo(_)));
    }

    /// The PDASHOSS01-107 guard used to *fail the run* here. Git is context,
    /// not a gate: a working dir that already holds files (a multi-repo
    /// workspace, a parent of clones, a plain task folder) runs as-is. The one
    /// thing that must still hold is that we never clone over those files.
    #[tokio::test]
    async fn runs_as_task_folder_when_non_empty_and_not_a_repo() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("junk"), b"keep").unwrap();
        let resolution = resolve(tmp.path(), Some("https://example.com/repo.git"))
            .await
            .unwrap();
        assert!(matches!(resolution, Resolution::Directory(_)));
        assert_eq!(std::fs::read(tmp.path().join("junk")).unwrap(), b"keep");
        assert!(!tmp.path().join(".git").exists());
    }

    /// The shape the bug report hit: `working_dir` is a container of sibling
    /// clones, so it is neither a repo nor empty.
    #[tokio::test]
    async fn runs_as_task_folder_when_working_dir_holds_sibling_repos() {
        let tmp = tempdir().unwrap();
        for repo in ["pi-dash", "private-pi-dash"] {
            std::fs::create_dir_all(tmp.path().join(repo).join(".git")).unwrap();
        }
        let resolution = resolve(tmp.path(), Some("https://example.com/repo.git"))
            .await
            .unwrap();
        assert!(matches!(resolution, Resolution::Directory(p) if p == tmp.path()));
        assert!(tmp.path().join("pi-dash/.git").is_dir());
        assert!(tmp.path().join("private-pi-dash/.git").is_dir());
        assert!(!tmp.path().join(".git").exists());
    }

    #[tokio::test]
    async fn accepts_empty_directory_without_repo_url() {
        let tmp = tempdir().unwrap();
        let resolution = resolve(tmp.path(), None).await.unwrap();
        assert!(matches!(resolution, Resolution::Directory(_)));
        assert!(!tmp.path().join(".git").exists());
    }

    /// PDASHOSS01-279 acceptance: the platform runs no `git clone`. An empty
    /// working directory plus a `repo_url` stays empty after `resolve`; the
    /// agent receives the repo facts in its prompt and clones itself if the
    /// task needs the repo.
    #[tokio::test]
    async fn empty_directory_with_repo_url_stays_empty() {
        for url in [
            "https://example.com/repo.git",
            "git@github.com:org/repo.git",
            "--upload-pack=evil",
            "ext::sh -c evil",
        ] {
            let tmp = tempdir().unwrap();
            let resolution = resolve(tmp.path(), Some(url)).await.unwrap();
            assert!(
                matches!(resolution, Resolution::Directory(p) if p == tmp.path()),
                "expected task folder for {url:?}"
            );
            assert!(!tmp.path().join(".git").exists(), "cloned for {url:?}");
            assert_eq!(
                tmp.path().read_dir().unwrap().count(),
                0,
                "empty dir gained entries for {url:?}"
            );
        }
    }

    #[tokio::test]
    async fn preserves_existing_files_without_repo_url() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("notes.txt"), "user notes").unwrap();
        for url in [None, Some(""), Some("  ")] {
            assert!(matches!(
                resolve(tmp.path(), url).await.unwrap(),
                Resolution::Directory(_)
            ));
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("notes.txt")).unwrap(),
                "user notes"
            );
            assert!(!tmp.path().join(".git").exists());
        }
    }

    /// An existing repository is left untouched: `resolve` reports it and
    /// never clones over it or cleans it.
    #[tokio::test]
    async fn leaves_existing_repo_untouched_with_repo_url() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git/objects")).unwrap();
        std::fs::write(tmp.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(tmp.path().join("keep.txt"), "keep").unwrap();
        let resolution = resolve(tmp.path(), Some("https://example.com/other.git"))
            .await
            .unwrap();
        assert!(matches!(resolution, Resolution::ExistingRepo(_)));
        assert!(tmp.path().join(".git/HEAD").exists());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("keep.txt")).unwrap(),
            "keep"
        );
    }

    #[tokio::test]
    async fn creates_missing_task_directory() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("tasks/new");
        assert!(
            matches!(resolve(&path, None).await.unwrap(), Resolution::Directory(p) if p == path)
        );
        assert!(path.is_dir());
    }

    #[tokio::test]
    async fn rejects_file_as_working_directory() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("notes.txt");
        std::fs::write(&path, "keep").unwrap();
        assert!(matches!(
            resolve(&path, None).await.unwrap_err(),
            ResolveError::Io(_)
        ));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "keep");
    }

    /// An unreadable working dir is the one genuinely unrecoverable case, and
    /// must not be mistaken for "not empty" — the agent cannot work there.
    #[cfg(unix)]
    #[tokio::test]
    async fn unreadable_working_dir_is_an_io_error() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("locked");
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = resolve(&path, Some("https://example.com/repo.git")).await;

        // Restore before asserting so the tempdir can always clean itself up.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(result, Err(ResolveError::Io(_))));
    }

    /// A working dir the daemon creates must be owner-only: it holds agent
    /// work products (source checkouts, generated output, possibly secrets)
    /// under a home dir other local users may be able to traverse.
    /// `DirBuilderExt::mode(0o700)` is what guarantees this, so the check is
    /// meaningful on a standard umask-0022 host without manipulating the
    /// process umask (which is shared by every parallel test).
    #[cfg(unix)]
    #[tokio::test]
    async fn creates_a_missing_working_dir_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("tasks/new");
        let _ = resolve(&path, None).await.unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "created working dir must be owner-only");
        let parent_mode = std::fs::metadata(tmp.path().join("tasks"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            parent_mode, 0o700,
            "created parent dirs must be owner-only too"
        );
    }

    /// But a directory the operator already made keeps its mode: `resolve`
    /// creates what is missing, never chmods what exists.
    #[cfg(unix)]
    #[tokio::test]
    async fn leaves_an_existing_working_dir_mode_unchanged() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("shared");
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = resolve(&path, None).await.unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o755,
            "an existing working dir must not be chmodded by resolve"
        );
    }
}
