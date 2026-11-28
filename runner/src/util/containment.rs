//! Per-run process containment (NEWPIDASH-255).
//!
//! When an agent run ends the runner kills the agent process, but not what
//! the agent started: background compiles, dev servers and scratch
//! databases survive, reparented away from the daemon, and exhaust host
//! memory. Containment gives every run a unit it can be emptied by,
//! whatever the agent launched — including via `setsid`/`nohup`.
//!
//! Backends, picked once per daemon (see [`backend`]):
//! * **cgroup** (Linux): the agent is moved into a child cgroup of the
//!   daemon's own cgroup (`pidash-run-<run_id>`). Membership survives
//!   `setsid` and reparenting, which is exactly the leak we are fixing.
//!   Cleanup reads `cgroup.procs`, SIGTERMs every member, waits the grace
//!   period, then SIGKILLs what remains. The start-up sweep finds
//!   `pidash-run-*` children the previous daemon left behind.
//! * **process group** (elsewhere Unix): the agent gets its own process
//!   group (`process_group(0)`) and cleanup signals the group — but only
//!   while a fresh snapshot proves the group is still the run's (see
//!   [`crate::util::group_ownership`]): a pgid whose members all exited
//!   can be handed to an unrelated group, and an unproven group is left
//!   running rather than risk killing a stranger. Members are learned by
//!   a periodic observer while the run is live, so proof outlives the
//!   leader. A child that calls `setsid` escapes the group — a documented
//!   limit of this backend. Platforms without process inspection (not
//!   Linux or macOS) can never prove ownership, so nothing is signalled
//!   there. Cleanup cannot sweep after a crash (a dead PGID can be
//!   reused by the OS), so swept recovery only exists on the cgroup
//!   backend.
//! * **none**: `PIDASH_CONTAINMENT=off` (diagnosis escape hatch) or
//!   Windows, where this module only ensures `kill_on_drop`. The old
//!   leak applies; the override is never recommended.
//!
//! `PIDASH_CONTAINMENT` selects the backend: `auto` (default),
//! `cgroup`, `process-group` or `off`.
//!
//! Run-end hooks: a run that needs something to outlive it must opt in
//! explicitly. The agent (or `parity-up.sh`, which tears the parity
//! Docker stack down) writes a shell script to
//! [`run_end_hook_path`] inside the workspace; at run end the runner
//! executes it — outside the run cgroup, in the daemon's own
//! context — after the cgroup is emptied, then deletes it. A crash
//! between hook-write and execution leaves the file behind, and the
//! start-up sweep runs it, so parity-stack teardown survives a daemon
//! crash too. Stale hooks are removed (not run) when a fresh run starts
//! after the sweep had no chance to consume them.
//!
//! Docker containers the run started are **not** child processes and
//! are out of scope: their teardown stays with the parity stack.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
#[cfg(unix)]
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

#[cfg(unix)]
use crate::util::group_ownership::{GroupOwner, GroupSignal, Proof};

/// Grace between SIGTERM and SIGKILL at run end (issue default).
pub const CLEANUP_GRACE: Duration = Duration::from_secs(10);
/// How long a run-end hook may run before the runner stops it.
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(120);
/// How often a live run's process group is re-observed, so members are
/// remembered (and keep proving the group) after the leader exits.
#[cfg(unix)]
const OBSERVE_INTERVAL: Duration = Duration::from_secs(1);
/// Prefix of the cgroups the sweep looks for under the daemon's cgroup.
const CGROUP_PREFIX: &str = "pidash-run-";

fn backend() -> BackendKind {
    static BACKEND: OnceLock<BackendKind> = OnceLock::new();
    *BACKEND.get_or_init(|| {
        let chosen = std::env::var("PIDASH_CONTAINMENT").unwrap_or_else(|_| "auto".to_string());
        match chosen.to_ascii_lowercase().as_str() {
            "off" => BackendKind::None,
            "process-group" | "process_group" | "pgid" => BackendKind::ProcessGroup,
            "cgroup" => BackendKind::Cgroup,
            "auto" => detect_auto(),
            other => {
                tracing::warn!(
                    value = other,
                    "unknown PIDASH_CONTAINMENT value; using auto"
                );
                detect_auto()
            }
        }
    })
}

