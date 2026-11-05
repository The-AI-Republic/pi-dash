//! Daemon shutdown cannot miss a spawn that was admitted before it began
//! (runner cleanup §4.1): an admitted spawn prepares, spawns and attaches its
//! tree before shutdown takes its snapshot. Its own test binary, because
//! `shutdown_all` flips process-wide state.
#![cfg(unix)]

use pidash::util::proctree::{self, ProcessTree};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn alive(pid: i32) -> bool {
    pidash::util::procinfo::info(pid).is_some_and(|p| !p.zombie)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_waits_for_an_admitted_spawn_and_finishes_its_tree() {
    let (admitted_tx, admitted_rx) = mpsc::channel();
    let spawner = std::thread::spawn(move || {
        let admit = proctree::admit().expect("admitted before shutdown");
        admitted_tx.send(()).unwrap();
        // Shutdown starts while this spawn is between admission and attach.
        std::thread::sleep(Duration::from_millis(300));
        let tree = ProcessTree::prepare(None, None).expect("admitted spawn may prepare");
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: one async-signal-safe syscall between fork and exec.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                nix::unistd::setsid()
                    .map(drop)
                    .map_err(|e| std::io::Error::from_raw_os_error(e as i32))
            });
        }
        let mut child = cmd.spawn().unwrap();
        tree.attach(Some(child.id()));
        drop(admit);
        let pid = child.id() as i32;
        // Reap the leader once shutdown kills it, so it does not linger as a
        // zombie; the tree stays owned until then.
        let _ = child.wait();
        drop(tree);
        pid
    });
    admitted_rx.recv().unwrap();

    let started = Instant::now();
    let unresolved = proctree::shutdown_all(Duration::from_secs(10)).await;
    assert_eq!(unresolved, 0);
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "shutdown did not wait for the admitted spawn"
    );
    let pid = tokio::task::spawn_blocking(move || spawner.join().unwrap())
        .await
        .unwrap();
    assert!(!alive(pid), "admitted agent {pid} survived daemon shutdown");
    assert!(
        proctree::admit().is_err(),
        "no spawn is admitted once shutdown began"
    );
}
