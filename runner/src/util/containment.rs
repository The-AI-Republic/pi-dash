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
//!   Cleanup reads `cgroup.procs` of the run cgroup and every cgroup nested
//!   in it, SIGTERMs every member, waits the grace period, then SIGKILLs
//!   what remains; the run is clean only once `cgroup.events` reports the
//!   whole subtree unpopulated. The start-up sweep finds
//!   `pidash-run-*` children the previous daemon left behind.
//! * **process group** (elsewhere Unix): when the run starts, the daemon
//!   creates the run's process group, led by an anchor process it keeps
//!   unreaped until cleanup ends, and every agent of the run is spawned
//!   into that group (`process_group(pgid)`). Cleanup signals the group
//!   only while a fresh snapshot proves it is still the run's (see
//!   [`crate::util::group_ownership`]); the anchor reserves the group id,
//!   so it cannot be handed to an unrelated group in between. When no
//!   anchor can be spawned, each agent leads a group of its own, which is
//!   never signalled: an unpinned group is left running rather than risk
//!   killing a stranger. A child that calls `setsid` escapes the group — a
//!   documented limit of this backend. Platforms without process
//!   inspection (not Linux or macOS) could never prove a group empty, so
//!   they get no containment (as if `off`), not a runner held forever.
//!   After a crash, macOS recovers the run's group from a record the
//!   daemon wrote when it created it: the anchor outlives the daemon and
//!   still pins the group, and the next daemon stops the group only once
//!   it has proven it from the anchor's exact identity (see
//!   [`crate::util::group_recovery`]). Elsewhere a dead PGID can be reused
//!   by the OS, so swept recovery only exists on the cgroup backend.
//! * **none**: `PIDASH_CONTAINMENT=off` (diagnosis escape hatch) or
//!   Windows, where this module only ensures `kill_on_drop`. The old
//!   leak applies; the override is never recommended.
//!
//! Ordering (PDASHOSS01-417). Cleanup runs before the run's terminal
//! report is sent, so the cloud never sees a run finished (and offers the
//! runner new work) while its processes are still being stopped.
//! [`RunGroup::cleanup`] reports [`CleanupOutcome::Unresolved`] when
//! processes survive SIGKILL or live processes hold the run's group without
//! proof of ownership; the runner then stays reserved, re-checking, until
//! they are gone. The run's report is still sent: the run itself is over.
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
/// Prefix of the cgroups the sweep looks for under the daemon's cgroup.
const CGROUP_PREFIX: &str = "pidash-run-";

fn backend() -> BackendKind {
    static BACKEND: OnceLock<BackendKind> = OnceLock::new();
    *BACKEND.get_or_init(|| {
        let chosen = std::env::var("PIDASH_CONTAINMENT").unwrap_or_else(|_| "auto".to_string());
        let kind = match chosen.to_ascii_lowercase().as_str() {
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
        };
        let usable = usable_backend(kind, crate::util::procinfo::inspection_usable());
        if usable != kind {
            tracing::warn!(
                "process-group containment needs process inspection, which is unavailable here (unsupported platform, or /proc mounted with hidepid); run processes are not contained"
            );
        }
        usable
    })
}