/// Pick a backend from the platform: cgroup on Linux (capability is
/// verified per run, with process-group fallback inside the run) and
/// process group elsewhere on Unix. Windows has no equivalent here.
fn detect_auto() -> BackendKind {
    #[cfg(target_os = "linux")]
    {
        BackendKind::Cgroup
    }
    #[cfg(not(target_os = "linux"))]
    {
        #[cfg(unix)]
        {
            BackendKind::ProcessGroup
        }
        #[cfg(not(unix))]
        {
            BackendKind::None
        }
    }
}

/// Handle for a run's unit of containment. Created in
/// [`create_run_group`]; [`RunGroup::cleanup`] empties it and logs one
/// line per run. `workspace` is set by the caller once workspace
/// resolution succeeds, so cleanup can run the run-end hook there.
#[derive(Debug)]
pub struct RunGroup {
    run_id: Uuid,
    backend: BackendKind,
    cgroup_dir: Option<PathBuf>,
    /// Process-group backend: the run's claim on its agent's group. `None`
    /// until the agent is spawned, and forever when the leader could not
    /// be read right after spawn (then nothing is ever signalled).
    #[cfg(unix)]
    owner: Option<Arc<Mutex<GroupOwner>>>,
    /// Background task re-observing `owner` while the run is live.
    #[cfg(unix)]
    observer: Option<tokio::task::JoinHandle<()>>,
    /// TERM → KILL grace ([`CLEANUP_GRACE`]; shorter in tests).
    grace: Duration,
    workspace: Option<PathBuf>,
    /// Set once the normal cleanup path has emptied the unit; the
    /// [`Drop`] fallback only stops processes on the abort path (the
    /// run-end hook is left for the daemon-start sweep, which runs
    /// hooks a previous daemon never got to).
    cleaned: bool,
}

impl RunGroup {
    /// Name of the backend in use, for logs.
    fn backend_name(&self) -> &'static str {
        match self.backend {
            BackendKind::Cgroup => "cgroup",
            BackendKind::ProcessGroup => "process-group",
            BackendKind::None => "none",
        }
    }

    /// Record the agent child PID just spawned. The cgroup move must
    /// happen promptly: the agent binary itself starts seconds after
    /// spawn (engine startup), so a parent-side move right after spawn
    /// reliably contains everything the agent starts. Only processes
    /// a user's `bashrc` already backgrounded in the bash-init window
    /// can race past the move.
    ///
    /// On the process-group backend the child already got its own process
    /// group from `login_shell_command`; this claims it (leader pid plus
    /// exact start time) and starts the observer that remembers members.
    pub fn move_pid(&mut self, pid: Option<u32>) {
        let Some(pid) = pid else { return };
        match self.backend {
            BackendKind::Cgroup => {
                let Some(dir) = &self.cgroup_dir else {
                    return;
                };
                let procs = dir.join("cgroup.procs");
                match std::fs::write(&procs, format!("{pid}\n")) {
                    Ok(()) => tracing::debug!(
                        run_id = %self.run_id, pid,
                        "agent pid moved into run cgroup"
                    ),
                    Err(e) => tracing::warn!(
                        run_id = %self.run_id, pid, error = %e,
                        "failed to move agent pid into run cgroup"
                    ),
                }
            }
            BackendKind::ProcessGroup => {
                #[cfg(unix)]
                {
                    let Ok(pid) = i32::try_from(pid) else { return };
                    match GroupOwner::claim_pid(pid) {
                        Some(owner) => self.set_owner(owner),
                        None => tracing::warn!(
                            run_id = %self.run_id, pid,
                            "agent process group cannot be proven; its processes will not be signalled at run end"
                        ),
                    }
                }
            }
            BackendKind::None => {}
        }
    }

    /// Install the group claim and start observing it.
    #[cfg(unix)]
    fn set_owner(&mut self, owner: GroupOwner) {
        let owner = Arc::new(Mutex::new(owner));
        if let Some(old) = self.observer.take() {
            old.abort();
        }
        self.observer = tokio::runtime::Handle::try_current()
            .ok()
            .map(|h| h.spawn(observe_loop(owner.clone())));
        self.owner = Some(owner);
    }

    /// Empty the unit: SIGTERM everything, wait [`CLEANUP_GRACE`], then
    /// SIGKILL what remains. Logs exactly one line per run with the count
    /// and the names of the processes stopped (see [`cleanup_report`]).
    pub async fn cleanup(&self) {
        let stopped = self.stop_all().await;
        cleanup_report(self.run_id, self.backend_name(), &stopped);
    }

    /// SIGTERM the contents of the unit, wait the grace, SIGKILL the
    /// rest. Returns the processes that were stopped (pid + name).
    async fn stop_all(&self) -> Vec<(u32, String)> {
        match self.backend {
            BackendKind::Cgroup => {
                let Some(dir) = &self.cgroup_dir else {
                    return Vec::new();
                };
                // Member listing is a handful of tiny file reads; done
                // synchronously so the TERM→KILL driver stays simple.
                let stopped = tokio::task::spawn_blocking({
                    let dir = dir.clone();
                    move || {
                        let members = cgroup_members(&dir);
                        stop_members(
                            members,
                            |pids| {
                                for pid in pids {
                                    send_signal(pid, Signal::Term);
                                }
                            },
                            || cgroup_members(&dir),
                            |pids| {
                                for pid in pids {
                                    send_signal(pid, Signal::Kill);
                                }
                            },
                            CLEANUP_GRACE,
                        )
                    }
                })
                .await
                .unwrap_or_default();
                remove_cgroup_dir(dir).await;
                stopped
            }
            BackendKind::ProcessGroup => {
                #[cfg(unix)]
                {
                    if let Some(observer) = &self.observer {
                        observer.abort();
                    }
                    let Some(owner) = self.owner.clone() else {
                        return Vec::new();
                    };
                    let (run_id, grace) = (self.run_id, self.grace);
                    tokio::task::spawn_blocking(move || stop_owned_group(run_id, &owner, grace))
                        .await
                        .unwrap_or_default()
                }
                #[cfg(not(unix))]
                {
                    Vec::new()
                }
            }
            BackendKind::None => Vec::new(),
        }
    }
}

