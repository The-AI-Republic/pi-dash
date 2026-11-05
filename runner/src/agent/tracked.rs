//! One supervised agent subprocess: the `Child`, its [`ProcessTree`], and the
//! wait task that owns both.
//!
//! Every bridge spawns through [`spawn`] and stops through
//! [`TrackedChild::finish`], which replaces the per-bridge `shutdown`:
//!
//! 1. graceful: the caller has closed stdin; wait for the leader up to
//!    `grace` (today's behaviour, now bounded);
//! 2. finish the group: SIGTERM ≤ 3 s, SIGKILL ≤ 2 s
//!    ([`ProcessTree::finish_group`]).
//!
//! Dropping a `TrackedChild` without finishing it (a dropped bridge) closes
//! the control channel; the wait task then runs the same finish with a 2 s
//! grace instead of waiting for the agent to exit on its own. If the task
//! itself is torn down (runtime shutdown), `kill_on_drop` kills the leader
//! and the tree's drop backstop SIGKILLs the group.
//!
//! The wait task keeps the tree after the leader exits: whatever the agent
//! started stays owned until the bridge is finished (a chat runtime's dev
//! server lives until the runtime ends). Design:
//! `.ai_design/runner_cleanup/design.md` §4.1.

use anyhow::{Context, Result};
use chrono::Utc;
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};

use crate::agent::ExitSnapshot;
use crate::util::proctree::{self, FinishOutcome, PROC_TAG_ENV, ProcessTree, TreeView};

/// Grace a dropped (unfinished) bridge gives its agent.
pub const DROP_GRACE: Duration = Duration::from_secs(2);

enum Ctl {
    /// Force-kill the leader only (interrupt fallback; unchanged behaviour).
    KillLeader,
    Finish {
        grace: Duration,
        done: oneshot::Sender<FinishOutcome>,
    },
}

/// Prepare the tree (provisional record), tag the command, spawn, and record
/// the leader. `kill_on_drop` is forced on.
pub fn spawn(cmd: &mut Command) -> Result<(Child, ProcessTree)> {
    // Held until the tree is attached, so daemon shutdown cannot snapshot
    // (or finish) the tree between prepare and attach.
    let admit = proctree::admit().context("admitting agent spawn")?;
    let tree = ProcessTree::prepare_current().context("preparing agent process record")?;
    if let Some(tag) = tree.tag() {
        cmd.env(PROC_TAG_ENV, tag);
    }
    cmd.kill_on_drop(true);
    let child = cmd.spawn()?;
    tree.attach(child.id());
    drop(admit);
    Ok((child, tree))
}

/// Handle to a supervised agent subprocess.
pub struct TrackedChild {
    pid: Option<u32>,
    ctl: mpsc::Sender<Ctl>,
    exit_rx: watch::Receiver<Option<ExitSnapshot>>,
    view: TreeView,
}

