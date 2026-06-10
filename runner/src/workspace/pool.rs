//! Worktree pool — one canonical git clone per work dir plus a bounded pool of
//! git worktrees ("desks") that runs and chat sessions lease to execute in.
//!
//! Decouples runners from working directories: N runners can reference one
//! work dir, and the pool bounds how many of them execute concurrently. When
//! all desks are taken, lease requests **wait in a FIFO queue** instead of
//! failing. See `.ai_design/worktree_pooling/design.md`.
//!
//! ## Shape
//!
//! The pool is an **actor**: a single owner task ([`pool_owner`]) holds all
//! lease/queue/worktree state, and callers talk to it over an mpsc channel via
//! a cloneable [`PoolHandle`]. Serializing every mutation through one task is
//! what makes "two runs race for the last desk" a non-race (design §9) — there
//! is exactly one decision-maker.
//!
//! A granted lease is a [`Lease`] holding the worktree path. Dropping it
//! enqueues a release back to the owner (salvage → clean → park → unlock →
//! wake the next waiter), so callers cannot forget to return a desk even on an
//! early `return` or panic-unwind.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::config::schema::{CleanMode, WorkdirConfig};
use crate::workspace::git;

/// What a lease is being held for. Recorded in the on-disk lock file so a
/// crash-reap can describe what it salvaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseKind {
    /// A delegated agent run.
    Run,
    /// An interactive chat session (held for the whole session).
    Session,
}

/// How a run ended — drives whether the worktree is salvaged before cleaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseOutcome {
    /// Clean success — no salvage, just clean and return.
    Success,
    /// Failed / cancelled / crashed — salvage the dirty tree first so the
    /// working state survives as a WIP commit (design §4.4 step 1).
    Aborted,
}

/// On-disk lock file contents. Written before checkout, deleted after cleaning.
/// On daemon start every lock file is stale by definition (no lease survives
/// the process), so each is reaped through the full release path (design §8).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockFile {
    kind: LeaseKind,
    holder_id: Uuid,
    branch: Option<String>,
}

/// Why an acquire failed. `PoolUnhealthy` is terminal for the work dir until
/// an operator clears it; `Cancelled` means the waiter was dequeued.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AcquireError {
    #[error("worktree pool for {workdir:?} is unhealthy: {reason}")]
    PoolUnhealthy { workdir: String, reason: String },
    #[error("lease request was cancelled before a worktree was granted")]
    Cancelled,
    #[error("pool owner task is gone")]
    PoolGone,
}

/// A request to lease a desk.
#[derive(Debug)]
pub struct LeaseRequest {
    pub kind: LeaseKind,
    /// The run/session id — used for the lock file and salvage label.
    pub holder_id: Uuid,
    /// The branch this run wants checked out, if it pins one. `None` means the
    /// run will create/choose its own branch (no branch lock to contend on at
    /// lease time).
    pub branch: Option<String>,
}

/// A granted desk. Holds the worktree path; releasing happens on drop.
pub struct Lease {
    worktree: PathBuf,
    /// `Success` by default would be wrong — a dropped-without-marking lease is
    /// an abnormal exit and should salvage. Defaults to `Aborted`; the happy
    /// path calls [`Lease::mark_success`].
    outcome: LeaseOutcome,
    branch: Option<String>,
    holder_id: Uuid,
    worktree_id: usize,
    release_tx: Option<mpsc::UnboundedSender<OwnerMsg>>,
}

impl Lease {
    /// The worktree path the agent should run in.
    pub fn path(&self) -> &Path {
        &self.worktree
    }

    /// Mark the run as a clean success so release skips salvage.
    pub fn mark_success(&mut self) {
        self.outcome = LeaseOutcome::Success;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(tx) = self.release_tx.take() {
            // Non-blocking: the owner task does the async salvage/clean. If the
            // owner is gone (daemon shutting down) the send fails silently —
            // the worktree's lock file is left on disk and reaped on next start.
            let _ = tx.send(OwnerMsg::Release {
                worktree_id: self.worktree_id,
                outcome: self.outcome,
                branch: self.branch.take(),
                holder_id: self.holder_id,
            });
        }
    }
}

