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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::config::schema::{CleanMode, WorkdirConfig};
use crate::workspace::git;

/// How many times a waiter's branch checkout may fail before the lease fails
/// hard (a typo'd or unfetchable branch must not present as an eternal queue,
/// design §9).
const MAX_CHECKOUT_ATTEMPTS: u32 = 3;
/// Delay before re-draining the queue after a checkout failure. Without this,
/// a transient fetch error on an otherwise idle pool would strand the waiter
/// forever (the queue is otherwise only re-evaluated on lease release).
const RETRY_DRAIN_DELAY: std::time::Duration = std::time::Duration::from_secs(3);
/// Hard ceiling on `setup_command` runtime. The owner task runs setup inline,
/// so a hung command would otherwise freeze every acquire/cancel/snapshot for
/// the work dir indefinitely.
const SETUP_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

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
    /// When the lease was granted — ops-debugging context for `pidash status`
    /// and crash-reap logs (design §8).
    #[serde(default)]
    acquired_at: Option<DateTime<Utc>>,
}

/// Why an acquire failed. `PoolUnhealthy` is terminal for the work dir until
/// an operator clears it; `Cancelled` means the waiter was dequeued.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AcquireError {
    #[error("worktree pool for {workdir:?} is unhealthy: {reason}")]
    PoolUnhealthy { workdir: String, reason: String },
    #[error("lease request was cancelled before a worktree was granted")]
    Cancelled,
    #[error("worktree checkout failed after {MAX_CHECKOUT_ATTEMPTS} attempts: {detail}")]
    Checkout { detail: String },
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
    /// When set, the owner task reports this waiter's queue position here:
    /// once when it parks and again whenever the position changes (positions
    /// only decrease). This is the authoritative "the run is actually waiting"
    /// signal feeding `RunQueued` / `WAITING_FOR_WORKTREE` (design §6.1) —
    /// unlike a pre-acquire snapshot, it cannot race the enqueue and it also
    /// covers branch-lock waits with free desks. The sender is dropped when
    /// the waiter is granted, cancelled, or failed.
    pub queued_tx: Option<mpsc::UnboundedSender<u32>>,
}

