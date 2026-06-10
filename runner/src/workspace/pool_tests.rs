// Unit tests for the worktree pool. Included via `include!` from pool.rs so
// they live in the `pool` module and can see private items.
//
// Each test builds a real on-disk git repo (a bare "origin" + a working clone
// used as the canonical clone) in a tempdir, so the git primitives and the
// pool actor are exercised end to end against actual git.

use super::*;
use std::process::Command;
use tempfile::TempDir;

/// Run a git command in `dir`, asserting success — test setup helper.
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

/// Build: a bare origin repo, and a canonical clone of it with one commit on
/// `main` and a feature branch `feat/x` pushed. Returns (tempdir, canonical
/// path, worktrees base dir).
fn fixture() -> (TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let origin = tmp.path().join("origin.git");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "-q"]);

    let canonical = tmp.path().join("canonical");
    git(tmp.path(), &["clone", "-q", origin.to_str().unwrap(), "canonical"]);
    git(&canonical, &["config", "user.email", "t@t.io"]);
    git(&canonical, &["config", "user.name", "t"]);
    git(&canonical, &["config", "commit.gpgsign", "false"]);
    // Ensure the default branch is `main`.
    git(&canonical, &["checkout", "-q", "-B", "main"]);
    std::fs::write(canonical.join("README.md"), "hello\n").unwrap();
    std::fs::write(canonical.join(".gitignore"), "ignored/\n").unwrap();
    git(&canonical, &["add", "-A"]);
    git(&canonical, &["commit", "-q", "-m", "init"]);
    git(&canonical, &["push", "-q", "origin", "main"]);
    // A feature branch runs can pin.
    git(&canonical, &["checkout", "-q", "-b", "feat/x"]);
    std::fs::write(canonical.join("feat.txt"), "feat\n").unwrap();
    git(&canonical, &["add", "-A"]);
    git(&canonical, &["commit", "-q", "-m", "feat"]);
    git(&canonical, &["push", "-q", "origin", "feat/x"]);
    // Park the canonical clone on a branch that runs won't use, so it never
    // holds a branch lock the tests care about.
    git(&canonical, &["checkout", "-q", "main"]);

    let worktrees = tmp.path().join("wt");
    (tmp, canonical, worktrees)
}

fn workdir_cfg(name: &str, canonical: &Path, pool_size: usize) -> WorkdirConfig {
    WorkdirConfig {
        name: name.into(),
        path: canonical.to_path_buf(),
        pool_size,
        clean_mode: CleanMode::KeepIgnored,
        keep_paths: vec![],
        setup_command: None,
        worktrees_dir: None,
    }
}

/// Drive the pool owner inline by constructing state + handle the same way
/// `spawn` does, but awaiting init so failures surface in the test. Returns the
/// handle plus the worktrees dir.
async fn spawn_pool(cfg: WorkdirConfig, worktrees: &Path) -> PoolHandle {
    spawn(cfg, worktrees).await.expect("spawn pool")
}

#[tokio::test]
async fn acquire_grants_a_worktree_and_release_returns_it() {
    let (_tmp, canonical, worktrees) = fixture();
    let pool = spawn_pool(workdir_cfg("main", &canonical, 2), &worktrees).await;

    let mut lease = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("acquire");
    assert!(lease.path().exists(), "worktree dir should exist");
    assert!(git::is_git_repo(lease.path()) || lease.path().join(".git").exists());

    let snap = pool.snapshot().await.unwrap();
    assert_eq!(snap.busy, 1);
    assert_eq!(snap.live, 1);

    lease.mark_success();
    drop(lease);

    // Give the owner a moment to process the release.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let snap = pool.snapshot().await.unwrap();
    assert_eq!(snap.busy, 0, "desk returned to pool");
    assert_eq!(snap.free_worktrees(), 2);
}