/// Whether run containment is on (not `PIDASH_CONTAINMENT=off`, and a
/// usable backend exists here). Startup recovery signals nothing when off.
pub fn enabled() -> bool {
    backend() != BackendKind::None
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

/// `kind`, or no containment when it cannot work here.
/// The process-group backend can only release a run once a group is
/// proven empty, which needs process inspection ([`crate::util::procinfo`]);
/// without it every run would hold its runner forever.
fn usable_backend(kind: BackendKind, inspectable: bool) -> BackendKind {
    if kind == BackendKind::ProcessGroup && !inspectable {
        return BackendKind::None;
    }
    kind
}

/// What run-end cleanup established about the run's processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupOutcome {
    /// Nothing of the run is left running (or the backend tracks nothing).
    Clean,
    /// Processes are still alive in the run's unit, or live processes hold
    /// its process group and cannot be proven to be the run's. The runner
    /// must stay reserved: they may still be using the working copy.
    Unresolved,
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
    /// Process-group backend: the agent's own group when the run has no
    /// anchored group. Never signalled; only checked for live members, so
    /// an unknown group is reported as unresolved rather than as clean.
    #[cfg(unix)]
    unclaimed_pgid: Option<i32>,
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
    /// On the process-group backend there is nothing to do: the agent was
    /// spawned into the run's anchored group (see [`Self::agent_pgid`]), so
    /// the claim never depends on reading a pid the bridge may already have
    /// reaped. Without an anchored group the agent led a group of its own,
    /// which is never signalled.
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
            BackendKind::ProcessGroup =>
            {
                #[cfg(unix)]
                if self.agent_pgid().is_none() {
                    let Ok(pid) = i32::try_from(pid) else { return };
                    self.unclaimed_pgid = Some(pid);
                    tracing::warn!(
                        run_id = %self.run_id, pid,
                        "run has no pinned process group; its processes will not be signalled at run end"
                    );
                }
            }
            BackendKind::None => {}
        }
    }

    /// Process group the run's agents must join (`process_group(pgid)`):
    /// the run's anchored group on the process-group backend. `None`: each
    /// agent leads a group of its own (never signalled).
    pub fn agent_pgid(&self) -> Option<i32> {
        #[cfg(unix)]
        {
            let owner = self.owner.as_ref()?;
            let o = lock_owner(owner);
            o.is_anchored().then(|| o.pgid())
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Empty the unit: SIGTERM everything, wait [`CLEANUP_GRACE`], then
    /// SIGKILL what remains. Logs exactly one line per run with the count
    /// and the names of the processes stopped (see [`cleanup_report`]).
    /// Safe to call again: a later call re-proves and re-checks the unit,
    /// which is how a caller waits out an [`CleanupOutcome::Unresolved`].
    pub async fn cleanup(&self) -> CleanupOutcome {
        let (stopped, outcome) = self.stop_all().await;
        cleanup_report(self.run_id, self.backend_name(), &stopped);
        outcome
    }

    /// SIGTERM the contents of the unit, wait the grace, SIGKILL the
    /// rest. Returns the processes that were stopped (pid + name) and
    /// whether anything is left.
    async fn stop_all(&self) -> (Vec<(u32, String)>, CleanupOutcome) {
        match self.backend {
            BackendKind::Cgroup => {
                let Some(dir) = &self.cgroup_dir else {
                    return (Vec::new(), CleanupOutcome::Clean);
                };
                let grace = self.grace;
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
                            grace,
                        )
                    }
                })
                .await
                .unwrap_or_default();
                // Clean only when the kernel reports the whole subtree
                // unpopulated: a member list can miss processes it could
                // not read.
                if cgroup_populated(dir) == Some(false) {
                    remove_cgroup_dir(dir).await;
                    (stopped, CleanupOutcome::Clean)
                } else {
                    let left = cgroup_members(dir);
                    let names: Vec<&str> = left.iter().map(|(_, n)| n.as_str()).collect();
                    tracing::warn!(
                        run_id = %self.run_id,
                        processes = left.len(),
                        names = %names.join(", "),
                        "run cgroup is still populated (or unreadable) after SIGKILL"
                    );
                    (stopped, CleanupOutcome::Unresolved)
                }
            }
            BackendKind::ProcessGroup => {
                #[cfg(unix)]
                {
                    let Some(owner) = self.owner.clone() else {
                        return (
                            Vec::new(),
                            unclaimed_group_outcome(self.run_id, self.unclaimed_pgid),
                        );
                    };
                    let (run_id, grace) = (self.run_id, self.grace);
                    tokio::task::spawn_blocking(move || stop_owned_group(run_id, &owner, grace))
                        .await
                        .unwrap_or((Vec::new(), CleanupOutcome::Unresolved))
                }
                #[cfg(not(unix))]
                {
                    (Vec::new(), CleanupOutcome::Clean)
                }
            }
            BackendKind::None => (Vec::new(), CleanupOutcome::Clean),
        }
    }
}