/// Shared TERM → grace → KILL driver over a member list. `relist`
/// re-reads the live members while waiting so processes that started
/// (or detached into) the unit during the wait still get SIGKILL, and
/// `kill` signals the stragglers. Returns the union of everything that
/// had to be signalled (names from the latest sample that saw each pid).
fn stop_members<F, G, K>(
    members: Vec<(u32, String)>,
    mut term: F,
    mut relist: G,
    mut kill: K,
    grace: Duration,
) -> Vec<(u32, String)>
where
    F: FnMut(Vec<u32>),
    G: FnMut() -> Vec<(u32, String)>,
    K: FnMut(Vec<u32>),
{
    if members.is_empty() {
        return members;
    }
    term(members.iter().map(|(pid, _)| *pid).collect());
    let mut union: Vec<(u32, String)> = members.clone();
    // Poll in small slices so a process that exits promptly doesn't
    // wait out the full grace. Blocking the (blocking-pool) thread is
    // the point: the grace must elapse before the kill.
    let deadline = std::time::Instant::now() + grace;
    let mut last: Vec<(u32, String)>;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let live = relist();
        let seen: std::collections::HashSet<u32> = union.iter().map(|(pid, _)| *pid).collect();
        for (pid, name) in live.iter() {
            if !seen.contains(pid) {
                union.push((*pid, name.clone()));
            }
        }
        let done = live.is_empty() || std::time::Instant::now() >= deadline;
        last = live;
        if done {
            break;
        }
    }
    if !last.is_empty() {
        kill(last.iter().map(|(pid, _)| *pid).collect());
        std::thread::sleep(Duration::from_millis(200));
        for (pid, name) in relist() {
            if !union.iter().any(|(p, _)| *p == pid) {
                union.push((pid, name));
            }
        }
    }
    union
}

/// Signals used by [`stop_members`].
#[derive(Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

/// Send one signal to one pid. Errors (e.g. ESRCH: already dead) are
/// swallowed: cleanup is best-effort, never a run failure.
fn send_signal(pid: u32, sig: Signal) {
    #[cfg(unix)]
    {
        let sig = match sig {
            Signal::Term => nix::sys::signal::Signal::SIGTERM,
            Signal::Kill => nix::sys::signal::Signal::SIGKILL,
        };
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), sig);
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, sig);
    }
}

