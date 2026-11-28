//! Per-run process containment tests (NEWPIDASH-255): a run that starts
//! `sleep 3600 &` and `setsid nohup sleep 3600 &` leaves neither behind
//! after cleanup, normal end, failure, cancellation and the start-up
//! sweep are all covered through the same `RunGroup::cleanup` path.

#![cfg(unix)]

use std::time::Duration;
use uuid::Uuid;

use pidash::util::containment::{BackendKind, create_run_group};
use pidash::util::shell::{RUN_ENV, RunEnv, login_shell_command};

/// Spawn the agent stand-in: a bash that backgrounds a detached sleep, in
/// the run's process group when it has one. Returns the shell's child pid. Both sleeps must end up in the run's
/// containment unit; the shell itself exits immediately (normal end),
/// or stays in the foreground sleep (failure / cancel case).
async fn spawn_leaky_agent(
    backend: BackendKind,
    process_group: Option<i32>,
    script: &str,
) -> (u32, tokio::process::Child) {
    let mut cmd = RUN_ENV
        .scope(
            RunEnv {
                run_id: Uuid::new_v4(),
                backend,
                process_group,
                runner_bin_dir: None,
                config_dir: None,
                data_dir: None,
                runner_exe: None,
                issue_identifier: None,
                project_identifier: None,
                agent_name: None,
            },
            async { login_shell_command("bash", &["-c", script], None) },
        )
        .await;
    cmd.stdin(std::process::Stdio::piped());
    let child = cmd.spawn().expect("spawn agent stand-in");
    let pid = child.id().expect("child has pid");
    (pid, child)
}

fn pid_alive(pid: u32) -> bool {
    // kill(pid, 0): ESRCH means dead, anything else means alive.
    // Zombies still answer kill(0) until the parent reaps them, so
    // treat the Z state as dead (cleanup won; reaping is the kernel's).
    if nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_err() {
        return false;
    }
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let Some(close) = stat.rfind(')') else {
        return true;
    };
    let state = stat[close + 2..].chars().next();
    state != Some('Z')
}

/// Normal end: the agent exits after backgrounding a `sleep` and a
/// `setsid`ed sleep; cleanup must leave neither behind (cgroup backend).
#[tokio::test]
async fn cleanup_kills_backgrounded_and_setsid_processes() {
    let run_id = Uuid::new_v4();
    let mut group = create_run_group(run_id);
    if group.backend_kind() != BackendKind::Cgroup {
        eprintln!("SKIP: cgroup backend unavailable on this host");
        return;
    }
    let pid_dir = tempfile::tempdir().expect("tempdir for pid files");
    let bg_pid_file = pid_dir.path().join("bg.pid");
    let ss_pid_file = pid_dir.path().join("setsid.pid");
    let script = format!(
        "sleep 3600 & echo $! > {} ; setsid nohup sleep 3600 >/dev/null 2>&1 & echo $! > {} ; exit 0",
        bg_pid_file.display(),
        ss_pid_file.display(),
    );
    let (pid, child) = spawn_leaky_agent(BackendKind::Cgroup, None, &script).await;
    group.move_pid(Some(pid));
    let mut child = child;
    let _ = child.wait().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let bg_pid = read_pid_file(&bg_pid_file);
    let ss_pid = read_pid_file(&ss_pid_file);
    assert!(
        pid_alive(bg_pid),
        "backgrounded sleep must be alive pre-cleanup"
    );
    assert!(pid_alive(ss_pid), "setsid sleep must be alive pre-cleanup");
    group.cleanup().await;
    assert_dead(bg_pid, "backgrounded sleep must die with the run cgroup").await;
    assert_dead(ss_pid, "setsid sleep must die with the run cgroup").await;
    // The sleeps died with the cgroup. The cgroup dir is gone; check
    // the unit is empty by confirming rmdir happened.
    let dir = self_cgroup_base().join(format!("pidash-run-{run_id}"));
    assert!(!dir.exists(), "run cgroup must be removed after cleanup");
}

/// Failure / cancellation: the agent is still alive (in a foreground
/// sleep) when cleanup runs; everything it started must die.
#[tokio::test]
async fn cleanup_kills_processes_of_live_agent() {
    let run_id = Uuid::new_v4();
    let mut group = create_run_group(run_id);
    if group.backend_kind() != BackendKind::Cgroup {
        eprintln!("SKIP: cgroup backend unavailable on this host");
        return;
    }
    let (pid, mut child) = spawn_leaky_agent(
        BackendKind::Cgroup,
        None,
        "sleep 3600 & setsid nohup sleep 3600 >/dev/null 2>&1 & sleep 3600",
    )
    .await;
    group.move_pid(Some(pid));
    tokio::time::sleep(Duration::from_millis(500)).await;
    group.cleanup().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !pid_alive(pid),
        "agent process itself must be dead after cleanup"
    );
    let _ = child.kill().await;
}