/// Outcome for a process group whose leader was never claimed: it is never
/// signalled, but live members in it mean the run may have left processes
/// behind, so the outcome is unresolved until the group is empty.
#[cfg(unix)]
fn unclaimed_group_outcome(run_id: Uuid, pgid: Option<i32>) -> CleanupOutcome {
    let Some(pgid) = pgid else {
        return CleanupOutcome::Clean;
    };
    let Ok(members) = crate::util::procinfo::group_members(pgid) else {
        tracing::warn!(
            run_id = %run_id, pgid,
            "unclaimed run process group could not be inspected; left running"
        );
        return CleanupOutcome::Unresolved;
    };
    let live: Vec<_> = members.into_iter().filter(|p| !p.zombie).collect();
    if live.is_empty() {
        // Only a scan no process creation raced may call the group empty.
        return match crate::util::procinfo::group_members_settled(pgid) {
            Ok(members) if members.iter().all(|p| p.zombie) => CleanupOutcome::Clean,
            _ => CleanupOutcome::Unresolved,
        };
    }
    tracing::warn!(
        run_id = %run_id, pgid,
        processes = live.len(),
        "unclaimed run process group still has live processes; left running"
    );
    CleanupOutcome::Unresolved
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
pub(crate) fn lock_owner(owner: &Mutex<GroupOwner>) -> std::sync::MutexGuard<'_, GroupOwner> {
    owner.lock().unwrap_or_else(|e| e.into_inner())
}

/// One relist of the TERM → KILL grace poll for a claimed group: the live
/// members to keep waiting on (and KILL if they remain). A group that
/// cannot be inspected or proven right now repeats the last proven list,
/// so the grace is not cut short and the KILL step still runs (it re-proves
/// the group before signalling); a lost proof is recorded in `lost` so the
/// processes left behind are reported, not counted as stopped.
#[cfg(unix)]
fn grace_relist(
    proof: Proof,
    last_known: &mut Vec<(u32, String)>,
    lost: &mut Option<Vec<crate::util::procinfo::ProcInfo>>,
) -> Vec<(u32, String)> {
    match proof {
        Proof::Ours(live) => {
            *last_known = live.into_iter().map(|p| (p.pid as u32, p.name)).collect();
            last_known.clone()
        }
        Proof::Unknown => last_known.clone(),
        Proof::Unproven(live) => {
            *lost = Some(live);
            last_known.clone()
        }
        Proof::Empty => Vec::new(),
    }
}

/// Attempts at a first cleanup snapshot when the group cannot be inspected.
#[cfg(unix)]
const INSPECT_ATTEMPTS: usize = 3;

/// TERM → grace → KILL for a claimed process group. Every signal is sent
/// by [`GroupOwner::signal`], which re-proves the group from a fresh
/// snapshot first; a group that stops being provable is left alone (and
/// reported), never signalled.
#[cfg(unix)]
pub(crate) fn stop_owned_group(
    run_id: Uuid,
    owner: &Mutex<GroupOwner>,
    grace: Duration,
) -> (Vec<(u32, String)>, CleanupOutcome) {
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
    let uninspectable = |pgid: i32| {
        tracing::warn!(
            run_id = %run_id, pgid,
            "run process group could not be inspected; left running"
        );
    };
    let mut first = None;
    for attempt in 0..INSPECT_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(100));
        }
        let mut o = lock_owner(owner);
        let pgid = o.pgid();
        match o.observe() {
            Proof::Ours(live) => {
                first = Some(names(live));
                break;
            }
            Proof::Empty => return (Vec::new(), CleanupOutcome::Clean),
            Proof::Unproven(live) => {
                unproven(&live, pgid);
                return (Vec::new(), CleanupOutcome::Unresolved);
            }
            Proof::Unknown => {}
        }
    }
    let Some(first) = first else {
        uninspectable(lock_owner(owner).pgid());
        return (Vec::new(), CleanupOutcome::Unresolved);
    };
    let mut last_known = first.clone();
    let mut lost = None;
    let stopped = stop_members(
        first,
        |_| {
            let mut o = lock_owner(owner);
            let pgid = o.pgid();
            match o.signal(GroupSignal::Term) {
                Proof::Unproven(live) => unproven(&live, pgid),
                Proof::Unknown => uninspectable(pgid),
                Proof::Ours(_) | Proof::Empty => {}
            }
        },
        || grace_relist(lock_owner(owner).observe(), &mut last_known, &mut lost),
        |_| {
            let mut o = lock_owner(owner);
            let pgid = o.pgid();
            match o.signal(GroupSignal::Kill) {
                Proof::Unproven(live) => unproven(&live, pgid),
                Proof::Unknown => uninspectable(pgid),
                Proof::Ours(_) | Proof::Empty => {}
            }
        },
        grace,
    );
    // Proof lost during the grace: what is left is reported as left
    // running, never as stopped by this run.
    if let Some(live) = &lost {
        unproven(live, lock_owner(owner).pgid());
    }
    // Clean only when a fresh snapshot shows the group empty: survivors of
    // SIGKILL, an unprovable group and a group that cannot be inspected
    // all leave the run unresolved.
    let outcome = match lock_owner(owner).observe() {
        Proof::Empty => CleanupOutcome::Clean,
        Proof::Ours(_) | Proof::Unproven(_) | Proof::Unknown => CleanupOutcome::Unresolved,
    };
    (stopped, outcome)
}