/// A granted desk. Holds the worktree path; releasing happens on drop.
pub struct Lease {
    worktree: PathBuf,
    /// `Success` by default would be wrong — a dropped-without-marking lease is
    /// an abnormal exit and should salvage. Defaults to `Aborted`; the happy
    /// path calls [`Lease::mark_success`].
    outcome: LeaseOutcome,
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
    /// idle materialized worktrees. An unhealthy pool advertises zero — every
    /// lease against it fails fast, so steering the matcher toward it would
    /// invert the hint exactly when it matters.
    pub fn free_worktrees(&self) -> u32 {
        if !self.healthy {
            return 0;
        }
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
        holder_id: Uuid,
    },
    /// Remove a queued waiter (cancel-while-parked, design §5).
    Cancel {
        holder_id: Uuid,
    },
    Snapshot {
        reply: oneshot::Sender<PoolSnapshot>,
    },
    /// Re-evaluate the queue. Scheduled (delayed) after a checkout failure so
    /// a waiter parked by a transient git error is retried even when no lease
    /// release is coming (an idle pool would otherwise strand it forever).
    Drain,
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
    /// Consecutive checkout failures for this waiter. Capacity and branch-lock
    /// waits don't count — only real git errors, bounded by
    /// [`MAX_CHECKOUT_ATTEMPTS`].
    checkout_attempts: u32,
    /// Last queue position reported through `req.queued_tx` (dedupe).
    last_reported_pos: Option<u32>,
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
    /// gives the request back (still ungranted, with the wait cause) so the
    /// caller can queue it. `Err` carries a hard failure (unhealthy) to report
    /// to the waiter.
    async fn try_grant(&mut self, req: LeaseRequest) -> TryGrant {
        if let PoolHealth::Unhealthy(reason) = &self.health {
            return TryGrant::Fail(AcquireError::PoolUnhealthy {
                workdir: self.cfg.name.clone(),
                reason: reason.clone(),
            });
        }

        // Branch-lock check: if this run pins a branch already checked out
        // elsewhere, it can't start regardless of free desks (design §4.3).
        // Fail-closed: an error listing worktrees is treated like a checkout
        // failure (bounded retry), never as "unlocked" — `checkout -B` does
        // not reliably refuse a doubly-held branch, so this check is the lock.
        if let Some(branch) = req.branch.as_deref() {
            match self.branch_locked_for_grant(branch).await {
                Ok(true) => return TryGrant::Wait(req, WaitCause::BranchLocked),
                Ok(false) => {}
                Err(e) => {
                    return TryGrant::Wait(req, WaitCause::CheckoutFailed(format!("{e:#}")));
                }
            }
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
            return TryGrant::Wait(req, WaitCause::Capacity);
        };

        match self.check_out_for(desk_id, &req).await {
            Ok(()) => {
                let desk = self.desks.iter_mut().find(|d| d.id == desk_id).unwrap();
                desk.leased_to = Some(req.holder_id);
                let lease = Lease {
                    worktree: desk.path.clone(),
                    outcome: LeaseOutcome::Aborted,
                    holder_id: req.holder_id,
                    worktree_id: desk_id,
                    release_tx: Some(self.self_tx.clone()),
                };
                TryGrant::Granted(lease)
            }
            Err(e) => {
                tracing::warn!(workdir = %self.cfg.name, desk_id, error = %e, "checkout failed; queueing");
                // The desk stays idle and unleased — remove the lock file
                // written before the failed checkout so a later crash-reap
                // doesn't "salvage" an idle desk under this request's name.
                let _ = tokio::fs::remove_file(self.lock_path(desk_id)).await;
                TryGrant::Wait(req, WaitCause::CheckoutFailed(format!("{e:#}")))
            }
        }
    }

    /// Branch-lock check used when deciding whether to grant. Separate method so
    /// the borrow on `self` is released before mutation. Errors propagate so
    /// the caller can fail closed.
    async fn branch_locked_for_grant(&self, branch: &str) -> anyhow::Result<bool> {
        let map = git::checked_out_branches(&self.canonical).await?;
        // If the holder is an idle desk of ours we could reuse it, but to keep
        // the logic simple we treat any holder as a lock. (An idle desk parks
        // on detached HEAD, so it never holds a branch — only busy desks / the
        // canonical clone do.)
        Ok(map.contains_key(branch))
    }