/// Health of a work dir's pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolHealth {
    Healthy,
    /// `setup_command` failed after retry, or the canonical clone is
    /// unusable. New leases fail fast until cleared (design §9).
    Unhealthy(String),
}

/// A point-in-time view of the pool for `pidash status` / IPC (design §10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolSnapshot {
    pub workdir_name: String,
    pub pool_size: usize,
    /// Worktrees currently leased (busy desks).
    pub busy: usize,
    /// Worktrees materialized so far (≤ pool_size).
    pub live: usize,
    /// Lease holders, one per busy desk.
    pub holders: Vec<Uuid>,
    /// Run/session ids waiting in the queue.
    pub queue: Vec<Uuid>,
    pub healthy: bool,
    pub unhealthy_reason: Option<String>,
}

impl PoolSnapshot {
    /// Free desks the matcher can use as a capacity hint (design §6.4). Counts
    /// desks that could be granted right now: unmaterialized capacity plus
    /// idle materialized worktrees.
    pub fn free_worktrees(&self) -> u32 {
        (self.pool_size.saturating_sub(self.busy)) as u32
    }
}

// ---------------------------------------------------------------------------
// Owner-task message protocol
// ---------------------------------------------------------------------------

enum OwnerMsg {
    Acquire {
        req: LeaseRequest,
        reply: oneshot::Sender<Result<Lease, AcquireError>>,
    },
    Release {
        worktree_id: usize,
        outcome: LeaseOutcome,
        branch: Option<String>,
        holder_id: Uuid,
    },
    /// Remove a queued waiter (cancel-while-parked, design §5).
    Cancel {
        holder_id: Uuid,
    },
    Snapshot {
        reply: oneshot::Sender<PoolSnapshot>,
    },
    /// Clear an `Unhealthy` state and allow leases again (operator retry).
    ClearUnhealthy,
}

/// Cloneable handle to a work dir's pool. Cheap to clone (wraps an mpsc sender).
#[derive(Clone)]
pub struct PoolHandle {
    tx: mpsc::UnboundedSender<OwnerMsg>,
    workdir_name: Arc<str>,
}

impl PoolHandle {
    /// Request a desk. Resolves when one is granted, the request is cancelled,
    /// or the pool is unhealthy. May wait arbitrarily long if all desks are
    /// busy — that wait IS the local queue (design §5).
    pub async fn acquire(&self, req: LeaseRequest) -> Result<Lease, AcquireError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(OwnerMsg::Acquire { req, reply })
            .map_err(|_| AcquireError::PoolGone)?;
        rx.await.map_err(|_| AcquireError::PoolGone)?
    }

    /// Remove a queued waiter by holder id (no-op if it already got a desk).
    pub fn cancel(&self, holder_id: Uuid) {
        let _ = self.tx.send(OwnerMsg::Cancel { holder_id });
    }

    pub async fn snapshot(&self) -> Option<PoolSnapshot> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(OwnerMsg::Snapshot { reply }).ok()?;
        rx.await.ok()
    }

    pub fn clear_unhealthy(&self) {
        let _ = self.tx.send(OwnerMsg::ClearUnhealthy);
    }

    pub fn workdir_name(&self) -> &str {
        &self.workdir_name
    }
}

// ---------------------------------------------------------------------------
// Owner-task internal state
// ---------------------------------------------------------------------------

/// A materialized worktree desk.
struct Desk {
    id: usize,
    path: PathBuf,
    /// `Some(holder_id)` when leased; `None` when idle in the pool.
    leased_to: Option<Uuid>,
}

struct Waiter {
    req: LeaseRequest,
    reply: oneshot::Sender<Result<Lease, AcquireError>>,
}

struct PoolState {
    cfg: WorkdirConfig,
    /// Where worktrees live (resolved: `worktrees_dir` or `data/worktrees/<n>`).
    pool_dir: PathBuf,
    /// Canonical clone path (object DB source). Agents never run here.
    canonical: PathBuf,
    desks: Vec<Desk>,
    queue: VecDeque<Waiter>,
    health: PoolHealth,
    /// Monotonic counter for desk ids / directory names.
    next_id: usize,
    /// Handle back to ourselves so granted leases can enqueue their release.
    self_tx: mpsc::UnboundedSender<OwnerMsg>,
}