/// (pid, comm) for every process in a cgroup and its nested cgroups, from
/// each `cgroup.procs`. Everything in the subtree belongs to the run.
fn cgroup_members(dir: &Path) -> Vec<(u32, String)> {
    let mut members = Vec::new();
    collect_cgroup_members(dir, &mut members);
    members
}

fn collect_cgroup_members(dir: &Path, members: &mut Vec<(u32, String)>) {
    if let Ok(text) = std::fs::read_to_string(dir.join("cgroup.procs")) {
        members.extend(
            text.lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
                .map(|pid| {
                    let name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                        .map(|s| s.trim_end().to_string())
                        .unwrap_or_default();
                    (pid, name)
                }),
        );
    }
    for sub in nested_cgroups(dir) {
        collect_cgroup_members(&sub, members);
    }
}

/// Child cgroup directories of `dir`.
fn nested_cgroups(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the cgroup's subtree still holds any process, from the
/// `populated` key of `cgroup.events`. `None` when it cannot be read.
fn cgroup_populated(dir: &Path) -> Option<bool> {
    let events = std::fs::read_to_string(dir.join("cgroup.events")).ok()?;
    events.lines().find_map(|line| match line.split_once(' ')? {
        ("populated", "0") => Some(false),
        ("populated", "1") => Some(true),
        _ => None,
    })
}

/// One log line per run with the count and names of stopped processes.
pub(crate) fn cleanup_report(run_id: Uuid, backend: &str, stopped: &[(u32, String)]) {
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
    create_run_group_for(run_id, None)
}

/// [`create_run_group`] for a unit of `runner_id`'s, so a crash-recovery
/// record left by it reserves only that runner.
pub fn create_run_group_for(run_id: Uuid, runner_id: Option<Uuid>) -> RunGroup {
    let chosen = backend();
    let mut group = match chosen {
        BackendKind::ProcessGroup => process_group_unit(run_id, runner_id),
        other => bare_group(run_id, other),
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
                group = match usable_backend(
                    BackendKind::ProcessGroup,
                    crate::util::procinfo::inspection_usable(),
                ) {
                    BackendKind::ProcessGroup => process_group_unit(run_id, runner_id),
                    other => bare_group(run_id, other),
                };
                #[cfg(not(unix))]
                {
                    group.backend = BackendKind::None;
                }
            }
        }
    }
    group
}

/// A run unit on `backend` that holds nothing yet: no cgroup is created.
fn bare_group(run_id: Uuid, backend: BackendKind) -> RunGroup {
    RunGroup {
        run_id,
        backend,
        cgroup_dir: None,
        #[cfg(unix)]
        owner: None,
        #[cfg(unix)]
        unclaimed_pgid: None,
        grace: CLEANUP_GRACE,
        workspace: None,
        cleaned: false,
    }
}

