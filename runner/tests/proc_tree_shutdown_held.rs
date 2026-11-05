//! Daemon shutdown also retries chat trees whose earlier finish could not
//! confirm their group gone (held trees, runner cleanup §4.1), and counts the
//! ones still unresolved. Its own test binary, because `shutdown_all` flips
//! process-wide state.
#![cfg(target_os = "linux")]

use pidash::util::proctree::{self, FinishOutcome, ProcContext, ProcScope, ProcessTree, ScopeKind};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;

fn alive(pid: i32) -> bool {
    pidash::util::procinfo::info(pid).is_some_and(|p| !p.zombie)
}

/// A chat tree whose leader exits leaving an untagged `sleep`: after the reap
/// the group cannot be proven ours, so its finish is unresolved and the tree
/// is held. Returns the record path and the orphan's pid.
async fn held_tree(
    ctx: &Arc<ProcContext>,
    dir: &std::path::Path,
    id: &str,
) -> (std::path::PathBuf, i32) {
    let scope = ProcScope {
        kind: ScopeKind::Chat,
        id: id.to_string(),
        runner_id: Some("r1".to_string()),
    };
    let tree = ProcessTree::prepare(Some(scope), Some(ctx.clone())).unwrap();
    let pf = dir.join(format!("{id}.pid"));
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("sleep 300 & echo $! > \"$PF\"; exit 0")
        .env("PF", &pf)
        .env_remove("PIDASH_PROC_TAG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    pidash::util::shell::new_session(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    tree.attach(child.id());
    child.wait().await.unwrap();
    tree.leader_reaped();
    let deadline = Instant::now() + Duration::from_secs(10);
    let orphan = loop {
        if let Some(pid) = std::fs::read_to_string(&pf)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
        {
            break pid;
        }
        assert!(Instant::now() < deadline, "no orphan pid");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let outcome = tree.finish_group().await;
    assert!(
        matches!(outcome, FinishOutcome::Unresolved(_)),
        "{outcome:?}"
    );
    let record = tree.record_path().unwrap();
    drop(tree);
    assert!(proctree::scope_unresolved(ScopeKind::Chat, id), "held");
    (record, orphan)
}

#[tokio::test]
async fn shutdown_retries_held_trees_and_counts_the_unresolved_ones() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = Arc::new(ProcContext::new("test-instance-shutdown-held", dir.path()));
    let (still_record, still_orphan) = held_tree(&ctx, dir.path(), "chat-still").await;
    let (gone_record, gone_orphan) = held_tree(&ctx, dir.path(), "chat-gone").await;

    // One held group empties on its own; only a retry can see it gone.
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(gone_orphan),
        nix::sys::signal::Signal::SIGKILL,
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(gone_orphan) {
        assert!(
            Instant::now() < deadline,
            "orphan {gone_orphan} survived SIGKILL"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let unresolved = proctree::shutdown_all(Duration::from_secs(10)).await;
    assert_eq!(unresolved, 1, "the still-unprovable held tree is counted");
    assert!(
        !gone_record.exists(),
        "the emptied held tree was retried and resolved"
    );
    assert!(still_record.exists(), "the unresolved one keeps its record");
    assert!(
        alive(still_orphan),
        "an unprovable group is never signalled"
    );
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(still_orphan),
        nix::sys::signal::Signal::SIGKILL,
    );
}
