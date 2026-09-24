use std::path::{Path, PathBuf};
use thiserror::Error;

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
    Cloned(PathBuf),
}

/// Resolve the directory the agent runs in.
///
/// Git is *context*, never a gate. The platform hands the agent a directory
/// plus the repository facts it knows (url, base branch, work branch — see the
/// prompt's repo context) and the agent decides what, if anything, to do with
/// Git. PDASHOSS01-136 already removed platform-side branch checkout on this
/// reasoning; clone bootstrap is the same kind of decision.
///
/// So `repo_url` is a best-effort *convenience* for a working dir that is empty
/// and therefore unambiguous to populate — never a requirement imposed on the
/// workspace. When it cannot be honoured (dir already has files, url is a shape
/// we won't hand to `git clone`, clone itself failed) the run proceeds in the
/// directory as an ordinary task folder. The only failure left is a working dir
/// that cannot be used as a directory at all.
pub async fn resolve(
    working_dir: &Path,
    repo_url: Option<&str>,
) -> Result<Resolution, ResolveError> {
    if let Err(e) = std::fs::create_dir_all(working_dir) {
        return Err(ResolveError::Io(e));
    }
    let dir = working_dir.to_path_buf();
    if git::is_git_repo(working_dir) {
        return Ok(Resolution::ExistingRepo(dir));
    }
    let Some(url) = repo_url.filter(|url| !url.trim().is_empty()) else {
        return Ok(Resolution::Directory(dir));
    };
    // Bootstrap only what is unambiguously safe to bootstrap. A directory that
    // already holds files belongs to the operator — it may be a parent of
    // clones, a multi-repo workspace, or a plain task folder — so never clone
    // over it, and never fail the run over it either.
    if !git::is_empty_dir(working_dir) {
        tracing::info!(
            working_dir = ?dir,
            "working dir is not a git repo and is not empty; running it as a \
             task folder without cloning — the agent drives git itself"
        );
        return Ok(Resolution::Directory(dir));
    }
    if !is_supported_clone_url(url) {
        // Deliberately not logged: a clone url can embed a token.
        tracing::warn!(
            working_dir = ?dir,
            "repo_url is not a shape we hand to `git clone`; skipping clone \
             bootstrap and running as a task folder"
        );
        return Ok(Resolution::Directory(dir));
    }
    match git::clone(url, working_dir).await {
        Ok(()) => Ok(Resolution::Cloned(dir)),
        Err(err) => {
            tracing::warn!(
                working_dir = ?dir,
                error = %err,
                "clone bootstrap failed; running as a task folder — the agent \
                 can clone with its own credentials if the task needs the repo"
            );
            Ok(Resolution::Directory(dir))
        }
    }
}

/// Defense-in-depth: only hand `git clone` URL forms we expect from the cloud.
/// `git clone --` already prevents flag-injection, but odd shapes (newlines,
/// leading dashes, `ext::`, `file://`) keep the surface small. A rejection
/// skips the clone; it does not fail the run.
fn is_supported_clone_url(url: &str) -> bool {
    if url.is_empty() || url.starts_with('-') {
        return false;
    }
    if url.chars().any(|c| c.is_control()) {
        return false;
    }
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("git@")
        || lower.starts_with("ssh://")
        || lower.starts_with("git://")
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

    /// A url shape we refuse to hand to `git clone` skips the bootstrap. The
    /// security property is unchanged — `git clone` is never invoked — but a
    /// malformed url no longer kills a run that may not need git at all.
    #[tokio::test]
    async fn skips_clone_bootstrap_for_unsupported_repo_url() {
        for url in [
            "--upload-pack=evil",
            "https://x.test/a.git\nrm -rf",
            "ext::sh -c evil",
            "file:///etc",
        ] {
            let tmp = tempdir().unwrap();
            let resolution = resolve(tmp.path(), Some(url)).await.unwrap();
            assert!(
                matches!(resolution, Resolution::Directory(_)),
                "expected task folder for {url:?}"
            );
            assert!(!tmp.path().join(".git").exists(), "cloned for {url:?}");
        }
    }

    /// A clone that fails (bad credentials, unreachable host, wrong url) leaves
    /// the runner in an ordinary task folder rather than failing the run: the
    /// agent has the repo url in its prompt context and its own credentials, so
    /// cloning is its call to make.
    #[tokio::test]
    async fn falls_back_to_task_folder_when_clone_fails() {
        let tmp = tempdir().unwrap();
        // Port 1 on loopback refuses immediately — no DNS, no network egress.
        let resolution = resolve(tmp.path(), Some("https://127.0.0.1:1/x.git"))
            .await
            .unwrap();
        assert!(matches!(resolution, Resolution::Directory(p) if p == tmp.path()));
        assert!(!tmp.path().join(".git").exists());
    }
}