/// Lock a group claim; a poisoned lock still holds a valid claim.
#[cfg(unix)]
fn lock_owner(owner: &Mutex<GroupOwner>) -> std::sync::MutexGuard<'_, GroupOwner> {
    owner.lock().unwrap_or_else(|e| e.into_inner())
}

/// Re-observe a live run's group every [`OBSERVE_INTERVAL`] so members are
/// remembered while it is still provably the run's. Stops once the group
/// is empty or can no longer be proven: neither ever becomes ours again.
#[cfg(unix)]
async fn observe_loop(owner: Arc<Mutex<GroupOwner>>) {
    loop {
        tokio::time::sleep(OBSERVE_INTERVAL).await;
        let o = owner.clone();
        match tokio::task::spawn_blocking(move || lock_owner(&o).observe()).await {
            Ok(Proof::Ours(_)) => {}
            _ => return,
        }
    }
}

/// TERM → grace → KILL for a claimed process group. Every signal is sent
/// by [`GroupOwner::signal`], which re-proves the group from a fresh
/// snapshot first; a group that stops being provable is left alone (and
/// reported), never signalled.
#[cfg(unix)]
fn stop_owned_group(
    run_id: Uuid,
    owner: &Mutex<GroupOwner>,
    grace: Duration,
) -> Vec<(u32, String)> {
    let names = |live: Vec<crate::util::procinfo::ProcInfo>| -> Vec<(u32, String)> {
        live.into_iter().map(|p| (p.pid as u32, p.name)).collect()
    };
    let unproven = |live: &[crate::util::procinfo::ProcInfo], pgid: i32| {
        let names: Vec<&str> = live.iter().map(|p| p.name.as_str()).collect();
        tracing::warn!(
            run_id = %run_id, pgid,
            processes = live.len(),
            names = %names.join(", "),
            "run process group cannot be proven to belong to this run; left running"
        );
    };
    let first = {
        let mut o = lock_owner(owner);
        let pgid = o.pgid();
        match o.observe() {
            Proof::Ours(live) => names(live),
            Proof::Empty => return Vec::new(),
            Proof::Unproven(live) => {
                unproven(&live, pgid);
                return Vec::new();
            }
        }
    };
    stop_members(
        first,
        |_| {
            let mut o = lock_owner(owner);
            let pgid = o.pgid();
            if let Proof::Unproven(live) = o.signal(GroupSignal::Term) {
                unproven(&live, pgid);
            }
        },
        || match lock_owner(owner).observe() {
            Proof::Ours(live) => names(live),
            // Gone, or no longer provable (reported when signalling):
            // nothing this run may still touch.
            Proof::Empty | Proof::Unproven(_) => Vec::new(),
        },
        |_| {
            let mut o = lock_owner(owner);
            let pgid = o.pgid();
            if let Proof::Unproven(live) = o.signal(GroupSignal::Kill) {
                unproven(&live, pgid);
            }
        },
        grace,
    )
}

/// (pid, comm) for every process in a cgroup, from `cgroup.procs`.
fn cgroup_members(dir: &Path) -> Vec<(u32, String)> {
    let Ok(text) = std::fs::read_to_string(dir.join("cgroup.procs")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .map(|pid| {
            let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .map(|s| s.trim_end().to_string())
                .unwrap_or_default();
            (pid, name)
        })
        .collect()
}

/// One log line per run with the count and names of stopped processes.
fn cleanup_report(run_id: Uuid, backend: &str, stopped: &[(u32, String)]) {
    if stopped.is_empty() {
        tracing::info!(run_id = %run_id, backend, "run process cleanup: nothing to stop");
    } else {
        let names: Vec<&str> = stopped.iter().map(|(_, n)| n.as_str()).collect();
        tracing::info!(
            run_id = %run_id,
            backend,
            processes = stopped.len(),
            names = %names.join(", "),
            "run process cleanup stopped {} processes: {}",
            stopped.len(),
            names.join(", ")
        );
    }
}

/// Create the run's unit of containment. On the cgroup backend this
/// mkdirs `pidash-run-<run_id>` under the daemon's own cgroup; a
/// failure (no delegation, cgroup v1, rootful layout) downgrades this
/// run to the process group and warns — never a run failure.
pub fn create_run_group(run_id: Uuid) -> RunGroup {
    let chosen = backend();
    let mut group = RunGroup {
        run_id,
        backend: chosen,
        cgroup_dir: None,
        #[cfg(unix)]
        owner: None,
        #[cfg(unix)]
        observer: None,
        grace: CLEANUP_GRACE,
        workspace: None,
        cleaned: false,
    };
    if chosen == BackendKind::Cgroup {
        let created = self_cgroup_dir()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "no cgroup v2 self path")
            })
            .and_then(|base| {
                let dir = base.join(format!("{CGROUP_PREFIX}{run_id}"));
                std::fs::create_dir_all(&dir).map(|()| dir)
            });
        match created {
            Ok(dir) => group.cgroup_dir = Some(dir),
            Err(e) => {
                // Fall back to the process group for this run too, so
                // something still gets cleaned up at run end.
                tracing::warn!(run_id = %run_id, error = %e, "cgroup creation failed; process-group fallback for this run");
                group.backend = BackendKind::ProcessGroup;
                #[cfg(not(unix))]
                {
                    group.backend = BackendKind::None;
                }
            }
        }
    }
    group
}