    /// Write the lock file and check out the run's branch in the desk.
    async fn check_out_for(&self, desk_id: usize, req: &LeaseRequest) -> anyhow::Result<()> {
        let lock = LockFile {
            kind: req.kind,
            holder_id: req.holder_id,
            branch: req.branch.clone(),
            acquired_at: Some(Utc::now()),
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
    /// with a short backoff before giving up (design §9) — for both the
    /// `worktree add` (a transient disk/lock blip must not condemn the whole
    /// pool) and the setup command.
    async fn materialize_desk(&mut self) -> anyhow::Result<usize> {
        let id = self.next_id;
        self.next_id += 1;
        let path = self.desk_path(id);
        if let Err(first) = git::worktree_add(&self.canonical, &path).await {
            // Clear any half-created state (stale dir, dangling bookkeeping)
            // and retry once before declaring the pool broken.
            tracing::warn!(workdir = %self.cfg.name, desk_id = id, error = %first, "worktree add failed; retrying once");
            self.remove_desk_dir(&path).await;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if let Err(e) = git::worktree_add(&self.canonical, &path).await {
                self.next_id = self.next_id.saturating_sub(1);
                return Err(e);
            }
        }

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
                        // than queueing forever. No sleep after the last
                        // attempt — there is nothing left to wait for.
                        if attempt == 0 {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                    }
                }
            }
            if let Some(e) = last_err {
                // Tear the half-set-up desk back down so it isn't reused dirty.
                self.remove_desk_dir(&path).await;
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

    /// Remove a desk directory and its git bookkeeping, tolerating any state:
    /// a registered worktree, an unregistered leftover dir, or nothing at all.
    /// Leaving a `wt-<n>` directory behind would make the next
    /// `git worktree add` of that path fail.
    async fn remove_desk_dir(&self, path: &Path) {
        if git::worktree_remove(&self.canonical, path, true).await.is_err() && path.exists() {
            let _ = tokio::fs::remove_dir_all(path).await;
        }
        let _ = git::worktree_prune(&self.canonical).await;
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
    async fn release(&mut self, worktree_id: usize, outcome: LeaseOutcome, holder_id: Uuid) {
        let Some(desk) = self.desks.iter().find(|d| d.id == worktree_id) else {
            return;
        };
        let path = desk.path.clone();

        // 1. Salvage on abnormal exit. Unconditional — recycling must never
        // destroy a failed run's working state (design §4.4), whether or not
        // the run pinned a branch. `salvage_wip` no-ops on a clean tree and
        // picks the branch HEAD actually sits on (or a salvage branch when
        // detached).
        if outcome == LeaseOutcome::Aborted {
            let label = format!("aborted run {holder_id}");
            match git::salvage_wip(&path, holder_id, &label).await {
                Ok(Some(sha)) => {
                    tracing::info!(workdir = %self.cfg.name, sha, "salvaged WIP before recycle")
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(workdir = %self.cfg.name, error = %e, "salvage failed; continuing recycle")
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
            self.remove_desk_dir(&path).await;
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
                self.remove_desk_dir(&desk.path).await;
                let _ = tokio::fs::remove_file(self.lock_path(desk.id)).await;
            } else {
                break;
            }
        }
    }

    /// FIFO with head-of-line bypass for branch locks only (design §5): a
    /// waiter blocked solely by a branch lock is skipped; the next grantable
    /// waiter starts. Capacity contention never reorders. A checkout failure
    /// counts against the waiter's bounded attempts (a broken branch must not
    /// queue forever); a retry drain is scheduled since no release may come.
    async fn drain_queue(&mut self) {
        let mut skipped: VecDeque<Waiter> = VecDeque::new();
        while let Some(mut waiter) = self.queue.pop_front() {
            // A waiter whose acquire future was dropped (without a cancel) has
            // nobody listening — discard it instead of wasting a checkout and
            // inflating reported queue positions.
            if waiter.reply.is_closed() {
                continue;
            }
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
                TryGrant::Wait(req, cause) => {
                    waiter.req = req;
                    match cause {
                        WaitCause::Capacity | WaitCause::BranchLocked => {
                            // Keep it queued (bypass), try the next one.
                            skipped.push_back(waiter);
                        }
                        WaitCause::CheckoutFailed(detail) => {
                            waiter.checkout_attempts += 1;
                            if waiter.checkout_attempts >= MAX_CHECKOUT_ATTEMPTS {
                                let _ = waiter.reply.send(Err(AcquireError::Checkout { detail }));
                            } else {
                                skipped.push_back(waiter);
                                self.schedule_retry_drain();
                            }
                        }
                    }
                }
                TryGrant::Fail(e) => {
                    let _ = waiter.reply.send(Err(e));
                }
            }
        }
        // If the pool went unhealthy mid-drain (`mark_unhealthy` failed the
        // main queue but cannot see this local deque), fail the skipped
        // waiters too — nothing will ever drain them (design §9 fail-fast).
        if let PoolHealth::Unhealthy(reason) = &self.health {
            for w in skipped.drain(..) {
                let _ = w.reply.send(Err(AcquireError::PoolUnhealthy {
                    workdir: self.cfg.name.clone(),
                    reason: reason.clone(),
                }));
            }
            return;
        }
        // Re-queue skipped (branch-locked) waiters at the front, preserving order.
        while let Some(w) = skipped.pop_back() {
            self.queue.push_front(w);
        }
        self.notify_positions();
    }

    /// Report queue positions to waiters that asked for them (design §6.1).
    /// Sends once on park and again whenever the position changes; deduped per
    /// waiter so a no-op drain stays silent.
    fn notify_positions(&mut self) {
        for (idx, waiter) in self.queue.iter_mut().enumerate() {
            let pos = idx as u32 + 1;
            if waiter.last_reported_pos == Some(pos) {
                continue;
            }
            if let Some(tx) = &waiter.req.queued_tx
                && tx.send(pos).is_ok()
            {
                waiter.last_reported_pos = Some(pos);
            }
        }
    }

    /// Schedule a delayed queue re-evaluation. Used after checkout failures:
    /// the queue is otherwise only drained on lease release, so a transient
    /// git error on an idle pool would strand the waiter forever.
    fn schedule_retry_drain(&self) {
        let tx = self.self_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(RETRY_DRAIN_DELAY).await;
            let _ = tx.send(OwnerMsg::Drain);
        });
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
    Wait(LeaseRequest, WaitCause),
    Fail(AcquireError),
}

/// Why a request could not be granted right now. Capacity and branch-lock
/// waits are normal queueing; a checkout failure is a git error that counts
/// against the waiter's bounded retries.
enum WaitCause {
    Capacity,
    BranchLocked,
    CheckoutFailed(String),
}

/// Run a work dir's `setup_command` in `cwd` via the system shell. Bounded by
/// [`SETUP_COMMAND_TIMEOUT`] — the pool owner runs this inline, so a hung
/// command would otherwise block every acquire/cancel/snapshot for the work
/// dir indefinitely.
async fn run_setup_command(cwd: &Path, cmd: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let out = tokio::time::timeout(
        SETUP_COMMAND_TIMEOUT,
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "setup_command `{cmd}` timed out after {}s",
            SETUP_COMMAND_TIMEOUT.as_secs()
        )
    })?
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
    adopt_existing_desks(state).await;