impl TrackedChild {
    /// Hand `child` and `tree` to a wait task. Exit is published on
    /// `exit_tx` the moment the leader is reaped.
    pub fn supervise(
        child: Child,
        tree: ProcessTree,
        exit_tx: watch::Sender<Option<ExitSnapshot>>,
    ) -> Self {
        let pid = child.id();
        let exit_rx = exit_tx.subscribe();
        let view = tree.view();
        let (ctl, ctl_rx) = mpsc::channel(4);
        tokio::spawn(wait_task(child, tree, ctl_rx, exit_tx));
        Self {
            pid,
            ctl,
            exit_rx,
            view,
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn exit_rx(&self) -> watch::Receiver<Option<ExitSnapshot>> {
        self.exit_rx.clone()
    }

    /// True once the group is gone (a live leader keeps its group alive, so
    /// this also means the leader has exited).
    pub fn is_gone(&self) -> bool {
        self.view.is_gone()
    }

    /// Force-kill the leader (the interrupt fallback when SIGINT fails).
    pub async fn kill_leader(&self) -> Result<()> {
        self.ctl
            .send(Ctl::KillLeader)
            .await
            .map_err(|_| anyhow::anyhow!("agent wait task already ended"))
    }

    /// Graceful wait for the leader up to `grace`, then finish the group.
    /// The caller closes stdin first. Bounded: never waits longer than
    /// `grace` + 3 s + 2 s plus a small reap margin.
    pub async fn finish(self, grace: Duration) -> FinishOutcome {
        let (done, rx) = oneshot::channel();
        if self.ctl.send(Ctl::Finish { grace, done }).await.is_err() {
            // The wait task already finished the tree.
            return FinishOutcome::Gone;
        }
        let bound = grace
            + crate::util::proctree::TERM_WAIT
            + crate::util::proctree::KILL_WAIT
            + Duration::from_secs(3);
        match tokio::time::timeout(bound, rx).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => FinishOutcome::Gone,
            Err(_) => FinishOutcome::Unresolved("finish timed out".to_string()),
        }
    }
}

fn exit_snapshot_from(status: Option<std::process::ExitStatus>) -> ExitSnapshot {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.as_ref().and_then(|s| s.signal())
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    ExitSnapshot {
        status_code: status.as_ref().and_then(|s| s.code()),
        signal,
        observed_at: Utc::now(),
    }
}

async fn wait_task(
    mut child: Child,
    tree: ProcessTree,
    mut ctl: mpsc::Receiver<Ctl>,
    exit_tx: watch::Sender<Option<ExitSnapshot>>,
) {
    let mut exited = false;
    // Publish the exit and tell the tree its leader is reaped: from here an
    // emptied group's pgid may be reused, so the tree re-proves ownership
    // before any later signal.
    let publish = |res: std::io::Result<std::process::ExitStatus>| {
        let _ = exit_tx.send(Some(exit_snapshot_from(res.ok())));
        tree.leader_reaped();
    };
    let (grace, done) = loop {
        tokio::select! {
            biased;
            req = ctl.recv() => match req {
                Some(Ctl::KillLeader) => {
                    if !exited {
                        let _ = child.start_kill();
                    }
                }
                Some(Ctl::Finish { grace, done }) => break (grace, Some(done)),
                // Every handle dropped without finishing: a dropped bridge.
                None => break (DROP_GRACE, None),
            },
            res = child.wait(), if !exited => {
                publish(res);
                exited = true;
            }
        }
    };

    // 1. Graceful: wait for the leader, bounded by `grace`.
    if !exited && let Ok(res) = tokio::time::timeout(grace, child.wait()).await {
        publish(res);
        exited = true;
    }

    // 2. Finish the group, reaping the leader concurrently so its zombie does
    //    not keep the group alive.
    let outcome = {
        let fin = tree.finish_group();
        tokio::pin!(fin);
        loop {
            tokio::select! {
                o = &mut fin => break o,
                res = child.wait(), if !exited => {
                    publish(res);
                    exited = true;
                }
            }
        }
    };
    if !exited {
        // No group to finish (not a session leader, or Windows): kill the
        // leader as the old shutdown did, then reap with a bound.
        let _ = child.start_kill();
        if let Ok(res) = tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
            publish(res);
        }
    }
    drop(tree);
    if let Some(done) = done {
        let _ = done.send(outcome);
    }
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Stdio;

    /// True while `pid` exists and is not a zombie.
    pub(crate) fn alive(pid: i32) -> bool {
        crate::util::procinfo::info(pid).is_some_and(|p| !p.zombie)
    }