/// The daemon's own cgroup directory on cgroup v2, from `/proc/self/cgroup`.
/// `None` on any other topology. Run cgroups are children of this directory,
/// which is what makes the start-up sweep safe (it only ever touches our
/// own children) and what systemd's `KillMode=control-group` reaps when the
/// daemon itself stops.
fn self_cgroup_dir() -> Option<PathBuf> {
    for line in std::fs::read_to_string("/proc/self/cgroup").ok()?.lines() {
        // v2 format: `0::<path>` (no controller list).
        if let Some(path) = line.strip_prefix("0::") {
            let path = path.trim();
            if !path.is_empty() && path != "/" {
                return Some(PathBuf::from(format!("/sys/fs/cgroup{path}")));
            }
        }
    }
    None
}

/// Remove a run cgroup once empty (retry briefly: the kernel reaps
/// members lazily).
async fn remove_cgroup_dir(dir: &Path) {
    for _ in 0..10 {
        if std::fs::remove_dir(dir).is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tracing::debug!(dir = %dir.display(), "run cgroup dir left behind (not empty)");
}

/// Stop every `pidash-run-*` child cgroup of the daemon's own cgroup that
/// a previous daemon left behind, then remove it. Runs at daemon start:
/// no run is live yet, so every such cgroup is orphaned by construction.
/// Each swept group gets the same TERM → grace → KILL treatment and log
/// line as a normal run cleanup. Only the cgroup backend sweeps (see
/// module docs); process-group PGIDs are never persisted across a crash.
pub async fn sweep_orphaned_run_groups() {
    #[cfg(target_os = "linux")]
    {
        if backend() == BackendKind::None {
            return;
        }
        let Some(base) = self_cgroup_dir() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&base) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !name.starts_with(CGROUP_PREFIX) {
                continue;
            }
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            // Parse the run id for the log line; a malformed name still
            // gets swept, the report just says "unknown".
            let run_id = name
                .strip_prefix(CGROUP_PREFIX)
                .and_then(|s| s.parse::<Uuid>().ok())
                .unwrap_or_else(Uuid::nil);
            let stopped = tokio::task::spawn_blocking({
                let dir = dir.clone();
                move || {
                    let members = cgroup_members(&dir);
                    stop_members(
                        members,
                        |pids| {
                            for pid in pids {
                                send_signal(pid, Signal::Term);
                            }
                        },
                        || cgroup_members(&dir),
                        |pids| {
                            for pid in pids {
                                send_signal(pid, Signal::Kill);
                            }
                        },
                        CLEANUP_GRACE,
                    )
                }
            })
            .await
            .unwrap_or_default();
            remove_cgroup_dir(&dir).await;
            cleanup_report(run_id, "cgroup-sweep", &stopped);
        }
    }
}

/// Where a run-end hook lives inside a workspace: a shell script the
/// runner executes (outside the run cgroup) after the run's processes
/// are stopped. `None` outside a git workspace the runner resolves.
pub fn run_end_hook_path(workspace: &Path) -> PathBuf {
    workspace.join(".pidash/run-end-hook")
}

