//! Dedicated per-runner chat worktree, managed **outside** the issue pool.
//!
//! The chat lane runs in its own git worktree so it can read and write code in
//! parallel with an issue run on a pool desk, on a physically separate tree (so
//! the two can never collide). Unlike a pool desk, this worktree is:
//!
//! - **per-runner** (keyed by the runner's data dir), not per chat session, so
//!   it persists across chat sessions — terminal-style. Whatever branch/dirty
//!   state one session leaves, the next resumes in.
//! - **lazily created** on first chat use (runners that never chat cost nothing).
//! - **never auto-cleaned**: the pool's release path salvages → resets → cleans
//!   a desk (wiping the tree), which is the *opposite* of what a persistent chat
//!   tree needs. Here a healthy existing worktree is reused **as-is**; only a
//!   broken/partial one (e.g. a crash mid-create) is removed and recreated.
//!
//! See design `make_chat_issue_parallel_working` §3.1 / §3.6 / §3.7.
//!
//! Phase 2 (deferred) adds a commit/push-on-session-end lifecycle; until then
//! the tree simply persists with the operator's uncommitted work.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::util::paths::RunnerPaths;
use crate::workspace::git;

/// The dedicated chat worktree path for a runner: a stable sub-dir of the
/// runner's per-instance data dir, so it is inherently per-runner and is cleaned
/// up with the runner on `RemoveRunner`.
pub fn path_for(runner_paths: &RunnerPaths) -> PathBuf {
    runner_paths.base_dir().join("chat-worktree")
}

/// Ensure the runner's dedicated chat worktree exists and is healthy, returning
/// its path. Idempotent and persistence-preserving:
///
/// - existing + healthy → returned untouched (keeps dirty state);
/// - existing + broken  → removed (git bookkeeping + dir) and recreated;
/// - absent             → created from `canonical`'s current HEAD (detached,
///   matching how the pool materialises desks so no branch lock is taken).
///
/// `canonical` is the workdir's canonical clone (from `PoolHandle::canonical`).
pub async fn ensure(canonical: &Path, worktree: &Path) -> Result<PathBuf> {
    if worktree.exists() {
        if git::is_git_repo(worktree) {
            // Healthy and persistent — reuse with its state intact. We never
            // salvage/reset/clean here (that would wipe the user's terminal
            // state); Phase 2's commit+push lifecycle is where that lands.
            return Ok(worktree.to_path_buf());
        }
        // Broken or partially-created (crash mid-`worktree add`). There is no
        // usable state to preserve, so drop the git admin entry and the dir,
        // then recreate below.
        let _ = git::worktree_remove(canonical, worktree, true).await;
        let _ = tokio::fs::remove_dir_all(worktree).await;
    }

    // Drop any stale `.git/worktrees/<name>` admin entry left by a crash before
    // re-adding (the pool also prunes the canonical at startup, but ensure must
    // be self-sufficient for the lazy first-create path).
    git::worktree_prune(canonical).await.ok();
    if let Some(parent) = worktree.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating chat worktree parent {parent:?}"))?;
    }
    git::worktree_add(canonical, worktree)
        .await
        .with_context(|| format!("creating chat worktree at {worktree:?}"))?;
    Ok(worktree.to_path_buf())
}

/// The deterministic branch a chat session works on, namespaced per runner so
/// runners sharing a repo never collide on push (design §3.1.1), and derivable
/// purely from ids the runner already has — which is what makes "revive" free:
/// the same session id maps to the same branch, so resuming just checks it out.
pub fn session_branch(runner_id: uuid::Uuid, chat_session_id: uuid::Uuid) -> String {
    format!("chat/{runner_id}/{chat_session_id}")
}

/// Phase 2: start (or resume) the session's branch in the dedicated worktree.
/// Fresh sessions branch from the repo's default branch; a returning session id
/// resumes its pushed branch. Returns the branch name.
pub async fn start_session(
    canonical: &Path,
    worktree: &Path,
    runner_id: uuid::Uuid,
    chat_session_id: uuid::Uuid,
) -> Result<String> {
    let default = git::default_branch(canonical)
        .await
        .unwrap_or_else(|_| "main".to_string());
    let branch = session_branch(runner_id, chat_session_id);
    git::start_chat_session(worktree, &branch, &default, chat_session_id)
        .await
        .with_context(|| format!("starting chat session branch {branch}"))?;
    Ok(branch)
}