/// A process-group run unit, with its anchored group created up front so
/// agents can be spawned into it.
fn process_group_unit(run_id: Uuid, runner_id: Option<Uuid>) -> RunGroup {
    let mut group = bare_group(run_id, BackendKind::ProcessGroup);
    #[cfg(unix)]
    {
        match GroupOwner::new_anchored() {
            Some(mut owner) => {
                crate::util::group_recovery::record_new_group(run_id, runner_id, &mut owner);
                group.owner = Some(Arc::new(Mutex::new(owner)));
            }
            None => tracing::warn!(
                run_id = %run_id,
                "run process group could not be anchored; its processes will not be signalled at run end"
            ),
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
    // Nested cgroups first: a cgroup with children cannot be removed.
    for sub in nested_cgroups(dir) {
        Box::pin(remove_cgroup_dir(&sub)).await;
    }
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
/// line as a normal run cleanup. Only the cgroup backend sweeps here (see
/// module docs); process groups are recovered from their records by
/// [`crate::util::group_recovery::recover_orphaned_groups`] (macOS).
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
    /// The unit's id (names its cgroup and recovery record).
    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    /// Cgroup backend: the unit cgroup's `cgroup.procs`.
    pub fn cgroup_procs(&self) -> Option<PathBuf> {
        self.cgroup_dir.as_ref().map(|d| d.join("cgroup.procs"))
    }

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

#[cfg(all(test, unix))]
impl RunGroup {
    /// A process-group run claiming `leader` as recorded, with a short
    /// grace: tests use it to stand in for a group that a stranger reused.
    pub(crate) fn claimed_for_test(leader: &crate::util::procinfo::ProcInfo) -> Self {
        let mut group = bare_group(Uuid::new_v4(), BackendKind::ProcessGroup);
        group.grace = Duration::from_millis(300);
        group.owner = GroupOwner::claim(leader).map(|o| Arc::new(Mutex::new(o)));
        group
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
        let detached = RunGroup {
            run_id: self.run_id,
            backend: self.backend,
            cgroup_dir: std::mem::take(&mut self.cgroup_dir),
            #[cfg(unix)]
            owner: self.owner.take(),
            #[cfg(unix)]
            unclaimed_pgid: self.unclaimed_pgid,
            grace: self.grace,
            workspace: None,
            cleaned: true,
        };
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    detached.cleanup().await;
                });
            }
            Err(_) => {
                // The runtime is already gone: the detached cleanup runs
                // on its own thread with its own minimal runtime.
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    if let Ok(rt) = rt {
                        rt.block_on(async {
                            detached.cleanup().await;
                        });
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

    /// A cgroup-backend run over `dir` (a stand-in cgroup directory).
    fn cgroup_run(dir: &Path) -> RunGroup {
        let mut group = bare_group(Uuid::new_v4(), BackendKind::Cgroup);
        group.cgroup_dir = Some(dir.to_path_buf());
        group.grace = Duration::from_millis(300);
        group.cleaned = true;
        group
    }

    /// Review item 4 on #3499: `cgroup.procs` lists only the run cgroup's
    /// own members. Processes in a nested cgroup keep the subtree
    /// populated, so cleanup must not report the run clean.
    #[tokio::test]
    async fn a_populated_cgroup_subtree_leaves_cleanup_unresolved() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cgroup.procs"), "").unwrap();
        std::fs::write(dir.path().join("cgroup.events"), "populated 1\nfrozen 0\n").unwrap();
        let group = cgroup_run(dir.path());
        assert_eq!(group.cleanup().await, CleanupOutcome::Unresolved);
    }

    #[tokio::test]
    async fn an_unpopulated_cgroup_is_clean() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cgroup.procs"), "").unwrap();
        std::fs::write(dir.path().join("cgroup.events"), "populated 0\nfrozen 0\n").unwrap();
        let group = cgroup_run(dir.path());
        assert_eq!(group.cleanup().await, CleanupOutcome::Clean);
    }

    /// A cgroup whose population cannot be read is not known to be empty.
    #[tokio::test]
    async fn a_cgroup_whose_events_cannot_be_read_is_unresolved() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cgroup.procs"), "").unwrap();
        let group = cgroup_run(dir.path());
        assert_eq!(group.cleanup().await, CleanupOutcome::Unresolved);
    }

    /// Members of a nested cgroup belong to the run too and are stopped.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn cgroup_cleanup_stops_members_of_nested_cgroups() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("300")
            .spawn()
            .unwrap();
        std::fs::write(dir.path().join("cgroup.procs"), "").unwrap();
        std::fs::write(nested.join("cgroup.procs"), format!("{}\n", child.id())).unwrap();
        std::fs::write(dir.path().join("cgroup.events"), "populated 1\n").unwrap();
        let group = cgroup_run(dir.path());
        let outcome = group.cleanup().await;
        let status = child.try_wait().unwrap();
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            status.is_some(),
            "a nested cgroup's member was left running"
        );
        assert_eq!(outcome, CleanupOutcome::Unresolved);
    }

    /// Review item 5 on #3499: where processes cannot be inspected, the
    /// process-group backend could never prove a group empty and would hold
    /// the runner forever, so such platforms get no containment instead.
    #[test]
    fn process_group_backend_needs_process_inspection() {
        assert_eq!(
            usable_backend(BackendKind::ProcessGroup, false),
            BackendKind::None
        );
        assert_eq!(
            usable_backend(BackendKind::ProcessGroup, true),
            BackendKind::ProcessGroup
        );
        assert_eq!(
            usable_backend(BackendKind::Cgroup, false),
            BackendKind::Cgroup
        );
        assert_eq!(usable_backend(BackendKind::None, true), BackendKind::None);
    }

    /// A run group on the process-group backend with a short grace.
    #[cfg(unix)]
    fn process_group_run() -> RunGroup {
        let mut group = process_group_unit(Uuid::new_v4(), None);
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
        group.owner = Some(Arc::new(Mutex::new(GroupOwner::claim(&recorded).unwrap())));
        let outcome = group.cleanup().await;
        group.mark_cleaned();
        let survived = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(survived, "an unproven process group was signalled");
        // Live processes the run cannot account for keep it unresolved.
        assert_eq!(outcome, CleanupOutcome::Unresolved);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn process_group_cleanup_stops_a_child_whose_leader_was_reaped_unobserved() {
        use std::io::BufRead;
        use std::os::unix::process::CommandExt;
        // Review items (#3497, both reviews): the agent starts its only
        // surviving child and is reaped before the run could read it, as
        // when the bridge's wait task reaps it first. The group must still
        // be provably the run's at cleanup, and the child stopped.
        let mut group = process_group_run();
        let mut leader = std::process::Command::new("sh")
            .args(["-c", "sleep 300 & echo $!"])
            .process_group(group.agent_pgid().unwrap_or(0))
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(leader.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let orphan: i32 = line.trim().parse().unwrap();
        leader.wait().unwrap();
        group.move_pid(Some(leader.id()));
        assert!(alive(orphan));
        let outcome = group.cleanup().await;
        group.mark_cleaned();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while alive(orphan) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let survived = alive(orphan);
        if survived {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(orphan),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        assert!(!survived, "child of a reaped leader was left running");
        assert_eq!(outcome, CleanupOutcome::Clean);
    }

    /// Second review of #3497, item 2: without an anchor nothing keeps the
    /// group id reserved between the last proof and `killpg`, so an
    /// unpinned group is never signalled, even when it is provably ours.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_unpinned_group_is_never_signalled() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let info = crate::util::procinfo::info(child.id() as i32).unwrap();
        let mut group = process_group_run();
        group.owner = Some(Arc::new(Mutex::new(GroupOwner::claim(&info).unwrap())));
        let outcome = group.cleanup().await;
        group.mark_cleaned();
        let survived = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(survived, "an unpinned process group was signalled");
        assert_eq!(outcome, CleanupOutcome::Unresolved);
    }

    /// Second review of #3497, item 3: losing proof during the grace must
    /// not read as "all stopped". The poll keeps the last proven list, so
    /// the KILL step still runs (and re-proves before signalling), and the
    /// processes left behind are recorded for the report.
    #[cfg(unix)]
    #[test]
    fn losing_proof_during_the_grace_is_reported_not_read_as_empty() {
        let stranger = crate::util::procinfo::ProcInfo {
            pid: 42,
            pgid: 10,
            start: 1,
            zombie: false,
            name: "survivor".into(),
        };
        let mut last_known = vec![(11, "agent".to_string())];
        let mut lost = None;
        let relisted = grace_relist(
            Proof::Unproven(vec![stranger.clone()]),
            &mut last_known,
            &mut lost,
        );
        assert_eq!(relisted, vec![(11, "agent".to_string())]);
        assert_eq!(lost, Some(vec![stranger]));
        assert!(grace_relist(Proof::Empty, &mut last_known, &mut lost).is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_group_without_a_claim_signals_nothing() {
        let mut group = bare_group(Uuid::new_v4(), BackendKind::ProcessGroup);
        // No anchored group could be created: no claim, no signal.
        group.move_pid(Some(u32::MAX));
        assert!(group.owner.is_none());
        assert_eq!(group.stop_all().await, (Vec::new(), CleanupOutcome::Clean));
        group.mark_cleaned();
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn unclaimed_group_with_live_members_is_unresolved_and_left_running() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sleep")
            .arg("300")
            .process_group(0)
            .spawn()
            .unwrap();
        let mut group = bare_group(Uuid::new_v4(), BackendKind::ProcessGroup);
        group.grace = Duration::from_millis(300);
        // No anchored group: the agent led its own, which has members.
        group.unclaimed_pgid = Some(child.id() as i32);
        let outcome = group.cleanup().await;
        group.mark_cleaned();
        let survived = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let _ = child.wait();
        assert!(survived, "an unclaimed process group was signalled");
        assert_eq!(outcome, CleanupOutcome::Unresolved);
        // Once the group is gone the same run cleans up.
        assert_eq!(group.cleanup().await, CleanupOutcome::Clean);
    }
}