/// Execute the run-end hook for a workspace, if one was written: delete
/// any stale hook from a previous run first (the caller in
/// `AssignWorker` handles that), run with [`HOOK_TIMEOUT`], log the
/// outcome, and delete the file. Missing file is a silent no-op.
/// Must run OUTSIDE the run cgroup (the daemon's own context) so the
/// hook itself is not emptied by the cleanup that precedes it.
pub async fn run_end_hook(workspace: &Path) {
    let hook = run_end_hook_path(workspace);
    if !hook.exists() {
        return;
    }
    tracing::info!(hook = %hook.display(), "executing run-end hook");
    let out = tokio::time::timeout(
        HOOK_TIMEOUT,
        tokio::process::Command::new("bash")
            .arg(&hook)
            .current_dir(workspace)
            .output(),
    )
    .await;
    match out {
        Ok(Ok(o)) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !stdout.trim().is_empty() {
                tracing::info!(hook = %hook.display(), output = %stdout.trim(), "run-end hook output");
            }
            if o.status.success() {
                tracing::info!(hook = %hook.display(), "run-end hook finished");
            } else {
                tracing::warn!(
                    hook = %hook.display(),
                    status = %o.status,
                    output = %stderr.trim(),
                    "run-end hook failed"
                );
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(hook = %hook.display(), error = %e, "run-end hook could not be executed");
        }
        Err(_) => {
            tracing::warn!(hook = %hook.display(), "run-end hook timed out");
        }
    }
    if let Err(e) = std::fs::remove_file(&hook)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::debug!(hook = %hook.display(), error = %e, "run-end hook delete failed");
    }
}

/// Remove a stale run-end hook without running it: a fresh run starts
/// clean, and the sweep at daemon start is responsible for consuming
/// hooks left by a crashed daemon.
pub fn discard_stale_hook(workspace: &Path) {
    let hook = run_end_hook_path(workspace);
    if hook.exists() {
        tracing::info!(hook = %hook.display(), "discarding stale run-end hook at run start");
        let _ = std::fs::remove_file(&hook);
    }
}

/// The containment backend in use, for logs and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Cgroup,
    ProcessGroup,
    None,
}

impl RunGroup {
    /// The backend kind for `RUN_ENV` and logs.
    pub fn backend_kind(&self) -> BackendKind {
        self.backend
    }

    /// Record the run's workspace so cleanup can find its run-end hook.
    pub fn set_workspace(&mut self, workspace: PathBuf) {
        self.workspace = Some(workspace);
    }

    /// The run's workspace, if resolution succeeded.
    pub fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    /// Mark the unit emptied (via [`cleanup`]); the [`Drop`] fallback
    /// stands down on the normal path.
    pub fn mark_cleaned(&mut self) {
        self.cleaned = true;
    }
}

/// Abort/drain fallback: when the task driving a run is aborted (daemon
/// shutdown, `RemoveRunner`) the normal cleanup path never runs, so the
/// drop itself empties the unit. Best-effort — systemd's
/// `KillMode=control-group` reaps the whole tree when the service stops,
/// and the next daemon's sweep recovers anything else. The run-end hook
/// is deliberately NOT run here: a hook file left behind is consumed by
/// the sweep at the next daemon start (or discarded, if that never
/// happens, when a fresh run starts in the same workspace).
impl Drop for RunGroup {
    fn drop(&mut self) {
        if self.cleaned || self.backend == BackendKind::None {
            return;
        }
        #[cfg(unix)]
        if let Some(observer) = self.observer.take() {
            observer.abort();
        }
        let detached = RunGroup {
            run_id: self.run_id,
            backend: self.backend,
            cgroup_dir: std::mem::take(&mut self.cgroup_dir),
            #[cfg(unix)]
            owner: self.owner.take(),
            #[cfg(unix)]
            observer: None,
            grace: self.grace,
            workspace: None,
            cleaned: true,
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move { detached.cleanup().await });
            }
            Err(_) => {
                // The runtime is already gone: the detached cleanup runs
                // on its own thread with its own minimal runtime.
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    if let Ok(rt) = rt {
                        rt.block_on(detached.cleanup());
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_path_is_workspace_pidash_dir() {
        let ws = Path::new("/tmp/ws");
        assert_eq!(
            run_end_hook_path(ws),
            PathBuf::from("/tmp/ws/.pidash/run-end-hook")
        );
    }

    #[test]
    fn stop_members_empty_is_noop() {
        let stopped = stop_members(
            Vec::new(),
            |_| panic!("term must not be called"),
            || panic!("relist must not be called"),
            |_| panic!("kill must not be called"),
            Duration::from_millis(10),
        );
        assert!(stopped.is_empty());
    }

    #[test]
    fn stop_members_terms_then_kills_stragglers() {
        let mut re_lists = 0usize;
        let stopped = stop_members(
            vec![(100, "a".to_string())],
            |pids| assert_eq!(pids, vec![100]),
            || {
                re_lists += 1;
                if re_lists == 1 {
                    vec![(100, "a".to_string())]
                } else {
                    Vec::new()
                }
            },
            |_pids| {},
            Duration::from_secs(5),
        );
        // The process exited during the grace poll, so no kill was
        // needed — but the per-manual account must still name it.
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].0, 100);
    }