/// Phase 2: persist the session's work on its branch (commit + push) so nothing
/// is left only on the dev machine. No-op when the tree is clean. Returns
/// whether a commit was made.
pub async fn end_session(worktree: &Path) -> Result<bool> {
    git::commit_and_push_all(worktree, "chat(pidash): persist chat session changes").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A canonical clone with a real bare `origin` to push to and `origin/HEAD`
    /// set to `main` — what a pooled runner's canonical looks like.
    fn canonical_with_origin() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "--bare", "-q", "-b", "main"]);
        let canonical = tmp.path().join("canonical");
        git(
            tmp.path(),
            &["clone", "-q", origin.to_str().unwrap(), "canonical"],
        );
        git(&canonical, &["config", "user.email", "t@t.io"]);
        git(&canonical, &["config", "user.name", "t"]);
        git(&canonical, &["config", "commit.gpgsign", "false"]);
        std::fs::write(canonical.join("README.md"), "hello\n").unwrap();
        git(&canonical, &["add", "-A"]);
        git(&canonical, &["commit", "-q", "-m", "init"]);
        git(&canonical, &["push", "-q", "origin", "main"]);
        git(&canonical, &["remote", "set-head", "origin", "main"]);
        (tmp, canonical)
    }

    /// A canonical clone with one commit on `main` — what `PoolHandle::canonical`
    /// points at.
    fn canonical_repo() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("canonical");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t.io"]);
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("README.md"), "hello\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        (tmp, repo)
    }

    #[tokio::test]
    async fn ensure_creates_worktree_lazily_from_canonical() {
        let (tmp, canonical) = canonical_repo();
        let wt = tmp.path().join("chat-worktree");
        assert!(!wt.exists(), "no worktree until first use (lazy)");

        let p = ensure(&canonical, &wt).await.unwrap();
        assert_eq!(p, wt);
        assert!(
            wt.join(".git").exists(),
            "created as a linked git worktree"
        );
        assert!(
            wt.join("README.md").exists(),
            "checked out from the canonical HEAD"
        );
        // It is a SEPARATE tree from the canonical (so it can't collide with
        // issue desks or the canonical clone).
        assert_ne!(wt, canonical);
    }

    #[tokio::test]
    async fn ensure_reuses_healthy_worktree_and_preserves_dirty_state() {
        // Terminal-style persistence: a second chat session resolves the SAME
        // worktree and must keep the operator's uncommitted work (§3.7).
        let (tmp, canonical) = canonical_repo();
        let wt = tmp.path().join("chat-worktree");
        ensure(&canonical, &wt).await.unwrap();

        std::fs::write(wt.join("scratch.txt"), "wip\n").unwrap(); // untracked
        std::fs::write(wt.join("README.md"), "edited\n").unwrap(); // tracked edit

        let p = ensure(&canonical, &wt).await.unwrap();
        assert_eq!(p, wt);
        assert!(
            wt.join("scratch.txt").exists(),
            "dirty untracked file preserved across sessions"
        );
        assert_eq!(
            std::fs::read_to_string(wt.join("README.md")).unwrap(),
            "edited\n",
            "dirty tracked edit preserved (no reset/clean)"
        );
    }

    #[tokio::test]
    async fn ensure_recreates_broken_worktree() {
        // A crash mid-create can leave a dir that isn't a valid worktree. ensure
        // must remove and recreate it (there is no usable state to preserve).
        let (tmp, canonical) = canonical_repo();
        let wt = tmp.path().join("chat-worktree");
        ensure(&canonical, &wt).await.unwrap();

        // Break it: drop the `.git` link so it is no longer a git work tree.
        let _ = std::fs::remove_file(wt.join(".git"));
        assert!(!git::is_git_repo(&wt), "now broken");

        let p = ensure(&canonical, &wt).await.unwrap();
        assert_eq!(p, wt);
        assert!(git::is_git_repo(&wt), "recreated as a healthy worktree");
        assert!(wt.join("README.md").exists());
    }

    // ---- Phase 2: session branch lifecycle ----

    #[tokio::test]
    async fn session_start_uses_per_runner_namespaced_branch() {
        let (tmp, canonical) = canonical_with_origin();
        let wt = tmp.path().join("chat-worktree");
        ensure(&canonical, &wt).await.unwrap();
        let rid = Uuid::new_v4();
        let sid = Uuid::new_v4();

        let branch = start_session(&canonical, &wt, rid, sid).await.unwrap();
        assert_eq!(branch, format!("chat/{rid}/{sid}"));
        // The worktree is now on that branch.
        let cur = git::current_branch(&wt).await.unwrap();
        assert_eq!(cur.as_deref(), Some(branch.as_str()));
    }

    #[tokio::test]
    async fn session_end_commits_and_pushes_then_revive_resumes() {
        let (tmp, canonical) = canonical_with_origin();
        let rid = Uuid::new_v4();
        let sid = Uuid::new_v4();

        // Session 1 in worktree A: do some work, then end (commit + push).
        let wt_a = tmp.path().join("chat-a");
        ensure(&canonical, &wt_a).await.unwrap();
        let branch = start_session(&canonical, &wt_a, rid, sid).await.unwrap();
        std::fs::write(wt_a.join("note.md"), "work from chat\n").unwrap();
        assert!(
            end_session(&wt_a).await.unwrap(),
            "a dirty session commits + pushes"
        );

        // The branch is now on origin.
        let ls = Command::new("git")
            .current_dir(&wt_a)
            .args(["ls-remote", "origin", &format!("refs/heads/{branch}")])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&ls.stdout).trim().is_empty(),
            "session branch was pushed to origin"
        );

        // Revive in a FRESH worktree B with the same ids → resumes the pushed
        // branch with its work intact (the deterministic-branch revive, §3.7).
        let wt_b = tmp.path().join("chat-b");
        ensure(&canonical, &wt_b).await.unwrap();
        let branch_b = start_session(&canonical, &wt_b, rid, sid).await.unwrap();
        assert_eq!(branch_b, branch);
        assert!(
            wt_b.join("note.md").exists(),
            "revive resumes the pushed session branch with its work"
        );
        assert_eq!(
            std::fs::read_to_string(wt_b.join("note.md")).unwrap(),
            "work from chat\n"
        );
    }

    #[tokio::test]
    async fn session_end_is_noop_on_clean_tree() {
        let (tmp, canonical) = canonical_with_origin();
        let wt = tmp.path().join("chat-worktree");
        ensure(&canonical, &wt).await.unwrap();
        start_session(&canonical, &wt, Uuid::new_v4(), Uuid::new_v4())
            .await
            .unwrap();
        assert!(
            !end_session(&wt).await.unwrap(),
            "a read-only session persists nothing"
        );
    }
}