    pub(crate) async fn wait_all_gone(pids: &[i32], bound: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + bound;
        while tokio::time::Instant::now() < deadline {
            if pids.iter().all(|p| !alive(*p)) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    pub(crate) async fn read_pids(file: &Path, n: usize) -> Vec<i32> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let pids: Vec<i32> = std::fs::read_to_string(file)
                .unwrap_or_default()
                .split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect();
            if pids.len() >= n {
                return pids;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "fake agent never wrote its pids"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// A fake agent: starts `sleep 300 &`, a child that traps SIGTERM (exits
    /// on it) and, optionally, a child that ignores SIGTERM. The leader
    /// itself waits for stdin EOF, or exits at once when `leader_exits`.
    pub(crate) fn fake_agent(pidfile: &Path, ignore_term: bool, leader_exits: bool) -> Command {
        let ignorer = if ignore_term {
            "(trap '' TERM; while :; do sleep 1; done) & echo $! >> \"$PF\";"
        } else {
            ""
        };
        let tail = if leader_exits {
            "exit 0"
        } else {
            "cat >/dev/null"
        };
        let script = format!(
            "sleep 300 & echo $! >> \"$PF\"; \
             (trap 'exit 0' TERM; while :; do sleep 1; done) & echo $! >> \"$PF\"; \
             {ignorer} echo $$ >> \"$PF\"; {tail}"
        );
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script).env("PF", pidfile);
        crate::util::shell::new_session(&mut cmd);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    }

    fn start(cmd: &mut Command) -> (TrackedChild, Option<tokio::process::ChildStdin>) {
        let (mut child, tree) = spawn(cmd).unwrap();
        let stdin = child.stdin.take();
        let (exit_tx, _rx) = watch::channel(None);
        (TrackedChild::supervise(child, tree, exit_tx), stdin)
    }

    #[tokio::test]
    async fn finish_cleans_the_whole_group_after_a_normal_exit() {
        let dir = tempfile::tempdir().unwrap();
        let pf = dir.path().join("pids");
        let (tracked, stdin) = start(&mut fake_agent(&pf, false, false));
        let pids = read_pids(&pf, 3).await;
        drop(stdin); // graceful: the leader exits on EOF
        let outcome = tracked.finish(Duration::from_secs(5)).await;
        assert_eq!(outcome, FinishOutcome::Gone);
        assert!(
            wait_all_gone(&pids, Duration::from_secs(1)).await,
            "{pids:?} survived"
        );
    }

    #[tokio::test]
    async fn a_child_that_ignores_sigterm_dies_by_sigkill() {
        let dir = tempfile::tempdir().unwrap();
        let pf = dir.path().join("pids");
        let (tracked, _stdin) = start(&mut fake_agent(&pf, true, false));
        let pids = read_pids(&pf, 4).await;
        // Leader never sees EOF (stdin kept open): grace expires, SIGTERM,
        // then SIGKILL for the ignorer.
        let started = tokio::time::Instant::now();
        let outcome = tracked.finish(Duration::from_millis(200)).await;
        assert_eq!(outcome, FinishOutcome::Gone);
        assert!(started.elapsed() >= crate::util::proctree::TERM_WAIT);
        assert!(
            wait_all_gone(&pids, Duration::from_secs(1)).await,
            "{pids:?} survived"
        );
    }

    #[tokio::test]
    async fn group_is_cleaned_when_the_leader_exits_first() {
        let dir = tempfile::tempdir().unwrap();
        let pf = dir.path().join("pids");
        let (tracked, _stdin) = start(&mut fake_agent(&pf, false, true));
        let pids = read_pids(&pf, 3).await;
        let mut exit_rx = tracked.exit_rx();
        tokio::time::timeout(Duration::from_secs(10), exit_rx.wait_for(Option::is_some))
            .await
            .expect("leader exit observed")
            .unwrap();
        assert!(alive(pids[0]), "children outlive the leader until finish");
        assert!(!tracked.is_gone());
        assert_eq!(
            tracked.finish(Duration::from_secs(1)).await,
            FinishOutcome::Gone
        );
        assert!(
            wait_all_gone(&pids, Duration::from_secs(1)).await,
            "{pids:?} survived"
        );
    }

    #[tokio::test]
    async fn dropping_an_unfinished_child_still_cleans_its_group() {
        let dir = tempfile::tempdir().unwrap();
        let pf = dir.path().join("pids");
        let (tracked, stdin) = start(&mut fake_agent(&pf, true, false));
        let pids = read_pids(&pf, 4).await;
        drop(tracked);
        drop(stdin);
        assert!(
            wait_all_gone(&pids, Duration::from_secs(15)).await,
            "{pids:?} survived a dropped bridge"
        );
    }

    #[tokio::test]
    async fn interrupt_fallback_still_targets_the_leader_only() {
        let dir = tempfile::tempdir().unwrap();
        let pf = dir.path().join("pids");
        let (tracked, _stdin) = start(&mut fake_agent(&pf, false, false));
        let pids = read_pids(&pf, 3).await;
        let leader = *pids.last().unwrap();
        assert_eq!(tracked.pid(), Some(leader as u32));
        tracked.kill_leader().await.unwrap();
        let mut exit_rx = tracked.exit_rx();
        tokio::time::timeout(Duration::from_secs(10), exit_rx.wait_for(Option::is_some))
            .await
            .unwrap()
            .unwrap();
        assert!(
            alive(pids[0]),
            "killing the leader leaves the group to finish"
        );
        assert_eq!(
            tracked.finish(Duration::from_secs(1)).await,
            FinishOutcome::Gone
        );
        assert!(wait_all_gone(&pids, Duration::from_secs(1)).await);
    }
}
