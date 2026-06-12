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