    // Heads-up (design §4.3): the canonical clone's checked-out branch holds a
    // permanent branch lock — runs pinning it will queue until it's parked.
    if let Ok(Some(branch)) = git::current_branch(&state.canonical).await {
        tracing::info!(
            workdir = %state.cfg.name,
            %branch,
            "canonical clone has this branch checked out; runs pinning it will \
             wait — `git checkout --detach` there to release it"
        );
    }
    Ok(())
}

/// On start, adopt every `wt-<id>` directory left under the pool dir — desks
/// from a clean shutdown (no lock file) as well as crashed leases (stale lock
/// file; no lease survives the process, so every lock is stale by definition).
///
/// Both kinds MUST be adopted or removed: a leftover directory that the pool
/// forgets about makes the next `git worktree add wt-<id>` fail, bricking the
/// pool on its first lease after a restart. Crashed leases are reaped through
/// the full release path (salvage → clean → park) so a crash can't lose a
/// dirty tree (design §8); a desk that fails cleaning is removed, never
/// repaired in place or registered broken.
async fn adopt_existing_desks(state: &mut PoolState) {
    // Collect stale locks first: desk id → lock contents.
    let locks_dir = state.pool_dir.join("locks");
    let mut locks: std::collections::HashMap<usize, LockFile> = std::collections::HashMap::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&locks_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let id = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("wt-"))
                .and_then(|n| n.strip_suffix(".lock.json"))
                .and_then(|n| n.parse::<usize>().ok());
            let Some(id) = id else { continue };
            if let Some(lock) = tokio::fs::read(&path)
                .await
                .ok()
                .and_then(|b| serde_json::from_slice::<LockFile>(&b).ok())
            {
                locks.insert(id, lock);
            }
            let _ = tokio::fs::remove_file(&path).await;
        }
    }

    // Scan for desk directories. The set of dirs (not the set of locks) is
    // the ground truth for what must be adopted or removed.
    let mut desk_ids: Vec<usize> = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(&state.pool_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let id = entry
                .file_name()
                .to_str()
                .and_then(|n| n.strip_prefix("wt-"))
                .and_then(|n| n.parse::<usize>().ok());
            if let Some(id) = id
                && entry.path().is_dir()
            {
                desk_ids.push(id);
            }
        }
    }
    desk_ids.sort_unstable();

    for id in desk_ids {
        let desk_path = state.desk_path(id);
        // Salvage any dirty tree before cleaning — from the lock's holder if
        // we know it, otherwise anonymously. `salvage_wip` no-ops when clean
        // (the normal case for clean-shutdown desks).
        let (holder, label) = match locks.get(&id) {
            Some(lock) => (
                lock.holder_id,
                format!("crash-reaped run {}", lock.holder_id),
            ),
            None => (Uuid::nil(), "desk adopted at startup".to_string()),
        };
        let _ = git::salvage_wip(&desk_path, holder, &label).await;

        let clean_ok = git::reset_clean(&desk_path, state.cfg.clean_mode, &state.cfg.keep_paths)
            .await
            .and(git::detach_head(&desk_path).await);
        match clean_ok {
            Ok(()) => {
                state.desks.push(Desk {
                    id,
                    path: desk_path,
                    leased_to: None,
                });
            }
            Err(e) => {
                // Never register a desk that failed cleaning (design §8) —
                // remove it; the pool recreates capacity lazily.
                tracing::warn!(workdir = %state.cfg.name, desk_id = id, error = %e, "adopted desk failed clean/park; removing");
                state.remove_desk_dir(&desk_path).await;
            }
        }
        state.next_id = state.next_id.max(id + 1);
    }
}