    #[test]
    fn stop_members_kills_processes_present_after_grace() {
        let mut re_lists = 0usize;
        let mut killed: Vec<u32> = Vec::new();
        let stopped = stop_members(
            vec![(100, "a".to_string())],
            |_| {},
            || {
                re_lists += 1;
                // Never empties: straggler stays for the kill.
                vec![(100, "a".to_string())]
            },
            |pids| killed = pids,
            Duration::from_millis(50),
        );
        assert!(killed.contains(&100));
        assert!(!stopped.is_empty());
        let _ = re_lists;
    }

    #[test]
    fn cgroup_members_empty_for_missing_dir() {
        assert!(cgroup_members(Path::new("/nonexistent-pidash-test-dir")).is_empty());
    }

    /// A run group on the process-group backend with a short grace.
    #[cfg(unix)]
    fn process_group_run() -> RunGroup {
        let mut group = create_run_group(Uuid::new_v4());
        group.backend = BackendKind::ProcessGroup;
        group.cgroup_dir = None;
        group.grace = Duration::from_millis(300);
        group
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn alive(pid: i32) -> bool {
        crate::util::procinfo::info(pid).is_some_and(|p| !p.zombie)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn process_group_cleanup_leaves_an_unproven_group_running() {
        use std::os::unix::process::CommandExt;
        // An unrelated group now holds the pgid the run recorded: its
        // leader has the recorded pid but not the recorded start time.
        let mut child = std::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let stranger = crate::util::procinfo::info(pid).unwrap();
        let recorded = crate::util::procinfo::ProcInfo {
            start: stranger.start - 1,
            ..stranger
        };
        let mut group = process_group_run();
        group.set_owner(GroupOwner::claim(&recorded).unwrap());
        group.cleanup().await;
        group.mark_cleaned();
        let survived = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(survived, "an unproven process group was signalled");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn process_group_cleanup_stops_members_that_outlive_the_leader() {
        use std::io::BufRead;
        use std::os::unix::process::CommandExt;
        // The leader starts a background child in its group, then dies and
        // is reaped; the observer must have learned the child for cleanup
        // to prove (and stop) the group without its leader.
        let mut leader = std::process::Command::new("sh")
            .args(["-c", "sleep 300 & echo $!; wait"])
            .process_group(0)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(leader.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let orphan: i32 = line.trim().parse().unwrap();
        let mut group = process_group_run();
        group.move_pid(Some(leader.id()));
        let owner = group.owner.clone().expect("leader claimed");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !lock_owner(&owner).knows_pid(orphan) {
            assert!(
                std::time::Instant::now() < deadline,
                "observer never learned the child"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        leader.kill().unwrap();
        leader.wait().unwrap();
        assert!(alive(orphan));
        group.cleanup().await;
        group.mark_cleaned();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while alive(orphan) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            !alive(orphan),
            "member outliving the leader was left running"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_group_without_a_claim_signals_nothing() {
        let mut group = process_group_run();
        // The leader exited before it could be read: no claim, no signal.
        group.move_pid(Some(u32::MAX));
        assert!(group.owner.is_none());
        assert!(group.stop_all().await.is_empty());
        group.mark_cleaned();
    }
}