impl PoolState {
    fn lock_path(&self, id: usize) -> PathBuf {
        self.pool_dir.join("locks").join(format!("wt-{id}.lock.json"))
    }

    fn desk_path(&self, id: usize) -> PathBuf {
        self.pool_dir.join(format!("wt-{id}"))
    }

    /// Try to grant a desk to `req` right now. Returns the lease on success, or
    /// gives the request back (still ungranted) so the caller can queue it.
    /// `Err` carries a hard failure (unhealthy) to report to the waiter.
    async fn try_grant(&mut self, req: LeaseRequest) -> TryGrant {
        if let PoolHealth::Unhealthy(reason) = &self.health {
            return TryGrant::Fail(AcquireError::PoolUnhealthy {
                workdir: self.cfg.name.clone(),
                reason: reason.clone(),
            });
        }

        // Branch-lock check: if this run pins a branch already checked out
        // elsewhere, it can't start regardless of free desks (design §4.3).
        if let Some(branch) = req.branch.as_deref()
            && self.branch_locked_for_grant(branch).await
        {
            return TryGrant::Wait(req);
        }

        // Find a free materialized desk, else lazily create one under the cap.
        let desk_id = if let Some(d) = self.desks.iter().find(|d| d.leased_to.is_none()) {
            d.id
        } else if self.desks.len() < self.cfg.pool_size {
            match self.materialize_desk().await {
                Ok(id) => id,
                Err(e) => {
                    // setup failed → mark unhealthy and fail this request.
                    self.mark_unhealthy(format!("worktree setup failed: {e:#}"));
                    return TryGrant::Fail(AcquireError::PoolUnhealthy {
                        workdir: self.cfg.name.clone(),
                        reason: format!("worktree setup failed: {e:#}"),
                    });
                }
            }
        } else {
            // All desks busy — queue it.
            return TryGrant::Wait(req);
        };

        match self.check_out_for(desk_id, &req).await {
            Ok(()) => {
                let desk = self.desks.iter_mut().find(|d| d.id == desk_id).unwrap();
                desk.leased_to = Some(req.holder_id);
                let lease = Lease {
                    worktree: desk.path.clone(),
                    outcome: LeaseOutcome::Aborted,
                    branch: req.branch.clone(),
                    holder_id: req.holder_id,
                    worktree_id: desk_id,
                    release_tx: Some(self.self_tx.clone()),
                };
                TryGrant::Granted(lease)
            }
            Err(e) => {
                tracing::warn!(workdir = %self.cfg.name, desk_id, error = %e, "checkout failed; queueing");
                TryGrant::Wait(req)
            }
        }
    }