async fn pool_owner(mut state: PoolState, mut rx: mpsc::UnboundedReceiver<OwnerMsg>) {
    while let Some(msg) = rx.recv().await {
        match msg {
            OwnerMsg::Acquire { req, reply } => match state.try_grant(req).await {
                TryGrant::Granted(lease) => {
                    let _ = reply.send(Ok(lease));
                }
                TryGrant::Wait(req, cause) => {
                    let checkout_attempts = match &cause {
                        WaitCause::CheckoutFailed(_) => 1,
                        _ => 0,
                    };
                    if let WaitCause::CheckoutFailed(_) = &cause {
                        state.schedule_retry_drain();
                    }
                    state.queue.push_back(Waiter {
                        req,
                        reply,
                        checkout_attempts,
                        last_reported_pos: None,
                    });
                    // Authoritative park signal: the waiter is now actually
                    // queued, so report its position (design §6.1).
                    state.notify_positions();
                }
                TryGrant::Fail(e) => {
                    let _ = reply.send(Err(e));
                }
            },
            OwnerMsg::Release {
                worktree_id,
                outcome,
                holder_id,
            } => {
                state.release(worktree_id, outcome, holder_id).await;
            }
            OwnerMsg::Cancel { holder_id } => {
                if let Some(pos) = state.queue.iter().position(|w| w.req.holder_id == holder_id) {
                    let waiter = state.queue.remove(pos).unwrap();
                    let _ = waiter.reply.send(Err(AcquireError::Cancelled));
                    // Everyone behind the cancelled waiter moved up one.
                    state.notify_positions();
                }
            }
            OwnerMsg::Snapshot { reply } => {
                let _ = reply.send(state.snapshot());
            }
            OwnerMsg::Drain => {
                state.drain_queue().await;
            }
            OwnerMsg::ClearUnhealthy => {
                if matches!(state.health, PoolHealth::Unhealthy(_)) {
                    tracing::info!(workdir = %state.cfg.name, "clearing unhealthy state");
                    state.health = PoolHealth::Healthy;
                    // Waiters may have queued between the failure and the
                    // clear; without a drain they'd sit until an unrelated
                    // release.
                    state.drain_queue().await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    include!("pool_tests.rs");
}
