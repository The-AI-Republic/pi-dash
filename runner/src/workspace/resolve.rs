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
    // `create_dir_all` succeeds on an existing directory whatever its mode, so
    // it does not prove the directory is usable. Probe it: a working dir the
    // runner cannot read is the one thing that genuinely has no recovery — the
    // agent could not work there either.
    if let Err(e) = std::fs::read_dir(working_dir) {
        return Err(ResolveError::Io(e));
    }
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
    if !is_bootstrappable(working_dir).map_err(ResolveError::Io)? {
        tracing::info!(
            working_dir = ?dir,
            "working dir is not a git repo and is not empty; running it as a \
             task folder without cloning — the agent drives git itself"
        );
        return Ok(Resolution::Directory(dir));
    }
    if !is_supported_clone_url(url) {
        tracing::warn!(
            working_dir = ?dir,
            // The shape only — a clone url can embed a token.
            scheme = url_shape(url),
            "repo_url is not a shape we hand to `git clone`; skipping clone \
             bootstrap and running as a task folder"
        );
        return Ok(Resolution::Directory(dir));
    }
    match git::clone(url, working_dir).await {
        Ok(()) => Ok(Resolution::Cloned(dir)),
        Err(err) => {
            // `git clone` usually tears its own partial checkout down, but not
            // always: a clone that fetches successfully and then fails to
            // *check out* (missing smudge filter, disk full, path collision)
            // exits non-zero and deliberately leaves `.git` plus a
            // half-populated worktree behind. Handing that to the agent is
            // worse than handing it nothing — `git status` reports every
            // un-checked-out file as deleted, and the agent has been told to
            // work on this repo, so a routine "commit your work" can commit
            // mass deletions. It is also sticky: the next run would take the
            // `ExistingRepo` branch and never re-clone.
            //
            // Everything here is ours to remove: the dir held nothing but
            // runner metadata immediately before the clone, and it is this
            // runner's exclusive working dir (enforced by config validation).
            // Clearing it also makes the failure self-healing — the next run
            // sees a bootstrappable dir again and retries the clone.
            if let Err(cleanup) = discard_partial_clone(working_dir) {
                tracing::error!(
                    working_dir = ?dir,
                    error = %cleanup,
                    "could not remove a failed clone's leftovers; the next run \
                     may find a partially checked-out repo here"
                );
            }
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

/// Entries the runner writes into a working dir itself, which must not count
/// as "the operator already put files here".
///
/// `.pidash/context.md` is (re)written after *every* run by
/// `cli::context::write_context_for_project`. Counting it would mean a single
/// failed clone permanently disabled clone bootstrap for that runner: the
/// fallback run writes `.pidash/`, and every later run then sees a non-empty
/// directory and skips the clone it should have retried.
const RUNNER_OWNED_ENTRIES: &[&str] = &[".pidash"];

/// Whether the directory holds nothing except runner-owned metadata, and is
/// therefore unambiguous to populate with a clone.
///
/// An unreadable directory is an error rather than "not empty": the two are
/// indistinguishable to `read_dir`, and silently treating one as the other is
/// how an unusable working dir reaches the agent.
fn is_bootstrappable(path: &Path) -> std::io::Result<bool> {
    for entry in std::fs::read_dir(path)? {
        let name = entry?.file_name();
        if !RUNNER_OWNED_ENTRIES.iter().any(|owned| name == *owned) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Remove what a failed `git clone` left behind, preserving runner-owned
/// metadata. Only ever called on a directory that was bootstrappable moments
/// before, so everything it deletes was written by that clone.
fn discard_partial_clone(path: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if RUNNER_OWNED_ENTRIES
            .iter()
            .any(|owned| entry.file_name() == *owned)
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// A non-secret discriminator for a rejected clone url, so an operator can tell
/// a misconfigured `repo_url` from a missing one without the value reaching the
/// logs — userinfo (`https://user:token@host`) never survives this.
fn url_shape(url: &str) -> &'static str {
    match url.split_once("://") {
        Some(("https", _)) => "https",
        Some(("http", _)) => "http",
        Some(("ssh", _)) => "ssh",
        Some(("git", _)) => "git",
        Some(_) => "other-scheme",
        None if url.contains('@') => "scp-like",
        None => "no-scheme",
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
        // The path we hand back must still be usable: `git clone` tears down
        // the partial checkout on failure, but it leaves a target directory it
        // did not create. If that ever changed we would be handing the agent a
        // path that no longer exists.
        assert!(tmp.path().is_dir());
        assert!(!tmp.path().join(".git").exists());
    }

    /// A failed bootstrap must not disable bootstrap forever. `.pidash/` is
    /// rewritten into the working dir after every run, so if it counted as
    /// "the operator put files here", one transient clone failure would mean
    /// the clone was never retried.
    #[tokio::test]
    async fn runner_metadata_does_not_block_a_later_clone_bootstrap() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".pidash")).unwrap();
        std::fs::write(tmp.path().join(".pidash/context.md"), "# ctx").unwrap();

        assert!(is_bootstrappable(tmp.path()).unwrap());

        // Anything the operator owns still stops the clone.
        std::fs::write(tmp.path().join("notes.txt"), "mine").unwrap();
        assert!(!is_bootstrappable(tmp.path()).unwrap());
    }

    /// A clone that fetches and then fails to *check out* leaves `.git` and a
    /// partial worktree behind. Reported as a task folder it would hand the
    /// agent a repo whose tracked files all look deleted, and stick: the next
    /// run would resolve `ExistingRepo` and never re-clone.
    #[tokio::test]
    async fn discards_a_partial_clone_but_keeps_runner_metadata() {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".pidash")).unwrap();
        std::fs::write(tmp.path().join(".pidash/context.md"), "# ctx").unwrap();
        // The shape `git clone` leaves when checkout fails.
        std::fs::create_dir_all(tmp.path().join(".git/objects")).unwrap();
        std::fs::write(tmp.path().join(".gitattributes"), "*.bin filter=x").unwrap();

        discard_partial_clone(tmp.path()).unwrap();

        assert!(!tmp.path().join(".git").exists());
        assert!(!tmp.path().join(".gitattributes").exists());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(".pidash/context.md")).unwrap(),
            "# ctx"
        );
        // Bootstrappable again, so the next run retries the clone.
        assert!(is_bootstrappable(tmp.path()).unwrap());
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

    #[test]
    fn url_shape_never_leaks_userinfo() {
        assert_eq!(url_shape("https://user:tok@host/r.git"), "https");
        assert_eq!(url_shape("git@github.com:org/repo.git"), "scp-like");
        assert_eq!(url_shape("github.com:org/repo"), "no-scheme");
        // `ext::` carries no `://`, so it lands in the catch-all rather than
        // being reported as a scheme.
        assert_eq!(url_shape("ext::sh -c evil"), "no-scheme");
        assert_eq!(url_shape("ftp://host/r.git"), "other-scheme");
        for url in [
            "https://user:tok@host/r.git",
            "git@github.com:org/repo.git",
            "ext::sh -c evil",
        ] {
            assert!(!url_shape(url).contains("tok"));
            assert!(!url_shape(url).contains("org"));
        }
    }
}
