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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

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
}
