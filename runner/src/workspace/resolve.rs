use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::workspace::git;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("working_dir is not a git repo and is not empty; refusing to operate on {0:?}")]
    NonEmptyNonRepo(PathBuf),
    #[error("assignment did not include repo_url and working_dir has no git repo")]
    MissingRepoUrl,
    #[error("git clone failed: {0}")]
    Clone(#[source] anyhow::Error),
    #[error("io error: {0}")]
    Io(#[source] std::io::Error),
}

#[derive(Debug, Clone)]
pub enum Resolution {
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
    if !git::is_empty_dir(working_dir) {
        return Err(ResolveError::NonEmptyNonRepo(working_dir.to_path_buf()));
    }
    let url = repo_url.ok_or(ResolveError::MissingRepoUrl)?;
    git::clone(url, working_dir)
        .await
        .map_err(ResolveError::Clone)?;
    Ok(Resolution::Cloned(working_dir.to_path_buf()))
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
    async fn refuses_non_empty_non_repo() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("junk"), b"").unwrap();
        let err = resolve(tmp.path(), None).await.unwrap_err();
        assert!(matches!(err, ResolveError::NonEmptyNonRepo(_)));
    }

    #[tokio::test]
    async fn errors_on_missing_url_for_empty_dir() {
        let tmp = tempdir().unwrap();
        let err = resolve(tmp.path(), None).await.unwrap_err();
        assert!(matches!(err, ResolveError::MissingRepoUrl));
    }
}
