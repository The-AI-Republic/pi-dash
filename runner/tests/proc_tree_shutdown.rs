//! Daemon shutdown finishes every live agent process tree (runner cleanup
//! §4.1). Its own test binary: `shutdown_all` flips process-wide state (no
//! new spawns), which must not leak into other tests.
#![cfg(unix)]

use pidash::agent::tracked::{self, TrackedChild};
use pidash::util::proctree;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;

fn alive(pid: i32) -> bool {
    pidash::util::procinfo::info(pid).is_some_and(|p| !p.zombie)
}

/// A fake agent whose group holds a SIGTERM-ignoring child; the leader never
/// exits by itself.
fn agent(pidfile: &std::path::Path) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg("(trap '' TERM; while :; do sleep 1; done) & echo $! > \"$PF\"; while :; do sleep 1; done")
        .env("PF", pidfile)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    pidash::util::shell::new_session(&mut cmd);
    cmd
}

#[tokio::test]
async fn daemon_shutdown_finishes_all_trees_in_parallel_and_stops_new_spawns() {
    let dir = tempfile::tempdir().unwrap();
    let mut handles = Vec::new();
    let mut pids = Vec::new();
    for i in 0..3 {
        let pf = dir.path().join(format!("pid{i}"));
        let (child, tree) = tracked::spawn(&mut agent(&pf)).unwrap();
        let leader = child.id().unwrap() as i32;
        let (exit_tx, _rx) = tokio::sync::watch::channel(None);
        handles.push(TrackedChild::supervise(child, tree, exit_tx));
        let deadline = Instant::now() + Duration::from_secs(20);
        let child_pid = loop {
            if let Ok(s) = std::fs::read_to_string(&pf)
                && let Ok(p) = s.trim().parse::<i32>()
            {
                break p;
            }
            assert!(Instant::now() < deadline, "fake agent never started");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        pids.extend([leader, child_pid]);
    }

    let started = Instant::now();
    let unresolved = proctree::shutdown_all(Duration::from_secs(10)).await;
    assert_eq!(unresolved, 0);
    // Parallel: three trees that each need SIGTERM (3 s) then SIGKILL take
    // about one tree's time, well inside the 10 s budget.
    assert!(started.elapsed() < Duration::from_secs(8), "{:?}", started.elapsed());
    let deadline = Instant::now() + Duration::from_secs(3);
    while pids.iter().any(|p| alive(*p)) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for p in &pids {
        assert!(!alive(*p), "pid {p} survived daemon shutdown");
    }

    assert!(proctree::shutting_down());
    let err = tracked::spawn(&mut agent(&dir.path().join("late"))).err();
    assert!(err.is_some(), "no new agent may be spawned once shutdown began");
    drop(handles);
}
