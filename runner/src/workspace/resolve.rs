use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::workspace::git;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("cannot clone into a non-empty working_dir that is not a git repo: {0:?}")]
    NonEmptyNonRepo(PathBuf),
    #[error("repo_url has an unsupported scheme: {0:?}")]
    UnsupportedScheme(String),
    #[error("git clone failed: {0}")]
    Clone(#[source] anyhow::Error),
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

pub async fn resolve(
    working_dir: &Path,
    repo_url: Option<&str>,
) -> Result<Resolution, ResolveError> {
    if let Err(e) = std::fs::create_dir_all(working_dir) {
        return Err(ResolveError::Io(e));
    }
    if git::is_git_repo(working_dir) {
        return Ok(Resolution::ExistingRepo(working_dir.to_path_buf()));
    }
    let Some(url) = repo_url.filter(|url| !url.trim().is_empty()) else {
        return Ok(Resolution::Directory(working_dir.to_path_buf()));
    };
    if !git::is_empty_dir(working_dir) {
        return Err(ResolveError::NonEmptyNonRepo(working_dir.to_path_buf()));
    }
    if !is_supported_clone_url(url) {
        return Err(ResolveError::UnsupportedScheme(url.to_string()));
    }
    git::clone(url, working_dir)
        .await
        .map_err(ResolveError::Clone)?;
    Ok(Resolution::Cloned(working_dir.to_path_buf()))
}

/// Defense-in-depth: only allow URL forms we expect to receive from the cloud.
/// `git clone --` already prevents flag-injection, but rejecting odd shapes
/// (newlines, leading dashes, ext::, file://) keeps the surface small.
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

    #[tokio::test]
    async fn refuses_clone_into_non_empty_non_repo() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("junk"), b"").unwrap();
        let err = resolve(tmp.path(), Some("https://example.com/repo.git"))
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::NonEmptyNonRepo(_)));
        assert_eq!(std::fs::read(tmp.path().join("junk")).unwrap(), b"");
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

    #[tokio::test]
    async fn rejects_flag_style_repo_url() {
        let tmp = tempdir().unwrap();
        let err = resolve(tmp.path(), Some("--upload-pack=evil"))
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::UnsupportedScheme(_)));
    }

    #[tokio::test]
    async fn rejects_repo_url_with_newline() {
        let tmp = tempdir().unwrap();
        let err = resolve(tmp.path(), Some("https://x.test/a.git\nrm -rf"))
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::UnsupportedScheme(_)));
    }
}