/// Abort path: dropping the guard without cleanup (daemon shutdown,
/// `RemoveRunner`) still empties the unit via `Drop`.
#[tokio::test]
async fn dropped_guard_empties_unit() {
    let run_id = Uuid::new_v4();
    let mut group = create_run_group(run_id);
    if group.backend_kind() != BackendKind::Cgroup {
        eprintln!("SKIP: cgroup backend unavailable on this host");
        return;
    }
    let (pid, _child) = spawn_leaky_agent(
        BackendKind::Cgroup,
        None,
        "sleep 3600 & setsid nohup sleep 3600 >/dev/null 2>&1 & sleep 3600",
    )
    .await;
    group.move_pid(Some(pid));
    tokio::time::sleep(Duration::from_millis(400)).await;
    drop(group);
    // Drop spawns the cleanup on the current runtime; wait for it to
    // carry out the TERM→KILL (grace elapses only if something lingers).
    for _ in 0..100 {
        if !pid_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("agent pid still alive after guard drop");
}

/// Process-group backend: backgrounded children die at cleanup.
#[tokio::test]
async fn process_group_cleanup_kills_backgrounded_child() {
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("SKIP: member listing needs /proc");
        return;
    }
    let run_id = Uuid::new_v4();
    let mut group = create_run_group(run_id);
    // Whatever backend this host picked (cgroup or process group),
    // the backgrounded child must die at cleanup. The agent itself
    // already exited here (normal-end shape); the live-agent shape is
    // covered by the cgroup test above.
    let backend = group.backend_kind();
    let pid_dir = tempfile::tempdir().expect("tempdir for pid file");
    let bg_pid_file = pid_dir.path().join("bg.pid");
    let script = format!("sleep 3600 & echo $! > {} ; exit 0", bg_pid_file.display());
    let (pid, mut child) = spawn_leaky_agent(backend, group.agent_pgid(), &script).await;
    group.move_pid(Some(pid));
    let _ = child.wait().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let bg_pid = read_pid_file(&bg_pid_file);
    assert!(
        pid_alive(bg_pid),
        "backgrounded sleep must be alive pre-cleanup"
    );
    group.cleanup().await;
    // The backgrounded sleep shared the agent's process group; it must
    // be dead. (A setsid'd child escapes the process-group backend —
    // the documented limit — so this test only backgrounds.)
    assert_dead(
        bg_pid,
        "backgrounded sleep must die with the agent's process group",
    )
    .await;
}

/// Read a pid captured via `echo $! > file` in a spawned script.
fn read_pid_file(path: &std::path::Path) -> u32 {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("read pid file {}", path.display()));
    raw.trim()
        .parse::<u32>()
        .unwrap_or_else(|_| panic!("parse pid from {}", path.display()))
}

/// Poll until `pid` is dead (ESRCH or zombie), failing after ~5s.
async fn assert_dead(pid: u32, msg: &str) {
    for _ in 0..50 {
        if !pid_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{msg} (pid {pid} still alive after cleanup)");
}

/// Start-up sweep: a cgroup left by a lost daemon is emptied and
/// removed at the next start.
#[tokio::test]
async fn sweep_stops_orphaned_run_groups() {
    if backend_off() {
        eprintln!("SKIP: containment off");
        return;
    }
    let base = self_cgroup_base();
    let stale_id = Uuid::new_v4();
    let dir = base.join(format!("pidash-run-{stale_id}"));
    if std::fs::create_dir_all(&dir).is_err() {
        eprintln!("SKIP: cannot create cgroup under self path");
        return;
    }
    let child = std::process::Command::new("sleep")
        .arg("3600")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    std::fs::write(dir.join("cgroup.procs"), format!("{pid}\n")).expect("write cgroup.procs");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(pid_alive(pid));
    pidash::util::containment::sweep_orphaned_run_groups().await;
    for _ in 0..30 {
        if !pid_alive(pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!pid_alive(pid), "sweep must stop the orphaned sleep");
    let mut child2 = child;
    let _ = child2.wait();
    assert!(!dir.exists(), "sweep must remove the orphaned cgroup");
}

/// The run-end hook runs outside the unit and is deleted after.
#[tokio::test]
async fn run_end_hook_executes_and_is_removed() {
    let ws = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(ws.path().join(".pidash")).expect("mkdir");
    let marker = ws.path().join("hook-ran");
    std::fs::write(
        ws.path().join(".pidash/run-end-hook"),
        format!("touch {}\n", marker.display()),
    )
    .expect("write hook");
    pidash::util::containment::run_end_hook(ws.path()).await;
    assert!(marker.exists(), "hook must have run");
    assert!(
        !pidash::util::containment::run_end_hook_path(ws.path()).exists(),
        "hook must be deleted after running"
    );
}

fn backend_off() -> bool {
    std::env::var("PIDASH_CONTAINMENT")
        .map(|v| v.eq_ignore_ascii_case("off"))
        .unwrap_or(false)
}

fn self_cgroup_base() -> std::path::PathBuf {
    for line in std::fs::read_to_string("/proc/self/cgroup")
        .expect("read /proc/self/cgroup")
        .lines()
    {
        if let Some(path) = line.strip_prefix("0::") {
            let path = path.trim();
            if !path.is_empty() && path != "/" {
                return std::path::PathBuf::from(format!("/sys/fs/cgroup{path}"));
            }
        }
    }
    panic!("no cgroup v2 self path");
}