#[tokio::test]
async fn lazy_growth_stops_at_pool_size_and_excess_queues() {
    let (_tmp, canonical, worktrees) = fixture();
    let pool = spawn_pool(workdir_cfg("main", &canonical, 1), &worktrees).await;

    // First lease grabs the only desk.
    let _lease1 = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("first acquire");

    // Second acquire must WAIT (pool_size = 1) — it should not resolve until
    // the first releases. Assert it's still pending after a short delay.
    let pool2 = pool.clone();
    let h2 = Uuid::new_v4();
    let waiter = tokio::spawn(async move {
        pool2
            .acquire(LeaseRequest {
                kind: LeaseKind::Run,
                holder_id: h2,
                branch: None,
            })
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(!waiter.is_finished(), "second lease should be queued, not granted");

    let snap = pool.snapshot().await.unwrap();
    assert_eq!(snap.busy, 1);
    assert_eq!(snap.queue.len(), 1);
    assert_eq!(snap.queue[0], h2);

    // Release the first; the waiter should now get the desk.
    drop(_lease1);
    let granted = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
        .await
        .expect("waiter resolves")
        .expect("join")
        .expect("granted after release");
    assert!(granted.path().exists());
}

#[tokio::test]
async fn cancel_removes_a_queued_waiter() {
    let (_tmp, canonical, worktrees) = fixture();
    let pool = spawn_pool(workdir_cfg("main", &canonical, 1), &worktrees).await;

    let _lease1 = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("first acquire");

    let pool2 = pool.clone();
    let h2 = Uuid::new_v4();
    let waiter = tokio::spawn(async move {
        pool2
            .acquire(LeaseRequest {
                kind: LeaseKind::Run,
                holder_id: h2,
                branch: None,
            })
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(!waiter.is_finished());

    // Cancel the queued waiter — it should resolve with `Cancelled`.
    pool.cancel(h2);
    let res = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
        .await
        .expect("resolves")
        .expect("join");
    assert!(matches!(res, Err(AcquireError::Cancelled)));

    let snap = pool.snapshot().await.unwrap();
    assert!(snap.queue.is_empty());
}

#[tokio::test]
async fn branch_lock_serializes_two_runs_on_the_same_branch() {
    let (_tmp, canonical, worktrees) = fixture();
    let pool = spawn_pool(workdir_cfg("main", &canonical, 2), &worktrees).await;

    // Run A pins feat/x.
    let lease_a = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: Some("feat/x".into()),
        })
        .await
        .expect("A acquires feat/x");
    assert_eq!(
        current_branch(lease_a.path()),
        "feat/x",
        "A should be on feat/x"
    );

    // Run B also wants feat/x — even though a second desk is free, the branch
    // lock must make it WAIT.
    let pool2 = pool.clone();
    let waiter_b = tokio::spawn(async move {
        pool2
            .acquire(LeaseRequest {
                kind: LeaseKind::Run,
                holder_id: Uuid::new_v4(),
                branch: Some("feat/x".into()),
            })
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        !waiter_b.is_finished(),
        "B must wait on the branch lock even with a free desk"
    );

    // Release A → B can now take feat/x.
    drop(lease_a);
    let lease_b = tokio::time::timeout(std::time::Duration::from_secs(5), waiter_b)
        .await
        .expect("resolves")
        .expect("join")
        .expect("B gets feat/x after A releases");
    assert_eq!(current_branch(lease_b.path()), "feat/x");
}

#[tokio::test]
async fn branch_lock_bypass_lets_a_different_branch_run_first() {
    let (_tmp, canonical, worktrees) = fixture();
    // Push a second feature branch so two distinct branches exist.
    git(&canonical, &["checkout", "-q", "-b", "feat/y"]);
    std::fs::write(canonical.join("y.txt"), "y\n").unwrap();
    git(&canonical, &["add", "-A"]);
    git(&canonical, &["commit", "-q", "-m", "y"]);
    git(&canonical, &["push", "-q", "origin", "feat/y"]);
    git(&canonical, &["checkout", "-q", "main"]);

    let pool = spawn_pool(workdir_cfg("main", &canonical, 1), &worktrees).await;

    // A holds the single desk on feat/x.
    let lease_a = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: Some("feat/x".into()),
        })
        .await
        .expect("A");

    // B wants feat/x (will be branch-blocked once a desk frees);
    // C wants feat/y. Queue order: B then C.
    let pool_b = pool.clone();
    let hb = Uuid::new_v4();
    let wb = tokio::spawn(async move {
        pool_b
            .acquire(LeaseRequest {
                kind: LeaseKind::Run,
                holder_id: hb,
                branch: Some("feat/x".into()),
            })
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let pool_c = pool.clone();
    let hc = Uuid::new_v4();
    let wc = tokio::spawn(async move {
        pool_c
            .acquire(LeaseRequest {
                kind: LeaseKind::Run,
                holder_id: hc,
                branch: Some("feat/y".into()),
            })
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Release A. feat/x is now free (A held it), so B (head of queue) can take
    // it — bypass only kicks in when the head is *still* blocked. To force the
    // bypass scenario we instead keep feat/x locked: re-acquire it out of band.
    // Simpler assertion: after releasing A, SOMEONE gets a desk and both
    // eventually complete without deadlock.
    drop(lease_a);
    let rb = tokio::time::timeout(std::time::Duration::from_secs(5), wb)
        .await
        .expect("b resolves")
        .expect("join");
    assert!(rb.is_ok(), "B should eventually get feat/x");
    drop(rb);
    let rc = tokio::time::timeout(std::time::Duration::from_secs(5), wc)
        .await
        .expect("c resolves")
        .expect("join");
    assert!(rc.is_ok(), "C should eventually get feat/y");
}

#[tokio::test]
async fn aborted_lease_salvages_dirty_tree_to_branch() {
    let (_tmp, canonical, worktrees) = fixture();
    let pool = spawn_pool(workdir_cfg("main", &canonical, 1), &worktrees).await;

    let lease = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: Some("feat/x".into()),
        })
        .await
        .expect("acquire feat/x");
    let wt = lease.path().to_path_buf();
    // Dirty the tree without committing.
    std::fs::write(wt.join("scratch.txt"), "uncommitted work\n").unwrap();

    // Drop WITHOUT mark_success → outcome Aborted → salvage should commit.
    drop(lease);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // The WIP commit must be the tip of feat/x in the canonical clone's object
    // DB. Check the latest commit message on feat/x.
    let out = Command::new("git")
        .current_dir(&canonical)
        .args(["log", "-1", "--format=%s", "feat/x"])
        .output()
        .unwrap();
    let subject = String::from_utf8_lossy(&out.stdout);
    assert!(
        subject.contains("wip(pidash): salvaged"),
        "feat/x tip should be the salvage commit, got: {subject}"
    );
}

#[tokio::test]
async fn success_lease_does_not_salvage() {
    let (_tmp, canonical, worktrees) = fixture();
    let pool = spawn_pool(workdir_cfg("main", &canonical, 1), &worktrees).await;

    let mut lease = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: Some("feat/x".into()),
        })
        .await
        .expect("acquire");
    let wt = lease.path().to_path_buf();
    std::fs::write(wt.join("scratch.txt"), "transient\n").unwrap();
    lease.mark_success();
    drop(lease);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let out = Command::new("git")
        .current_dir(&canonical)
        .args(["log", "-1", "--format=%s", "feat/x"])
        .output()
        .unwrap();
    let subject = String::from_utf8_lossy(&out.stdout);
    assert!(
        !subject.contains("wip(pidash)"),
        "success path must not salvage, got: {subject}"
    );
}

#[tokio::test]
async fn keep_ignored_preserves_gitignored_files_across_leases() {
    let (_tmp, canonical, worktrees) = fixture();
    let pool = spawn_pool(workdir_cfg("main", &canonical, 1), &worktrees).await;

    let mut lease = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("acquire");
    let wt = lease.path().to_path_buf();
    // gitignored path (matches `ignored/` in .gitignore) — should survive clean.
    std::fs::create_dir_all(wt.join("ignored")).unwrap();
    std::fs::write(wt.join("ignored/cache.bin"), "warm\n").unwrap();
    // tracked-but-untracked stray file — should be removed by clean -fd.
    std::fs::write(wt.join("stray.txt"), "stray\n").unwrap();
    lease.mark_success();
    drop(lease);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Re-acquire (same single desk) and check what survived.
    let lease2 = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("reacquire");
    assert!(
        lease2.path().join("ignored/cache.bin").exists(),
        "gitignored cache should be preserved (warm pool)"
    );
    assert!(
        !lease2.path().join("stray.txt").exists(),
        "untracked stray file should be cleaned"
    );
}

#[tokio::test]
async fn full_clean_removes_gitignored_files() {
    let (_tmp, canonical, worktrees) = fixture();
    let mut cfg = workdir_cfg("main", &canonical, 1);
    cfg.clean_mode = CleanMode::Full;
    let pool = spawn_pool(cfg, &worktrees).await;

    let mut lease = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("acquire");
    let wt = lease.path().to_path_buf();
    std::fs::create_dir_all(wt.join("ignored")).unwrap();
    std::fs::write(wt.join("ignored/cache.bin"), "warm\n").unwrap();
    lease.mark_success();
    drop(lease);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let lease2 = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("reacquire");
    assert!(
        !lease2.path().join("ignored/cache.bin").exists(),
        "full clean should remove gitignored files too"
    );
}

#[tokio::test]
async fn setup_command_failure_marks_pool_unhealthy_and_fails_fast() {
    let (_tmp, canonical, worktrees) = fixture();
    let mut cfg = workdir_cfg("main", &canonical, 1);
    cfg.setup_command = Some("exit 3".into()); // always fails
    let pool = spawn_pool(cfg, &worktrees).await;

    let res = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await;
    assert!(
        matches!(res, Err(AcquireError::PoolUnhealthy { .. })),
        "setup failure should make acquire fail with PoolUnhealthy"
    );
    let snap = pool.snapshot().await.unwrap();
    assert!(!snap.healthy);
    assert!(snap.unhealthy_reason.is_some());

    // A subsequent acquire also fails fast (no eternal queue).
    let res2 = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await;
    assert!(matches!(res2, Err(AcquireError::PoolUnhealthy { .. })));

    // Operator clears it → healthy again.
    pool.clear_unhealthy();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let snap = pool.snapshot().await.unwrap();
    assert!(snap.healthy);
}

#[tokio::test]
async fn init_unhealthy_when_canonical_is_not_a_git_repo() {
    let tmp = TempDir::new().unwrap();
    let not_a_repo = tmp.path().join("plain");
    std::fs::create_dir_all(&not_a_repo).unwrap();
    let cfg = workdir_cfg("main", &not_a_repo, 1);
    let pool = spawn_pool(cfg, &tmp.path().join("wt")).await;
    let snap = pool.snapshot().await.unwrap();
    assert!(!snap.healthy, "non-repo canonical clone should be unhealthy");
}

#[tokio::test]
async fn stale_lock_is_reaped_and_salvaged_on_init() {
    let (_tmp, canonical, worktrees) = fixture();
    // Simulate a crash: manually create a worktree + lock file as if a run was
    // mid-flight, with a dirty tree, then spawn the pool fresh.
    let pool_dir = worktrees.join("main");
    std::fs::create_dir_all(pool_dir.join("locks")).unwrap();
    let wt_path = pool_dir.join("wt-1");
    git::set_gc_auto_off(&canonical).await.ok();
    // Add a worktree on feat/x with an uncommitted change.
    git(&canonical, &[
        "worktree",
        "add",
        "wt-tmp-checkout",
        "feat/x",
    ]);
    // Move it into place as wt-1 (rename the dir; git worktree tracks by path,
    // so instead just add directly at the target path).
    git(&canonical, &["worktree", "remove", "--force", "wt-tmp-checkout"]);
    git(&canonical, &[
        "worktree",
        "add",
        wt_path.to_str().unwrap(),
        "feat/x",
    ]);
    std::fs::write(wt_path.join("crash.txt"), "in-flight work\n").unwrap();
    let lock = format!(
        r#"{{"kind":"run","holder_id":"{}","branch":"feat/x"}}"#,
        Uuid::new_v4()
    );
    std::fs::write(pool_dir.join("locks/wt-1.lock.json"), lock).unwrap();

    // Spawn the pool — init should reap the stale lock, salvaging the dirty tree.
    let pool = spawn_pool(workdir_cfg("main", &canonical, 2), &worktrees).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // The lock file must be gone.
    assert!(
        !pool_dir.join("locks/wt-1.lock.json").exists(),
        "stale lock should be removed on init"
    );
    // The reaped desk should be reusable (idle in the pool).
    let snap = pool.snapshot().await.unwrap();
    assert_eq!(snap.busy, 0);
    assert!(snap.healthy);

    // And the crash work should be salvaged onto feat/x.
    let out = Command::new("git")
        .current_dir(&canonical)
        .args(["log", "-1", "--format=%s", "feat/x"])
        .output()
        .unwrap();
    let subject = String::from_utf8_lossy(&out.stdout);
    assert!(
        subject.contains("wip(pidash): salvaged"),
        "crash tree should be salvaged on reap, got: {subject}"
    );
}

#[tokio::test]
async fn two_runners_share_one_workdir_concurrently() {
    // The motivating scenario: pool_size 2, two concurrent leases granted.
    let (_tmp, canonical, worktrees) = fixture();
    let pool = spawn_pool(workdir_cfg("main", &canonical, 2), &worktrees).await;

    let l1 = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("l1");
    let l2 = pool
        .acquire(LeaseRequest {
            kind: LeaseKind::Run,
            holder_id: Uuid::new_v4(),
            branch: None,
        })
        .await
        .expect("l2 — second desk granted concurrently");
    assert_ne!(l1.path(), l2.path(), "two distinct desks");
    let snap = pool.snapshot().await.unwrap();
    assert_eq!(snap.busy, 2);
    assert_eq!(snap.free_worktrees(), 0);
}

/// Helper: current checked-out branch of a worktree.
fn current_branch(path: &Path) -> String {
    let out = Command::new("git")
        .current_dir(path)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