    /// Branch-lock check used when deciding whether to grant. Separate method so
    /// the borrow on `self` is released before mutation.
    async fn branch_locked_for_grant(&self, branch: &str) -> bool {
        match git::checked_out_branches(&self.canonical).await {
            Ok(map) => {
                if let Some(holder_path) = map.get(branch) {
                    // If the holder is an idle desk of ours we could reuse it,
                    // but to keep the logic simple we treat any other holder as
                    // a lock. (An idle desk parks on detached HEAD, so it never
                    // holds a branch — only busy desks / the canonical clone do.)
                    let _ = holder_path;
                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    /// Write the lock file and check out the run's branch in the desk.
    async fn check_out_for(&self, desk_id: usize, req: &LeaseRequest) -> anyhow::Result<()> {
        let lock = LockFile {
            kind: req.kind,
            holder_id: req.holder_id,
            branch: req.branch.clone(),
        };
        let lock_path = self.lock_path(desk_id);
        if let Some(parent) = lock_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&lock_path, serde_json::to_vec_pretty(&lock)?).await?;

        if let Some(branch) = req.branch.as_deref().filter(|s| !s.is_empty()) {
            let path = self.desk_path(desk_id);
            git::checkout_work_branch(&path, branch).await?;
        }
        Ok(())
    }

    /// Create a new worktree desk and run `setup_command` (once). One retry
    /// with a short backoff before giving up (design §9).
    async fn materialize_desk(&mut self) -> anyhow::Result<usize> {
        let id = self.next_id;
        self.next_id += 1;
        let path = self.desk_path(id);
        git::worktree_add(&self.canonical, &path).await?;

        if let Some(cmd) = self.cfg.setup_command.clone().filter(|c| !c.trim().is_empty()) {
            let mut last_err = None;
            for attempt in 0..2 {
                match run_setup_command(&path, &cmd).await {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(workdir = %self.cfg.name, attempt, error = %e, "setup_command failed");
                        last_err = Some(e);
                        // Short backoff before the single retry. Bounded so a
                        // broken work dir is declared unhealthy promptly rather
                        // than queueing forever.
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
            if let Some(e) = last_err {
                // Tear the half-set-up desk back down so it isn't reused dirty.
                let _ = git::worktree_remove(&self.canonical, &path, true).await;
                let _ = git::worktree_prune(&self.canonical).await;
                self.next_id = self.next_id.saturating_sub(1);
                return Err(e);
            }
        }

        self.desks.push(Desk {
            id,
            path,
            leased_to: None,
        });
        Ok(id)
    }

    fn mark_unhealthy(&mut self, reason: String) {
        tracing::error!(workdir = %self.cfg.name, %reason, "work dir marked unhealthy");
        self.health = PoolHealth::Unhealthy(reason.clone());
        // Fail every queued waiter fast — a broken work dir must not present as
        // an eternal queue (design §9).
        while let Some(w) = self.queue.pop_front() {
            let _ = w.reply.send(Err(AcquireError::PoolUnhealthy {
                workdir: self.cfg.name.clone(),
                reason: reason.clone(),
            }));
        }
    }

    /// Release a desk: salvage (if aborted + dirty) → clean → park → unlock,
    /// then wake the next grantable waiter (design §4.4, §5).
    async fn release(
        &mut self,
        worktree_id: usize,
        outcome: LeaseOutcome,
        branch: Option<String>,
        holder_id: Uuid,
    ) {
        let Some(desk) = self.desks.iter().find(|d| d.id == worktree_id) else {
            return;
        };
        let path = desk.path.clone();

        // 1. Salvage on abnormal exit, if a branch is known and tree is dirty.
        if outcome == LeaseOutcome::Aborted
            && let Some(branch) = branch.as_deref().filter(|s| !s.is_empty())
        {
            let label = format!("aborted run {holder_id}");
            match git::salvage_wip(&path, branch, &label).await {
                Ok(Some(sha)) => {
                    tracing::info!(workdir = %self.cfg.name, %branch, sha, "salvaged WIP before recycle")
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(workdir = %self.cfg.name, %branch, error = %e, "salvage failed; continuing recycle")
                }
            }
        }

        // 2. Clean. 3. Park on detached HEAD. A failure here means the desk is
        // suspect — drop it and let the pool recreate lazily.
        let clean_ok = git::reset_clean(&path, self.cfg.clean_mode, &self.cfg.keep_paths)
            .await
            .and(git::detach_head(&path).await);
        if let Err(e) = clean_ok {
            tracing::warn!(workdir = %self.cfg.name, desk_id = worktree_id, error = %e, "clean/park failed; reclaiming desk");
            let _ = git::worktree_remove(&self.canonical, &path, true).await;
            let _ = git::worktree_prune(&self.canonical).await;
            self.desks.retain(|d| d.id != worktree_id);
        } else {
            // Re-run setup after a `full` clean so the next lease is warm.
            if self.cfg.clean_mode == CleanMode::Full
                && let Some(cmd) = self.cfg.setup_command.clone().filter(|c| !c.trim().is_empty())
                && let Err(e) = run_setup_command(&path, &cmd).await
            {
                tracing::warn!(workdir = %self.cfg.name, error = %e, "post-full-clean setup failed");
            }
            if let Some(desk) = self.desks.iter_mut().find(|d| d.id == worktree_id) {
                desk.leased_to = None;
            }
        }

        // 4. Unlock.
        let _ = tokio::fs::remove_file(self.lock_path(worktree_id)).await;

        // 5. If pool_size shrank below the live count, prune surplus idle desks.
        self.trim_surplus_desks().await;

        // 6. Wake the queue — grant to the first waiter that can start.
        self.drain_queue().await;
    }

    /// Remove idle desks beyond `pool_size` (e.g. after a TUI shrink, design §9).
    async fn trim_surplus_desks(&mut self) {
        while self.desks.iter().filter(|d| d.leased_to.is_none()).count() > 0
            && self.desks.len() > self.cfg.pool_size
        {
            if let Some(pos) = self.desks.iter().position(|d| d.leased_to.is_none()) {
                let desk = self.desks.remove(pos);
                let _ = git::worktree_remove(&self.canonical, &desk.path, true).await;
                let _ = git::worktree_prune(&self.canonical).await;
                let _ = tokio::fs::remove_file(self.lock_path(desk.id)).await;
            } else {
                break;
            }
        }
    }

    /// FIFO with head-of-line bypass for branch locks only (design §5): a
    /// waiter blocked solely by a branch lock is skipped; the next grantable
    /// waiter starts. Capacity contention never reorders.
    async fn drain_queue(&mut self) {
        let mut skipped: VecDeque<Waiter> = VecDeque::new();
        while let Some(waiter) = self.queue.pop_front() {
            // Stop if no desk could possibly be granted (all busy and at cap).
            let any_free = self.desks.iter().any(|d| d.leased_to.is_none())
                || self.desks.len() < self.cfg.pool_size;
            if !any_free {
                self.queue.push_front(waiter);
                break;
            }
            match self.try_grant(waiter.req).await {
                TryGrant::Granted(lease) => {
                    let _ = waiter.reply.send(Ok(lease));
                }
                TryGrant::Wait(req) => {
                    // Branch-locked: keep it queued (bypass), try the next one.
                    skipped.push_back(Waiter {
                        req,
                        reply: waiter.reply,
                    });
                }
                TryGrant::Fail(e) => {
                    let _ = waiter.reply.send(Err(e));
                }
            }
        }
        // Re-queue skipped (branch-locked) waiters at the front, preserving order.
        while let Some(w) = skipped.pop_back() {
            self.queue.push_front(w);
        }
    }

    fn snapshot(&self) -> PoolSnapshot {
        let holders: Vec<Uuid> = self.desks.iter().filter_map(|d| d.leased_to).collect();
        let queue: Vec<Uuid> = self.queue.iter().map(|w| w.req.holder_id).collect();
        let (healthy, reason) = match &self.health {
            PoolHealth::Healthy => (true, None),
            PoolHealth::Unhealthy(r) => (false, Some(r.clone())),
        };
        PoolSnapshot {
            workdir_name: self.cfg.name.clone(),
            pool_size: self.cfg.pool_size,
            busy: self.desks.iter().filter(|d| d.leased_to.is_some()).count(),
            live: self.desks.len(),
            holders,
            queue,
            healthy,
            unhealthy_reason: reason,
        }
    }
}

enum TryGrant {
    Granted(Lease),
    Wait(LeaseRequest),
    Fail(AcquireError),
}

/// Run a work dir's `setup_command` in `cwd` via the system shell.
async fn run_setup_command(cwd: &Path, cmd: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let out = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output()
        .await
        .context("spawning setup_command")?;
    if !out.status.success() {
        anyhow::bail!(
            "setup_command `{cmd}` exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Owner task + construction
// ---------------------------------------------------------------------------

/// Build a pool for `cfg`, run its init (resolve canonical clone, prune, reap
/// stale locks), spawn the owner task, and return a handle. `data_worktrees`
/// is the daemon's default worktrees base (`data_dir/worktrees`); the work
/// dir's `worktrees_dir` override wins when set.
pub async fn spawn(cfg: WorkdirConfig, data_worktrees: &Path) -> anyhow::Result<PoolHandle> {
    let pool_dir = cfg
        .worktrees_dir
        .clone()
        .unwrap_or_else(|| data_worktrees.join(&cfg.name));
    let canonical = cfg.path.clone();

    let (tx, rx) = mpsc::unbounded_channel();
    let workdir_name: Arc<str> = Arc::from(cfg.name.as_str());

    let mut state = PoolState {
        cfg,
        pool_dir,
        canonical,
        desks: Vec::new(),
        queue: VecDeque::new(),
        health: PoolHealth::Healthy,
        next_id: 1,
        self_tx: tx.clone(),
    };

    // Initialization (design §4.2 / §8): ensure dirs, resolve canonical clone,
    // prune dead worktree bookkeeping, reap stale locks.
    if let Err(e) = init_pool(&mut state).await {
        state.mark_unhealthy(format!("pool init failed: {e:#}"));
    }

    tokio::spawn(pool_owner(state, rx));

    Ok(PoolHandle { tx, workdir_name })
}

async fn init_pool(state: &mut PoolState) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&state.pool_dir).await?;
    tokio::fs::create_dir_all(state.pool_dir.join("locks")).await?;

    // The canonical clone must exist as a git repo. We do NOT clone here from a
    // URL — the operator (or `pidash workdir add`) provisions it; an empty or
    // missing path is a config error surfaced as unhealthy.
    if !git::is_git_repo(&state.canonical) {
        anyhow::bail!(
            "canonical clone {:?} is not a git repo (provision it before adding runners)",
            state.canonical
        );
    }
    // Disable auto-gc on the shared object DB so a background gc can't race
    // concurrent worktree operations.
    let _ = git::set_gc_auto_off(&state.canonical).await;

    git::worktree_prune(&state.canonical).await.ok();
    reap_stale_locks(state).await;
    Ok(())
}

/// On start, every lock file is stale (no lease survives the process). Each
/// locked worktree is reaped through salvage → clean → park → unlock so a
/// crash can't strand a desk or lose a dirty tree (design §8).
async fn reap_stale_locks(state: &mut PoolState) {
    let locks_dir = state.pool_dir.join("locks");
    let mut entries = match tokio::fs::read_dir(&locks_dir).await {
        Ok(e) => e,
        Err(_) => return,
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // Parse `wt-<id>.lock.json` for the desk id.
        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("wt-"))
            .and_then(|n| n.strip_suffix(".lock.json"))
            .and_then(|n| n.parse::<usize>().ok());
        let Some(id) = id else { continue };
        let lock: Option<LockFile> = tokio::fs::read(&path)
            .await
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());

        let desk_path = state.desk_path(id);
        if desk_path.exists() {
            // Salvage the pre-crash tree to its branch, then clean and park.
            if let Some(lock) = &lock
                && let Some(branch) = lock.branch.as_deref().filter(|s| !s.is_empty())
            {
                let label = format!("crash-reaped run {}", lock.holder_id);
                let _ = git::salvage_wip(&desk_path, branch, &label).await;
            }
            let _ = git::reset_clean(&desk_path, state.cfg.clean_mode, &state.cfg.keep_paths).await;
            let _ = git::detach_head(&desk_path).await;
            // Re-register the reaped desk as an idle pool member so it's reused.
            state.desks.push(Desk {
                id,
                path: desk_path,
                leased_to: None,
            });
            state.next_id = state.next_id.max(id + 1);
        }
        let _ = tokio::fs::remove_file(&path).await;
    }
}

async fn pool_owner(mut state: PoolState, mut rx: mpsc::UnboundedReceiver<OwnerMsg>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            OwnerMsg::Acquire { req, reply } => match state.try_grant(req).await {
                TryGrant::Granted(lease) => {
                    let _ = reply.send(Ok(lease));
                }
                TryGrant::Wait(req) => {
                    state.queue.push_back(Waiter { req, reply });
                }
                TryGrant::Fail(e) => {
                    let _ = reply.send(Err(e));
                }
            },
            OwnerMsg::Release {
                worktree_id,
                outcome,
                branch,
                holder_id,
            } => {
                state.release(worktree_id, outcome, branch, holder_id).await;
            }
            OwnerMsg::Cancel { holder_id } => {
                if let Some(pos) = state.queue.iter().position(|w| w.req.holder_id == holder_id) {
                    let waiter = state.queue.remove(pos).unwrap();
                    let _ = waiter.reply.send(Err(AcquireError::Cancelled));
                }
            }
            OwnerMsg::Snapshot { reply } => {
                let _ = reply.send(state.snapshot());
            }
            OwnerMsg::ClearUnhealthy => {
                if matches!(state.health, PoolHealth::Unhealthy(_)) {
                    tracing::info!(workdir = %state.cfg.name, "clearing unhealthy state");
                    state.health = PoolHealth::Healthy;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    include!("pool_tests.rs");
}
