//! Process ownership for agent subprocesses.
//!
//! Every agent is spawned as the leader of a new session (`setsid`, see
//! [`crate::util::shell`]), so everything it starts shares its process group
//! unless it deliberately leaves. A [`ProcessTree`] owns that group: it writes
//! a durable record before the spawn, fills in the pgid after, and on
//! [`ProcessTree::finish_group`] terminates whatever is still in the group
//! (SIGTERM, then SIGKILL) before deleting the record. A record that could not
//! be resolved survives, and the next daemon start resolves it in
//! [`recover`]. Design: `.ai_design/runner_cleanup/design.md` §4.1, §4.3.
//!
//! Safety rules, enforced here and nowhere else:
//! * a pgid is signalled only while this owner has observed it alive and not
//!   yet observed it gone; after ESRCH it is never signalled again;
//! * EPERM means "could not clean", never "empty" (unless every member is a
//!   zombie, which is how macOS reports a dead, unreaped group);
//! * crash recovery acts only on records of this daemon's instance id, and
//!   only on a group proven ours (see [`group_identity`]): a member carries
//!   our tag (Linux), or the recorded leader is still alive with its recorded
//!   start time, or the group is leaderless and every member started inside
//!   the owner's lifetime (at or after the leader, at or before the owning
//!   daemon's last-alive mark, `data_dir/proc/alive`).
//!
//! Windows keeps today's leader-only behaviour behind the same API: no group
//! exists, so every group operation is a no-op there.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock, Weak};
use std::time::Duration;
use uuid::Uuid;

/// Environment variable carrying the per-process ownership tag.
pub const PROC_TAG_ENV: &str = "PIDASH_PROC_TAG";

/// How long a group gets to exit after SIGTERM.
pub const TERM_WAIT: Duration = Duration::from_secs(3);
/// How long a group gets to disappear after SIGKILL.
pub const KILL_WAIT: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(50);
/// How far two readings of one process's start time may differ and still be
/// the same process. Linux derives start times from `btime` in `/proc/stat`,
/// which can jitter by about a second between daemons; macOS start times are
/// exact. A pid cannot be freed and handed to a new process within this
/// window in practice.
pub const START_TOLERANCE_MS: u64 = 2_000;
/// How often the instance-lock holder rewrites `data_dir/proc/alive`.
pub const ALIVE_INTERVAL: Duration = Duration::from_secs(5);
/// File holding the lock holder's last-alive mark, beside the records.
pub const ALIVE_FILE: &str = "alive";

/// What a tree belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    Run,
    Chat,
}

impl ScopeKind {
    fn as_str(self) -> &'static str {
        match self {
            ScopeKind::Run => "run",
            ScopeKind::Chat => "chat",
        }
    }
}

/// The run or chat runtime an agent is spawned for. Set as a task-local by the
/// supervisor around a run (`AssignWorker`) and a chat runtime (`ChatWorker`,
/// local IPC chat), so every bridge reaches it without new parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcScope {
    pub kind: ScopeKind,
    pub id: String,
    pub runner_id: Option<String>,
}

tokio::task_local! {
    pub static PROC_SCOPE: ProcScope;
}

/// Daemon-wide context: where records live and whose they are. Installed by
/// the daemon only after it holds the exclusive instance lock; without it
/// trees still clean their group, but write no records and set no tag.
#[derive(Debug, Clone)]
pub struct ProcContext {
    pub instance_id: String,
    pub proc_dir: PathBuf,
    /// Whether ownership checks may use environment reads (the tag). Defaults
    /// to [`crate::util::procinfo::ENV_OWNERSHIP_PROOF`]; tests turn it off to
    /// exercise the macOS start-time proof on Linux.
    pub env_proof: bool,
}

impl ProcContext {
    pub fn new(instance_id: impl Into<String>, data_dir: &Path) -> Self {
        Self {
            instance_id: instance_id.into(),
            proc_dir: data_dir.join("proc"),
            env_proof: crate::util::procinfo::ENV_OWNERSHIP_PROOF,
        }
    }

    /// `<instance_id>:<run|chat>:<id>`.
    pub fn tag_for(&self, scope: &ProcScope) -> String {
        format!("{}:{}:{}", self.instance_id, scope.kind.as_str(), scope.id)
    }
}

static CONTEXT: RwLock<Option<Arc<ProcContext>>> = RwLock::new(None);

/// Install the daemon-wide context (daemon start, after the instance lock).
pub fn install_context(ctx: ProcContext) {
    if let Ok(mut g) = CONTEXT.write() {
        *g = Some(Arc::new(ctx));
    }
}

fn current_context() -> Option<Arc<ProcContext>> {
    CONTEXT.read().ok().and_then(|g| g.clone())
}

/// On-disk record: `data_dir/proc/<record_id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcRecord {
    pub record_id: Uuid,
    pub instance_id: String,
    pub runner_id: Option<String>,
    pub kind: ScopeKind,
    pub run_or_chat_id: String,
    pub tag: String,
    /// Per-run scratch folder (runner cleanup PR 2); always `None` for now.
    pub scratch: Option<PathBuf>,
    /// Filled in after the spawn. `None` means the daemon may have crashed
    /// between writing the record and spawning.
    pub pgid: Option<i32>,
    pub leader_start_time_ms: Option<u64>,
    /// Upper bound of the owning daemon's lifetime, stamped by the first
    /// recovery pass that keeps this record (the crashed daemon's last-alive
    /// mark; `0` when there was none). Later passes reuse it, so a newer
    /// daemon's lifetime never widens the window a leaderless group is
    /// checked against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_alive_until_ms: Option<u64>,
}

fn record_path(dir: &Path, id: Uuid) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Write `bytes` to `dir/name` atomically: tmp file, fsync, rename, fsync the
/// directory.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(path)
}

/// Write `rec` atomically (see [`write_atomic`]).
pub fn write_record(dir: &Path, rec: &ProcRecord) -> std::io::Result<PathBuf> {
    let bytes = serde_json::to_vec(rec).map_err(std::io::Error::other)?;
    write_atomic(dir, &format!("{}.json", rec.record_id), &bytes)
}

/// Wall-clock milliseconds since the Unix epoch (the unit of process start
/// times in [`crate::util::procinfo`]).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
struct AliveMark {
    instance_id: String,
    alive_ms: u64,
}

/// Record that this instance was alive at `at_ms` (`dir/alive`, atomic). The
/// instance-lock holder rewrites it every [`ALIVE_INTERVAL`]; after a crash it
/// bounds which leaderless groups can still be ours.
pub fn write_alive(dir: &Path, instance_id: &str, at_ms: u64) -> std::io::Result<()> {
    let mark = AliveMark {
        instance_id: instance_id.to_string(),
        alive_ms: at_ms,
    };
    let bytes = serde_json::to_vec(&mark).map_err(std::io::Error::other)?;
    write_atomic(dir, ALIVE_FILE, &bytes).map(|_| ())
}

/// The last-alive mark `instance_id` left in `dir`, if any.
pub fn read_alive(dir: &Path, instance_id: &str) -> Option<u64> {
    let bytes = std::fs::read(dir.join(ALIVE_FILE)).ok()?;
    let mark: AliveMark = serde_json::from_slice(&bytes).ok()?;
    (mark.instance_id == instance_id).then_some(mark.alive_ms)
}

/// All parseable records in `dir` whose instance id is `instance_id`.
/// Records of other instances and unparseable files are left alone.
pub fn list_records(dir: &Path, instance_id: &str) -> Vec<(PathBuf, ProcRecord)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match serde_json::from_slice::<ProcRecord>(&bytes) {
            Ok(rec) if rec.instance_id == instance_id => out.push((path, rec)),
            Ok(_) => {}
            Err(e) => tracing::warn!(path = %path.display(), "unreadable proc record: {e}"),
        }
    }
    out
}

/// Result of probing a process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// At least one member that can still run.
    Alive,
    /// No such group (ESRCH), or only zombies remain.
    Gone,
    /// EPERM: members exist that we may not signal.
    Denied,
}

/// What `kill(-pgid, …)` returned, before zombies are taken into account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
enum KillResult {
    Delivered,
    /// ESRCH.
    NoSuchGroup,
    /// EPERM.
    NotPermitted,
    /// Any other error (or an invalid pgid).
    Failed,
}

#[cfg(unix)]
fn kill_result(res: nix::Result<()>) -> KillResult {
    use nix::errno::Errno;
    match res {
        Ok(()) => KillResult::Delivered,
        Err(Errno::ESRCH) => KillResult::NoSuchGroup,
        Err(Errno::EPERM) => KillResult::NotPermitted,
        Err(_) => KillResult::Failed,
    }
}

/// The group has members and every one of them is a zombie: nothing in it
/// can run again.
#[cfg_attr(not(unix), allow(dead_code))]
fn only_zombies(members: &[crate::util::procinfo::ProcInfo]) -> bool {
    !members.is_empty() && members.iter().all(|m| m.zombie)
}

/// Refine a `kill(-pgid, …)` result with the group's members, so a group
/// holding only zombies counts as gone (they cannot run again, and waiting on
/// them would only time out).
///
/// Linux delivers to a zombie-only group (`Ok`). macOS instead returns EPERM
/// for it until the zombies are reaped, so EPERM is not "could not clean"
/// until the members show a live process we may not signal. Our own leader
/// is reaped by its bridge's wait task; orphans by init/launchd.
///
/// The last member can be reaped between the EPERM and the listing, which
/// then shows no member at all. So EPERM with no visible member asks
/// `recheck` (a fresh `kill(-pgid, 0)`) once more: ESRCH now means the group
/// is gone; anything else stays "could not clean".
/// `members` and `recheck` are only called when the answer depends on them.
#[cfg_attr(not(unix), allow(dead_code))]
fn classify_kill(
    res: KillResult,
    members: impl FnOnce() -> Vec<crate::util::procinfo::ProcInfo>,
    recheck: impl FnOnce() -> KillResult,
) -> Probe {
    match res {
        KillResult::NoSuchGroup => Probe::Gone,
        KillResult::Failed => Probe::Denied,
        KillResult::Delivered => {
            if only_zombies(&members()) {
                Probe::Gone
            } else {
                Probe::Alive
            }
        }
        KillResult::NotPermitted => {
            let members = members();
            if only_zombies(&members)
                || (members.is_empty() && recheck() == KillResult::NoSuchGroup)
            {
                Probe::Gone
            } else {
                Probe::Denied
            }
        }
    }
}

/// A fresh `kill(-pgid, 0)`, for [`classify_kill`]'s recheck.
#[cfg(unix)]
fn recheck_group(pgid: i32) -> KillResult {
    kill_result(nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(-pgid),
        None,
    ))
}

/// `kill(-pgid, 0)`, refined by [`classify_kill`].
pub fn probe_group(pgid: i32) -> Probe {
    #[cfg(unix)]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        if pgid <= 1 {
            return Probe::Denied;
        }
        classify_kill(
            kill_result(kill(Pid::from_raw(-pgid), None)),
            || crate::util::procinfo::group_members(pgid),
            || recheck_group(pgid),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = pgid;
        Probe::Gone
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sig {
    Term,
    Kill,
}

/// What signalling an owned group did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signalled {
    Sent,
    /// The group is gone (ESRCH), or its pgid provably belongs to someone
    /// else now: either way ours is gone.
    Gone,
    /// Not sent: EPERM, or the group could not be proven ours.
    Refused(&'static str),
}

/// Map a group signal's result: `Err(true)` when the group is gone (ESRCH,
/// or EPERM because only zombies remain; see [`classify_kill`]),
/// `Err(false)` when it could not be signalled.
#[cfg_attr(not(unix), allow(dead_code))]
fn signal_result(
    res: KillResult,
    members: impl FnOnce() -> Vec<crate::util::procinfo::ProcInfo>,
    recheck: impl FnOnce() -> KillResult,
) -> Result<(), bool> {
    match res {
        // Delivered: the caller polls for the group to empty.
        KillResult::Delivered => Ok(()),
        other => Err(classify_kill(other, members, recheck) == Probe::Gone),
    }
}

/// Signal a whole group. `Err(true)` when it is gone, `Err(false)` when it
/// could not be signalled (see [`signal_result`]).
fn signal_group(pgid: i32, sig: Sig) -> Result<(), bool> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;
        if pgid <= 1 {
            return Err(false);
        }
        let s = match sig {
            Sig::Term => Signal::SIGTERM,
            Sig::Kill => Signal::SIGKILL,
        };
        signal_result(
            kill_result(killpg(Pid::from_raw(pgid), s)),
            || crate::util::procinfo::group_members(pgid),
            || recheck_group(pgid),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (pgid, sig);
        Err(true)
    }
}

/// Poll `probe_group` until it reports `Gone` or `bound` passes, and return
/// the last answer. `Denied` is polled through too: after a signal, macOS
/// answers EPERM while members are mid-exit, so only an EPERM that lasts the
/// whole bound counts as "could not clean".
async fn wait_group(pgid: i32, bound: Duration) -> Probe {
    let deadline = tokio::time::Instant::now() + bound;
    loop {
        let p = probe_group(pgid);
        if p == Probe::Gone || tokio::time::Instant::now() >= deadline {
            return p;
        }
        tokio::time::sleep(POLL).await;
    }
}

/// How a finish ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishOutcome {
    /// The group is confirmed gone; the record is deleted.
    Gone,
    /// Members may remain (timeout, EPERM); the record is kept for recovery.
    Unresolved(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Record written (or not needed), nothing spawned yet.
    Provisional,
    /// The group exists and is ours to signal.
    Armed(i32),
    /// No group to own (spawn failed, child was not a session leader, or a
    /// non-Unix platform).
    Unowned,
    /// Observed gone; never signal again.
    Gone,
}

struct TreeInner {
    record_id: Uuid,
    scope: Option<ProcScope>,
    tag: Option<String>,
    record_dir: Option<PathBuf>,
    /// See [`ProcContext::env_proof`].
    env_proof: bool,
    record: Mutex<Option<ProcRecord>>,
    phase: Mutex<Phase>,
    /// Start time of the leader, for the identity check after it is reaped.
    leader_start_ms: Mutex<Option<u64>>,
    /// Set once the leader has been reaped. From then on an empty group's
    /// pgid can be reused, so every signal first re-proves the group is ours.
    leader_reaped: AtomicBool,
    finish_lock: tokio::sync::Mutex<()>,
}

impl TreeInner {
    fn pgid(&self) -> Option<i32> {
        match *self.phase.lock().unwrap_or_else(|e| e.into_inner()) {
            Phase::Armed(pgid) => Some(pgid),
            _ => None,
        }
    }

    /// Signal while holding the phase lock, so a concurrent drop or finish
    /// can never signal a group another path has already observed gone.
    ///
    /// Once the leader has been reaped, an emptied group's pgid may have been
    /// reused, so the group is first re-proven ours with the same check crash
    /// recovery uses ([`group_identity`]), with the leader known dead and the
    /// owner's lifetime running until now.
    fn signal(&self, sig: Sig) -> Signalled {
        let mut phase = self.phase.lock().unwrap_or_else(|e| e.into_inner());
        let Phase::Armed(pgid) = *phase else {
            return Signalled::Gone;
        };
        if self.leader_reaped.load(Ordering::SeqCst) {
            let leader_start = *self
                .leader_start_ms
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let proof = Ownership {
                tag: self.tag.as_deref(),
                leader_start,
                leader_dead: true,
                alive_until_ms: now_ms(),
                env_proof: self.env_proof,
            };
            match group_identity(pgid, &proof) {
                Some(true) => {}
                Some(false) => {
                    *phase = Phase::Gone;
                    return Signalled::Gone;
                }
                None => {
                    if probe_group(pgid) == Probe::Gone {
                        *phase = Phase::Gone;
                        return Signalled::Gone;
                    }
                    return Signalled::Refused("group can no longer be proven ours");
                }
            }
        }
        match signal_group(pgid, sig) {
            Ok(()) => Signalled::Sent,
            Err(true) => {
                *phase = Phase::Gone;
                Signalled::Gone
            }
            Err(false) => Signalled::Refused("EPERM"),
        }
    }

    fn mark_gone(&self) {
        *self.phase.lock().unwrap_or_else(|e| e.into_inner()) = Phase::Gone;
        if let Some(dir) = &self.record_dir {
            let _ = std::fs::remove_file(record_path(dir, self.record_id));
        }
        REGISTRY.forget(self.record_id);
    }

    async fn finish_group(&self) -> FinishOutcome {
        let _serial = self.finish_lock.lock().await;
        let Some(pgid) = self.pgid() else {
            // Nothing owned (or already gone): nothing can be cleaned, so the
            // record carries no information any more.
            self.mark_gone();
            return FinishOutcome::Gone;
        };
        let mut outcome = match probe_group(pgid) {
            Probe::Gone => None,
            Probe::Denied => Some("EPERM probing group".to_string()),
            Probe::Alive => match self.signal(Sig::Term) {
                Signalled::Gone => None,
                Signalled::Refused(why) => Some(format!("SIGTERM not sent: {why}")),
                Signalled::Sent => match wait_group(pgid, TERM_WAIT).await {
                    Probe::Gone => None,
                    Probe::Denied => Some("EPERM after SIGTERM".to_string()),
                    Probe::Alive => {
                        tracing::debug!(pgid, "group survived SIGTERM; sending SIGKILL");
                        match self.signal(Sig::Kill) {
                            Signalled::Gone => None,
                            Signalled::Refused(why) => Some(format!("SIGKILL not sent: {why}")),
                            Signalled::Sent => match wait_group(pgid, KILL_WAIT).await {
                                Probe::Gone => None,
                                Probe::Denied => Some("EPERM after SIGKILL".to_string()),
                                Probe::Alive => Some(
                                    "group still has members after SIGKILL (uninterruptible?)"
                                        .to_string(),
                                ),
                            },
                        }
                    }
                },
            },
        };
        match outcome.take() {
            None => {
                self.mark_gone();
                FinishOutcome::Gone
            }
            Some(why) => {
                tracing::warn!(
                    pgid,
                    record_id = %self.record_id,
                    "could not confirm agent process group gone ({why}); keeping record for recovery"
                );
                FinishOutcome::Unresolved(why)
            }
        }
    }

    fn backstop(&self) {
        let phase = *self.phase.lock().unwrap_or_else(|e| e.into_inner());
        if let Phase::Armed(pgid) = phase {
            // Synchronous, no waiting: the record stays until a later finish
            // (or recovery) confirms the group gone.
            let _ = self.signal(Sig::Kill);
            tracing::debug!(
                pgid,
                "process tree dropped unfinished; sent SIGKILL to group"
            );
        }
    }
}

/// Live trees, so daemon shutdown can finish them all.
pub struct Registry {
    /// Held shared by each [`SpawnAdmission`] and exclusively by
    /// [`shutdown_all`] while it stops accepting.
    gate: RwLock<()>,
    accepting: AtomicBool,
    live: Mutex<HashMap<Uuid, Weak<TreeInner>>>,
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| Registry {
    gate: RwLock::new(()),
    accepting: AtomicBool::new(true),
    live: Mutex::new(HashMap::new()),
});

impl Registry {
    fn remember(&self, inner: &Arc<TreeInner>) {
        if let Ok(mut m) = self.live.lock() {
            m.insert(inner.record_id, Arc::downgrade(inner));
        }
    }

    fn forget(&self, id: Uuid) {
        if let Ok(mut m) = self.live.lock() {
            m.remove(&id);
        }
    }

    fn snapshot(&self, filter: impl Fn(&TreeInner) -> bool) -> Vec<Arc<TreeInner>> {
        let Ok(m) = self.live.lock() else {
            return Vec::new();
        };
        m.values()
            .filter_map(Weak::upgrade)
            .filter(|t| filter(t))
            .collect()
    }
}

/// True once daemon shutdown has started: no new agent may be spawned and
/// terminal run messages are left to the shutdown drain.
pub fn shutting_down() -> bool {
    !REGISTRY.accepting.load(Ordering::SeqCst)
}

/// Admission for one spawn: hold it from [`ProcessTree::prepare`] through
/// [`ProcessTree::attach`].
/// [`shutdown_all`] waits for every admission to be dropped before it stops
/// accepting and takes its snapshot, so an admitted spawn is either attached
/// and in the snapshot, or never prepared at all.
pub struct SpawnAdmission {
    _gate: std::sync::RwLockReadGuard<'static, ()>,
}

/// Admit one spawn, or fail once daemon shutdown has begun.
pub fn admit() -> std::io::Result<SpawnAdmission> {
    let guard = REGISTRY.gate.read().unwrap_or_else(|e| e.into_inner());
    if shutting_down() {
        return Err(std::io::Error::other(
            "daemon is shutting down; not spawning new agents",
        ));
    }
    Ok(SpawnAdmission { _gate: guard })
}

/// Daemon shutdown: stop accepting spawns, snapshot every live tree and
/// finish them in parallel, bounded by `bound`. Returns how many trees were
/// still unresolved.
pub async fn shutdown_all(bound: Duration) -> usize {
    // Wait (briefly: admissions span one synchronous prepare/spawn/attach)
    // for admitted spawns to attach, then refuse new ones.
    let stop = tokio::task::spawn_blocking(|| {
        let _gate = REGISTRY.gate.write().unwrap_or_else(|e| e.into_inner());
        REGISTRY.accepting.store(false, Ordering::SeqCst);
    });
    if stop.await.is_err() {
        REGISTRY.accepting.store(false, Ordering::SeqCst);
    }
    let trees = REGISTRY.snapshot(|_| true);
    let n = trees.len();
    let finishes = trees
        .into_iter()
        .map(|t| async move { t.finish_group().await });
    match tokio::time::timeout(bound, futures_util::future::join_all(finishes)).await {
        Ok(results) => results
            .into_iter()
            .filter(|r| matches!(r, FinishOutcome::Unresolved(_)))
            .count(),
        Err(_) => {
            tracing::warn!(trees = n, "finishing agent process trees timed out");
            n
        }
    }
}

/// Finish every live tree belonging to `scope` (safety net for a bridge that
/// was dropped before it could be finished, e.g. a failed startup).
pub async fn finish_scope(kind: ScopeKind, id: &str) {
    let trees = REGISTRY.snapshot(|t| {
        t.scope
            .as_ref()
            .is_some_and(|s| s.kind == kind && s.id == id)
    });
    let finishes = trees
        .into_iter()
        .map(|t| async move { t.finish_group().await });
    futures_util::future::join_all(finishes).await;
}

/// The owner of one agent's process group. Exactly one owner exists; dropping
/// it unfinished sends SIGKILL to the group as a backstop.
pub struct ProcessTree {
    inner: Arc<TreeInner>,
}

/// A non-owning view, for probing whether the group is gone.
#[derive(Clone)]
pub struct TreeView {
    inner: Weak<TreeInner>,
}

impl TreeView {
    /// True when the group is known to be gone (or was never owned).
    pub fn is_gone(&self) -> bool {
        let Some(inner) = self.inner.upgrade() else {
            return true;
        };
        match inner.pgid() {
            Some(pgid) if probe_group(pgid) == Probe::Gone => {
                // Observed gone: never signal this pgid again.
                inner.mark_gone();
                true
            }
            Some(_) => false,
            None => true,
        }
    }
}

impl ProcessTree {
    /// Prepare a tree for the next spawn in the current task: reads the
    /// task-local [`PROC_SCOPE`] and the daemon [`ProcContext`], and writes
    /// the provisional record when both are present. Fails once daemon
    /// shutdown has begun, or when the record cannot be written.
    pub fn prepare_current() -> std::io::Result<Self> {
        let scope = PROC_SCOPE.try_with(Clone::clone).ok();
        Self::prepare(scope, current_context())
    }

    /// [`Self::prepare_current`] with explicit inputs (tests, recovery).
    pub fn prepare(
        scope: Option<ProcScope>,
        ctx: Option<Arc<ProcContext>>,
    ) -> std::io::Result<Self> {
        if shutting_down() {
            return Err(std::io::Error::other(
                "daemon is shutting down; not spawning new agents",
            ));
        }
        let record_id = Uuid::new_v4();
        let env_proof = ctx
            .as_ref()
            .map_or(crate::util::procinfo::ENV_OWNERSHIP_PROOF, |c| c.env_proof);
        let (tag, record_dir, record) = match (&scope, &ctx) {
            (Some(scope), Some(ctx)) => {
                let tag = ctx.tag_for(scope);
                let rec = ProcRecord {
                    record_id,
                    instance_id: ctx.instance_id.clone(),
                    runner_id: scope.runner_id.clone(),
                    kind: scope.kind,
                    run_or_chat_id: scope.id.clone(),
                    tag: tag.clone(),
                    scratch: None,
                    pgid: None,
                    leader_start_time_ms: None,
                    owner_alive_until_ms: None,
                };
                write_record(&ctx.proc_dir, &rec)?;
                (Some(tag), Some(ctx.proc_dir.clone()), Some(rec))
            }
            _ => (None, None, None),
        };
        let inner = Arc::new(TreeInner {
            record_id,
            scope,
            tag,
            record_dir,
            env_proof,
            record: Mutex::new(record),
            phase: Mutex::new(Phase::Provisional),
            leader_start_ms: Mutex::new(None),
            leader_reaped: AtomicBool::new(false),
            finish_lock: tokio::sync::Mutex::new(()),
        });
        REGISTRY.remember(&inner);
        Ok(Self { inner })
    }

    /// The `PIDASH_PROC_TAG` value for the agent, when records are kept.
    pub fn tag(&self) -> Option<&str> {
        self.inner.tag.as_deref()
    }

    pub fn record_id(&self) -> Uuid {
        self.inner.record_id
    }

    /// Path of the on-disk record, when one is kept.
    pub fn record_path(&self) -> Option<PathBuf> {
        self.inner
            .record_dir
            .as_ref()
            .map(|d| record_path(d, self.inner.record_id))
    }

    pub fn pgid(&self) -> Option<i32> {
        self.inner.pgid()
    }

    pub fn view(&self) -> TreeView {
        TreeView {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Record the spawned leader. The group is owned only when the child is
    /// really the leader of its own group (it went through the `setsid`
    /// spawn helper); otherwise the tree stays leader-only so we never signal
    /// a group we did not create (such as the daemon's own).
    pub fn attach(&self, pid: Option<u32>) {
        let owned = pid.and_then(|pid| {
            let pid = i32::try_from(pid).ok()?;
            #[cfg(unix)]
            {
                let pgid = nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(pid))).ok()?;
                (pgid.as_raw() == pid).then_some(pid)
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                None
            }
        });
        *self.inner.phase.lock().unwrap_or_else(|e| e.into_inner()) = match owned {
            Some(pgid) => Phase::Armed(pgid),
            None => Phase::Unowned,
        };
        let Some(pgid) = owned else {
            return;
        };
        let start = crate::util::procinfo::info(pgid).map(|p| p.start_time_ms);
        *self
            .inner
            .leader_start_ms
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = start;
        let mut rec = self.inner.record.lock().unwrap_or_else(|e| e.into_inner());
        if let (Some(rec), Some(dir)) = (rec.as_mut(), &self.inner.record_dir) {
            rec.pgid = Some(pgid);
            rec.leader_start_time_ms = start;
            if let Err(e) = write_record(dir, rec) {
                tracing::warn!("failed to update proc record {}: {e}", rec.record_id);
            }
        }
    }

    /// The leader has been reaped (its exit status collected). Its pid, and
    /// with it the pgid, can now be reused once the group empties: disarm at
    /// once if the group is already gone, and re-prove ownership before any
    /// later signal (see `TreeInner::signal`).
    pub fn leader_reaped(&self) {
        self.inner.leader_reaped.store(true, Ordering::SeqCst);
        if let Some(pgid) = self.inner.pgid()
            && probe_group(pgid) == Probe::Gone
        {
            self.inner.mark_gone();
        }
    }

    /// Test-only: simulate a daemon crash. The record stays on disk exactly
    /// as written, nothing is signalled, and the tree leaves the registry.
    #[cfg(test)]
    pub fn abandon(self) {
        *self.inner.phase.lock().unwrap_or_else(|e| e.into_inner()) = Phase::Gone;
        REGISTRY.forget(self.inner.record_id);
    }

    /// Terminate whatever is left in the group (SIGTERM ≤ 3 s, then SIGKILL
    /// ≤ 2 s). The graceful step (close stdin, wait for the leader) is the
    /// caller's; see `agent::tracked`. Safe to call more than once.
    pub async fn finish_group(&self) -> FinishOutcome {
        self.inner.finish_group().await
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.inner.backstop();
        let phase = *self.inner.phase.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(phase, Phase::Provisional | Phase::Unowned)
            && let Some(dir) = &self.inner.record_dir
        {
            // Nothing was (or could be) owned: the record is meaningless.
            let _ = std::fs::remove_file(record_path(dir, self.inner.record_id));
        }
        REGISTRY.forget(self.inner.record_id);
    }
}

/// What [`recover`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Records resolved after signalling their group.
    pub cleaned: usize,
    /// Records resolved without signalling (group gone, or provably reused).
    pub resolved: usize,
    /// Records kept for a later retry.
    pub kept: usize,
}

fn tagged(pid: i32, tag: &str) -> Option<bool> {
    let env = crate::util::procinfo::environ(pid)?;
    Some(crate::util::procinfo::env_value(&env, PROC_TAG_ENV) == Some(tag.as_bytes()))
}

/// Signal-and-poll a group we do not parent (recovery): SIGTERM, poll ≤ 3 s,
/// SIGKILL, poll ≤ 2 s. `proof` was just confirmed for the group; it is
/// checked again before the SIGKILL, because the group can empty after the
/// SIGTERM and its pgid be reused while we poll.
async fn clean_foreign_group(pgid: i32, proof: &Ownership<'_>) -> bool {
    // The live members the proof was confirmed on (pid and start time).
    let proven: Vec<(i32, u64)> = crate::util::procinfo::group_members(pgid)
        .iter()
        .filter(|m| !m.zombie)
        .map(|m| (m.pid, m.start_time_ms))
        .collect();
    match signal_group(pgid, Sig::Term) {
        Err(true) => return true,
        Err(false) => return false,
        Ok(()) => {}
    }
    match wait_group(pgid, TERM_WAIT).await {
        Probe::Gone => return true,
        Probe::Denied => return false,
        Probe::Alive => {}
    }
    match still_ours(pgid, proof, &proven) {
        Some(true) => {}
        // Ours emptied and the pgid now belongs to someone else.
        Some(false) => return true,
        None => return false,
    }
    match signal_group(pgid, Sig::Kill) {
        Err(true) => return true,
        Err(false) => return false,
        Ok(()) => {}
    }
    wait_group(pgid, KILL_WAIT).await == Probe::Gone
}

/// Signal-and-poll individual processes found by tag (a provisional record
/// with no pgid). The tag is re-checked right before each signal.
async fn clean_tagged_pids(tag: &str) -> bool {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let find = || -> Vec<i32> {
            crate::util::procinfo::same_uid_processes()
                .into_iter()
                .map(|p| p.pid)
                .filter(|pid| tagged(*pid, tag) == Some(true))
                .collect()
        };
        for (sig, bound) in [(Signal::SIGTERM, TERM_WAIT), (Signal::SIGKILL, KILL_WAIT)] {
            let pids = find();
            if pids.is_empty() {
                return true;
            }
            for pid in pids {
                if tagged(pid, tag) == Some(true) {
                    let _ = kill(Pid::from_raw(pid), sig);
                }
            }
            let deadline = tokio::time::Instant::now() + bound;
            while tokio::time::Instant::now() < deadline {
                if find().is_empty() {
                    return true;
                }
                tokio::time::sleep(POLL).await;
            }
        }
        find().is_empty()
    }
    #[cfg(not(unix))]
    {
        let _ = tag;
        true
    }
}

/// Re-check, before escalating, that `pgid` is still the group proven ours.
/// It is if [`group_identity`] proves it again, or if every live member is
/// one of the `proven` members (same pid, start time within tolerance): the leader may
/// have exited on SIGTERM, leaving members the start-time window alone can
/// no longer vouch for, but no newcomer has joined. A member outside that
/// set means the group can no longer be proven ours.
fn still_ours(pgid: i32, proof: &Ownership<'_>, proven: &[(i32, u64)]) -> Option<bool> {
    match group_identity(pgid, proof) {
        Some(true) => return Some(true),
        Some(false) => return Some(false),
        None => {}
    }
    let live: Vec<_> = crate::util::procinfo::group_members(pgid)
        .into_iter()
        .filter(|m| !m.zombie)
        .collect();
    (!live.is_empty()
        && live.iter().all(|m| {
            proven.iter().any(|&(pid, start)| {
                pid == m.pid && start.abs_diff(m.start_time_ms) <= START_TOLERANCE_MS
            })
        }))
    .then_some(true)
}

/// What a group must match to be ours. See [`group_identity`].
struct Ownership<'a> {
    /// Our `PIDASH_PROC_TAG`, when records (and so the tag) are kept.
    tag: Option<&'a str>,
    /// Recorded start time of our leader.
    leader_start: Option<u64>,
    /// Our leader is known to have exited (we reaped it): any process with
    /// pid == pgid is someone else.
    leader_dead: bool,
    /// The owning daemon was not alive after this time: now for a live tree,
    /// the crashed daemon's last-alive mark in recovery (`0` = unknown).
    alive_until_ms: u64,
    /// Environment reads may be used (Linux; see `procinfo`).
    env_proof: bool,
}

/// Is the live group `pgid` still ours? Used by crash recovery and, once the
/// leader is reaped, before every signal of a live tree.
///
/// * `Some(true)`: a member carries our tag (only where environments are
///   readable); or the process with pid == pgid is our original leader (its
///   start time matches the recorded one, and no member was readable without
///   our tag); or the group is leaderless and
///   every member started inside the owner's lifetime (at or after the
///   recorded leader start, at or before `alive_until_ms`) and, where
///   environments are readable, none was readable without our tag.
/// * `Some(false)`: the pgid provably belongs to another group now: its
///   leader is a different process, or a member is older than our leader.
/// * `None`: cannot prove either way (or no live member is left). Never
///   signal; keep the record.
///
/// A group created after our group emptied has every member started after
/// our leader, so the leader start alone cannot tell it apart; the leader
/// identity and the lifetime upper bound can.
fn group_identity(pgid: i32, o: &Ownership<'_>) -> Option<bool> {
    let all = crate::util::procinfo::group_members(pgid);
    let live: Vec<_> = all.iter().filter(|m| !m.zombie).collect();
    if live.is_empty() {
        return None;
    }
    let mut any_readable = false;
    if o.env_proof
        && let Some(tag) = o.tag
    {
        for m in &live {
            match tagged(m.pid, tag) {
                Some(true) => return Some(true),
                Some(false) => any_readable = true,
                None => {}
            }
        }
    }
    let leader_start = o.leader_start?;
    if let Some(leader) = all.iter().find(|m| m.pid == pgid) {
        // A process holds the group's pid: it is either our leader or the
        // leader of a group that reused the pgid.
        if o.leader_dead || leader.start_time_ms.abs_diff(leader_start) > START_TOLERANCE_MS {
            return Some(false);
        }
        // Matching start time, but a readable member without our tag: rapid
        // pid reuse can land inside the tolerance, and readable negative tag
        // evidence outweighs an approximate start-time match.
        return if any_readable { None } else { Some(true) };
    }
    if live
        .iter()
        .any(|m| m.start_time_ms.saturating_add(START_TOLERANCE_MS) < leader_start)
    {
        // A member older than our leader: the pgid was reused.
        return Some(false);
    }
    if any_readable {
        // Readable, untagged, started after our leader: cannot prove either
        // way. Keep the record and retry later.
        return None;
    }
    // Leaderless: every member must have started while the owner was alive.
    // A member started later belongs to a group that reused the pgid after
    // the crash, or to ours started after the last mark; we cannot tell, so
    // do not signal.
    live.iter()
        .all(|m| m.start_time_ms <= o.alive_until_ms)
        .then_some(true)
}

/// Inputs to [`recover`] beyond the records.
#[derive(Debug, Clone, Copy)]
pub struct RecoverOpts {
    /// The previous daemon's last-alive mark ([`read_alive`]), read before
    /// this daemon starts its own heartbeat.
    pub last_alive_ms: Option<u64>,
    /// See [`ProcContext::env_proof`].
    pub env_proof: bool,
}

/// Crash recovery for `records` (already filtered to our instance id by
/// [`list_records`]; call it before any new spawn so live records are not in
/// the list). Deletes a record only once its group is confirmed gone.
pub async fn recover(records: Vec<(PathBuf, ProcRecord)>, opts: RecoverOpts) -> RecoveryReport {
    // Records are independent, so resolve them in parallel: the whole pass
    // takes about one record's worst case (SIGTERM 3 s + SIGKILL 2 s), which
    // keeps daemon start short while it waits for recovery.
    let results =
        futures_util::future::join_all(records.into_iter().map(|r| recover_one(r, opts))).await;
    let mut report = RecoveryReport::default();
    for r in results {
        match r {
            Recovered::Cleaned => report.cleaned += 1,
            Recovered::Resolved => report.resolved += 1,
            Recovered::Kept => report.kept += 1,
        }
    }
    report
}

enum Recovered {
    Cleaned,
    Resolved,
    Kept,
}

async fn recover_one((path, mut rec): (PathBuf, ProcRecord), opts: RecoverOpts) -> Recovered {
    // The owner's lifetime ends at the mark read by the first pass that saw
    // this record; a later daemon's mark must not widen it.
    let alive_until_ms = rec
        .owner_alive_until_ms
        .unwrap_or(opts.last_alive_ms.unwrap_or(0));
    // Pin it before any signal or wait: this pass can be cut short (startup
    // bound, crash) while cleanup is in flight, and the record must not reach
    // the next daemon without it.
    if rec.pgid.is_some() && rec.owner_alive_until_ms.is_none() {
        rec.owner_alive_until_ms = Some(alive_until_ms);
        if let Some(dir) = path.parent()
            && let Err(e) = write_record(dir, &rec)
        {
            tracing::warn!(record_id = %rec.record_id, "failed to stamp proc record: {e}");
        }
    }
    let outcome = match rec.pgid {
        // Crash between record and spawn (or before the pgid was filled
        // in): resolve by tag alone.
        None => {
            if !opts.env_proof {
                tracing::warn!(
                    record_id = %rec.record_id,
                    "proc record has no pgid and process environments are unreadable on this platform; nothing can be found to clean"
                );
                Recovered::Resolved
            } else if clean_tagged_pids(&rec.tag).await {
                Recovered::Resolved
            } else {
                Recovered::Kept
            }
        }
        Some(pgid) => match probe_group(pgid) {
            Probe::Gone => Recovered::Resolved,
            Probe::Denied => Recovered::Kept,
            Probe::Alive => {
                let proof = Ownership {
                    tag: Some(&rec.tag),
                    leader_start: rec.leader_start_time_ms,
                    leader_dead: false,
                    alive_until_ms,
                    env_proof: opts.env_proof,
                };
                match group_identity(pgid, &proof) {
                    Some(true) => {
                        tracing::info!(pgid, record_id = %rec.record_id, "recovering leftover agent process group");
                        if clean_foreign_group(pgid, &proof).await {
                            Recovered::Cleaned
                        } else {
                            Recovered::Kept
                        }
                    }
                    Some(false) => {
                        tracing::info!(pgid, record_id = %rec.record_id, "proc record's pgid now belongs to another group; ignoring it");
                        Recovered::Resolved
                    }
                    None => Recovered::Kept,
                }
            }
        },
    };
    match outcome {
        Recovered::Kept => {
            tracing::warn!(record_id = %rec.record_id, "could not resolve proc record; keeping it for the next start");
        }
        Recovered::Cleaned | Recovered::Resolved => {
            let _ = std::fs::remove_file(&path);
        }
    }
    outcome
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn scope(id: &str) -> ProcScope {
        ProcScope {
            kind: ScopeKind::Run,
            id: id.to_string(),
            runner_id: Some("r1".to_string()),
        }
    }

    #[test]
    fn record_is_written_atomically_before_spawn_and_filled_after() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(ProcContext::new("test-instance", dir.path()));
        let tree = ProcessTree::prepare(Some(scope("run-a")), Some(ctx)).unwrap();
        let path = tree.record_path().unwrap();
        let rec: ProcRecord = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rec.pgid, None);
        assert_eq!(rec.tag, "test-instance:run:run-a");
        assert_eq!(tree.tag(), Some("test-instance:run:run-a"));
        // No temp file is left behind.
        let names: Vec<_> = std::fs::read_dir(dir.path().join("proc"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1);
        drop(tree);
        assert!(!path.exists(), "a never-spawned record is removed on drop");
    }

    #[test]
    fn records_of_other_instances_are_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let mine = ProcContext::new("mine", dir.path());
        let rec = |inst: &str| ProcRecord {
            record_id: Uuid::new_v4(),
            instance_id: inst.into(),
            runner_id: None,
            kind: ScopeKind::Run,
            run_or_chat_id: "x".into(),
            tag: format!("{inst}:run:x"),
            scratch: None,
            pgid: Some(999_999),
            leader_start_time_ms: Some(0),
            owner_alive_until_ms: None,
        };
        write_record(&mine.proc_dir, &rec("mine")).unwrap();
        write_record(&mine.proc_dir, &rec("theirs")).unwrap();
        let listed = list_records(&mine.proc_dir, "mine");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].1.instance_id, "mine");
    }

    #[test]
    fn attach_refuses_a_child_that_is_not_a_group_leader() {
        // `sleep` without setsid stays in the test's own group: owning that
        // group would mean signalling the test harness itself.
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let tree = ProcessTree::prepare(None, None).unwrap();
        tree.attach(Some(child.id()));
        assert_eq!(tree.pgid(), None);
        drop(tree);
        let _ = child.kill();
        let _ = child.wait();
    }

    fn session_cmd(script: &str, tag: Option<&str>) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(tag) = tag {
            cmd.env(PROC_TAG_ENV, tag);
        } else {
            cmd.env_remove(PROC_TAG_ENV);
        }
        crate::util::shell::new_session(&mut cmd);
        cmd
    }

    use crate::agent::tracked::tests::{alive, read_pids, wait_all_gone};

    fn ctx(dir: &Path, id: &str) -> Arc<ProcContext> {
        Arc::new(ProcContext::new(id, dir))
    }

    /// A context whose ownership checks never read environments: the macOS
    /// behaviour, exercised on every platform.
    fn ctx_env(dir: &Path, id: &str, env_proof: bool) -> Arc<ProcContext> {
        let mut c = ProcContext::new(id, dir);
        c.env_proof = env_proof;
        Arc::new(c)
    }

    fn opts(ctx: &ProcContext, last_alive_ms: Option<u64>) -> RecoverOpts {
        RecoverOpts {
            last_alive_ms,
            env_proof: ctx.env_proof,
        }
    }

    fn stale(ctx: &ProcContext, id: &str, pgid: Option<i32>, leader_start: Option<u64>) -> PathBuf {
        let rec = ProcRecord {
            record_id: Uuid::new_v4(),
            instance_id: ctx.instance_id.clone(),
            runner_id: None,
            kind: ScopeKind::Run,
            run_or_chat_id: id.into(),
            tag: format!("{}:run:{id}", ctx.instance_id),
            scratch: None,
            pgid,
            leader_start_time_ms: leader_start,
            owner_alive_until_ms: None,
        };
        write_record(&ctx.proc_dir, &rec).unwrap()
    }

    /// A group whose leader started (tagged or not), forked `sleep 300` and
    /// exited, and was reaped: `(pgid, orphan pid)`.
    async fn leaderless_group(dir: &Path, tag: Option<&str>) -> (i32, i32) {
        let pf = dir.join(format!("pids-{}", Uuid::new_v4()));
        let mut child = session_cmd(
            &format!("sleep 300 & echo $! > {}; exit 0", pf.display()),
            tag,
        )
        .spawn()
        .unwrap();
        let pgid = child.id().unwrap() as i32;
        child.wait().await.unwrap();
        let orphan = read_pids(&pf, 1).await[0];
        assert!(alive(orphan));
        (pgid, orphan)
    }

    #[tokio::test]
    async fn recovery_escalates_only_while_the_proven_members_remain() {
        // After SIGTERM, recovery re-checks the group before SIGKILL. A
        // leaderless group the start-time window cannot vouch for (no mark)
        // stays ours only while its live members are the ones proven before
        // the SIGTERM; a newcomer (a reused pgid) is never escalated on.
        let dir = tempfile::tempdir().unwrap();
        let (pgid, orphan) = leaderless_group(dir.path(), None).await;
        let start = crate::util::procinfo::info(orphan).unwrap().start_time_ms;
        let proof = Ownership {
            tag: None,
            leader_start: Some(start),
            leader_dead: false,
            alive_until_ms: 0,
            env_proof: false,
        };
        assert_eq!(group_identity(pgid, &proof), None);
        assert_eq!(still_ours(pgid, &proof, &[(orphan, start)]), Some(true));
        assert_eq!(still_ours(pgid, &proof, &[]), None, "a newcomer");
        assert_eq!(
            still_ours(pgid, &proof, &[(orphan + 1, start)]),
            None,
            "a different pid"
        );
        // Provable as someone else's: a member older than our leader.
        let later = Ownership {
            leader_start: Some(start + 10 * START_TOLERANCE_MS),
            ..proof
        };
        assert_eq!(still_ours(pgid, &later, &[(orphan, start)]), Some(false));
        kill_now(orphan);
        wait_all_gone(&[orphan], Duration::from_secs(10)).await;
    }

    fn kill_now(pid: i32) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }

    async fn leaderless_group_left_by_a_crashed_daemon_is_cleaned(env_proof: bool) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_env(dir.path(), "test-instance-crash", env_proof);
        let pf = dir.path().join("pids");
        let tree = ProcessTree::prepare(Some(scope("run-crash")), Some(ctx.clone())).unwrap();
        // The agent starts a child, then exits (as on stdin EOF when the
        // daemon dies): a leaderless group.
        let mut child = session_cmd(
            &format!("sleep 300 & echo $! > {}; exit 0", pf.display()),
            tree.tag(),
        )
        .spawn()
        .unwrap();
        tree.attach(child.id());
        let pgid = tree.pgid().expect("leader owns its group");
        child.wait().await.unwrap();
        let orphan = read_pids(&pf, 1).await[0];
        assert!(alive(orphan));
        // Crash: no finish, no drop backstop. The daemon's last heartbeat
        // came after the orphan started.
        tree.abandon();
        let last_alive = now_ms();

        let records = list_records(&ctx.proc_dir, &ctx.instance_id);
        assert_eq!(records.len(), 1, "unresolved record survives the crash");
        assert_eq!(records[0].1.pgid, Some(pgid));
        let report = recover(records, opts(&ctx, Some(last_alive))).await;
        assert_eq!(report.cleaned, 1, "{report:?}");
        assert!(wait_all_gone(&[orphan], Duration::from_secs(1)).await);
        assert!(list_records(&ctx.proc_dir, &ctx.instance_id).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn recovery_cleans_a_leaderless_group_left_by_a_crashed_daemon() {
        leaderless_group_left_by_a_crashed_daemon_is_cleaned(true).await;
    }

    #[tokio::test]
    async fn recovery_cleans_a_leaderless_group_by_start_times_when_environments_are_unreadable() {
        leaderless_group_left_by_a_crashed_daemon_is_cleaned(false).await;
    }

    #[tokio::test]
    async fn a_zombie_only_group_left_by_a_crashed_daemon_is_resolved_and_its_record_removed() {
        // The stale agent died but its parent (here the test) has not reaped
        // it yet. macOS answers kill(-pgid) with EPERM for such a group, so
        // recovery must see the zombie and treat the group as gone.
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_env(dir.path(), "test-instance-zombie", false);
        let mut child = session_cmd("exec sleep 300", None).spawn().unwrap();
        let pid = child.id().unwrap() as i32;
        let start = crate::util::procinfo::info(pid).unwrap().start_time_ms;
        stale(&ctx, "run-zombie", Some(pid), Some(start));
        kill_now(pid);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !only_zombies(&crate::util::procinfo::group_members(pid)) {
            assert!(
                std::time::Instant::now() < deadline,
                "the killed agent never showed up as a zombie: {:?}",
                crate::util::procinfo::group_members(pid)
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(probe_group(pid), Probe::Gone);
        let report = recover(
            list_records(&ctx.proc_dir, &ctx.instance_id),
            opts(&ctx, Some(now_ms())),
        )
        .await;
        assert_eq!(report.kept, 0, "{report:?}");
        assert!(list_records(&ctx.proc_dir, &ctx.instance_id).is_empty());
        let _ = child.wait().await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn recovery_resolves_a_provisional_record_by_tag() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-prov");
        let tree = ProcessTree::prepare(Some(scope("run-prov")), Some(ctx.clone())).unwrap();
        let tag = tree.tag().unwrap().to_string();
        // Crash after the spawn but before the pgid was recorded.
        tree.abandon();
        let mut child = session_cmd("exec sleep 300", Some(&tag)).spawn().unwrap();
        let pid = child.id().unwrap() as i32;
        let records = list_records(&ctx.proc_dir, &ctx.instance_id);
        assert_eq!(records[0].1.pgid, None);
        let report = recover(records, opts(&ctx, None)).await;
        assert_eq!(report.kept, 0, "{report:?}");
        let _ = child.wait().await;
        assert!(!alive(pid));
        assert!(list_records(&ctx.proc_dir, &ctx.instance_id).is_empty());
    }

    #[tokio::test]
    async fn a_provisional_record_signals_nothing_when_environments_are_unreadable() {
        // macOS: no tag scan is possible, so nothing is signalled and the
        // (unactionable) record is dropped.
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_env(dir.path(), "test-instance-prov-noenv", false);
        let tree = ProcessTree::prepare(Some(scope("run-prov")), Some(ctx.clone())).unwrap();
        let tag = tree.tag().unwrap().to_string();
        tree.abandon();
        let mut child = session_cmd("exec sleep 300", Some(&tag)).spawn().unwrap();
        let pid = child.id().unwrap() as i32;
        let report = recover(
            list_records(&ctx.proc_dir, &ctx.instance_id),
            opts(&ctx, Some(now_ms())),
        )
        .await;
        assert_eq!(report.resolved, 1, "{report:?}");
        assert!(alive(pid));
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn recovery_ignores_a_reused_pgid_whose_leader_has_a_different_start_time() {
        for env_proof in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = ctx_env(dir.path(), "test-instance-reuse", env_proof);
            // A live group whose leader holds the recorded pgid but is not our
            // leader: it started a minute after the recorded one, i.e. after
            // our group died and the pid was reused. Its members are all
            // newer than our leader, so only the leader identity tells.
            let mut child = session_cmd("sleep 300 & exec sleep 300", None)
                .spawn()
                .unwrap();
            let pid = child.id().unwrap() as i32;
            let start = crate::util::procinfo::info(pid).unwrap().start_time_ms;
            stale(&ctx, "old", Some(pid), Some(start - 60_000));
            let report = recover(
                list_records(&ctx.proc_dir, &ctx.instance_id),
                opts(&ctx, Some(now_ms())),
            )
            .await;
            assert_eq!(report.cleaned, 0, "env_proof={env_proof}");
            assert_eq!(report.resolved, 1, "env_proof={env_proof}");
            assert!(alive(pid), "a reused pgid is never signalled");
            kill_now(-pid);
            let _ = child.wait().await;
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_readable_untagged_leader_is_not_proven_ours_by_its_start_time() {
        // Rapid pid reuse: a leader holding the recorded pgid whose start
        // time matches ours within the tolerance, but whose readable
        // environment lacks our tag. The tag evidence wins: keep, never
        // signal.
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-readable");
        let mut child = session_cmd("sleep 300 & exec sleep 300", None)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap() as i32;
        let start = crate::util::procinfo::info(pid).unwrap().start_time_ms;
        stale(&ctx, "readable", Some(pid), Some(start));
        let report = recover(
            list_records(&ctx.proc_dir, &ctx.instance_id),
            opts(&ctx, Some(now_ms())),
        )
        .await;
        assert_eq!(report.cleaned, 0, "{report:?}");
        assert_eq!(report.kept, 1, "{report:?}");
        assert!(alive(pid), "never signalled");
        kill_now(-pid);
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn the_owner_lifetime_is_pinned_before_recovery_signals() {
        // Recovery can be cut short (startup bound, or a crash) while it
        // waits for a group to die. The record must already carry the
        // owner's lifetime, or the next daemon's newer mark widens it.
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_env(dir.path(), "test-instance-pin", false);
        let mut child = session_cmd("trap '' TERM; sleep 300 & wait", None)
            .spawn()
            .unwrap();
        let pid = child.id().unwrap() as i32;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let start = crate::util::procinfo::info(pid).unwrap().start_time_ms;
        let path = stale(&ctx, "pin", Some(pid), Some(start));
        let mark = now_ms();
        let cut = tokio::time::timeout(
            Duration::from_millis(500),
            recover(
                list_records(&ctx.proc_dir, &ctx.instance_id),
                opts(&ctx, Some(mark)),
            ),
        )
        .await;
        assert!(cut.is_err(), "recovery was still waiting for the group");
        let rec: ProcRecord = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rec.owner_alive_until_ms, Some(mark));
        kill_now(-pid);
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn recovery_ignores_a_reused_pgid_whose_members_are_older_than_our_leader() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-older");
        let (pgid, orphan) = leaderless_group(dir.path(), None).await;
        stale(&ctx, "older", Some(pgid), Some(u64::MAX / 2));
        let report = recover(
            list_records(&ctx.proc_dir, &ctx.instance_id),
            opts(&ctx, Some(u64::MAX)),
        )
        .await;
        assert_eq!(report.resolved, 1, "{report:?}");
        assert!(alive(orphan));
        kill_now(orphan);
    }

    #[tokio::test]
    async fn a_leaderless_group_started_after_the_last_alive_mark_is_not_signalled() {
        // A group created after the crash, whose leader got the stale pgid
        // and has since exited: every member started after our leader, but
        // also after the crashed daemon's last-alive mark, so it cannot be
        // ours.
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_env(dir.path(), "test-instance-after", false);
        let leader_start = now_ms() - 60_000;
        let last_alive = leader_start + 1_000;
        let (pgid, orphan) = leaderless_group(dir.path(), None).await;
        let path = stale(&ctx, "after", Some(pgid), Some(leader_start));
        let report = recover(
            list_records(&ctx.proc_dir, &ctx.instance_id),
            opts(&ctx, Some(last_alive)),
        )
        .await;
        assert_eq!(report.kept, 1, "{report:?}");
        assert!(alive(orphan), "not signalled");
        let rec: ProcRecord = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            rec.owner_alive_until_ms,
            Some(last_alive),
            "the owner's lifetime is pinned on the kept record"
        );
        // A later daemon's newer mark must not widen the window.
        let report = recover(
            list_records(&ctx.proc_dir, &ctx.instance_id),
            opts(&ctx, Some(now_ms())),
        )
        .await;
        assert_eq!(report.kept, 1, "{report:?}");
        assert!(alive(orphan), "still not signalled");
        kill_now(orphan);
    }

    #[tokio::test]
    async fn a_leaderless_group_is_not_signalled_without_a_last_alive_mark() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_env(dir.path(), "test-instance-nomark", false);
        let leader_start = now_ms() - 60_000;
        let (pgid, orphan) = leaderless_group(dir.path(), None).await;
        stale(&ctx, "nomark", Some(pgid), Some(leader_start));
        let report = recover(
            list_records(&ctx.proc_dir, &ctx.instance_id),
            opts(&ctx, None),
        )
        .await;
        assert_eq!(report.kept, 1, "{report:?}");
        assert!(alive(orphan));
        kill_now(orphan);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn an_unprovable_record_is_kept_for_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-keep");
        // Leaderless, readable but untagged, inside the owner's lifetime:
        // cannot prove either way, so neither signal nor forget.
        let (pgid, orphan) = leaderless_group(dir.path(), None).await;
        stale(&ctx, "keep", Some(pgid), Some(0));
        let report = recover(
            list_records(&ctx.proc_dir, &ctx.instance_id),
            opts(&ctx, Some(now_ms())),
        )
        .await;
        assert_eq!(report.kept, 1);
        assert!(alive(orphan));
        assert_eq!(list_records(&ctx.proc_dir, &ctx.instance_id).len(), 1);
        kill_now(orphan);
    }

    #[tokio::test]
    async fn another_instances_live_group_is_never_touched() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = ctx(dir.path(), "test-instance-theirs");
        let tree = ProcessTree::prepare(Some(scope("run-theirs")), Some(theirs.clone())).unwrap();
        let mut child = session_cmd("exec sleep 300", tree.tag()).spawn().unwrap();
        tree.attach(child.id());
        let pid = child.id().unwrap() as i32;
        tree.abandon();
        // Our instance shares the data dir but has a different id.
        let records = list_records(&theirs.proc_dir, "test-instance-mine");
        assert!(records.is_empty());
        assert_eq!(
            recover(records, opts(&theirs, Some(now_ms()))).await,
            RecoveryReport::default()
        );
        assert!(alive(pid));
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn a_group_whose_pgid_is_held_by_a_new_leader_after_ours_was_reaped_is_never_signalled() {
        // After the leader is reaped an empty group's pgid can be reused. Model
        // that: the tree is armed on a live group led by another process (the
        // reuser), and our leader has been reported reaped.
        for env_proof in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = ctx_env(dir.path(), "test-instance-revalidate", env_proof);
            let tree = ProcessTree::prepare(Some(scope("run-reval")), Some(ctx.clone())).unwrap();
            let mut impostor = session_cmd("exec sleep 300", None).spawn().unwrap();
            let pid = impostor.id().unwrap() as i32;
            tree.attach(impostor.id());
            tree.leader_reaped();
            assert_eq!(tree.pgid(), Some(pid), "a live group stays armed");
            let outcome = tree.finish_group().await;
            assert_eq!(
                outcome,
                FinishOutcome::Gone,
                "ours is gone; the pgid is someone else's"
            );
            assert!(alive(pid), "the reuser is never signalled by finish");
            drop(tree);
            assert!(alive(pid), "nor by the drop backstop");
            let _ = impostor.start_kill();
            let _ = impostor.wait().await;
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_leaderless_untagged_group_after_the_leader_was_reaped_is_not_signalled() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-untagged");
        let pf = dir.path().join("pids");
        let tree = ProcessTree::prepare(Some(scope("run-untagged")), Some(ctx.clone())).unwrap();
        // Untagged on purpose: readable environments without our tag.
        let mut child = session_cmd(
            &format!("sleep 300 & echo $! > {}; exit 0", pf.display()),
            None,
        )
        .spawn()
        .unwrap();
        tree.attach(child.id());
        child.wait().await.unwrap();
        tree.leader_reaped();
        let orphan = read_pids(&pf, 1).await[0];
        let outcome = tree.finish_group().await;
        assert!(
            matches!(outcome, FinishOutcome::Unresolved(_)),
            "{outcome:?}"
        );
        assert!(alive(orphan), "an unprovable group is never signalled");
        drop(tree);
        assert!(alive(orphan));
        kill_now(orphan);
    }

    #[tokio::test]
    async fn a_group_observed_gone_when_the_leader_is_reaped_is_disarmed() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-disarm");
        let tree = ProcessTree::prepare(Some(scope("run-disarm")), Some(ctx.clone())).unwrap();
        let mut child = session_cmd("exit 0", tree.tag()).spawn().unwrap();
        tree.attach(child.id());
        assert!(tree.pgid().is_some());
        let path = tree.record_path().unwrap();
        child.wait().await.unwrap();
        tree.leader_reaped();
        assert_eq!(tree.pgid(), None, "an empty group is never signalled again");
        assert!(!path.exists(), "and its record is resolved");
    }

    async fn tagged_group_kept_after_the_leader_was_reaped_is_cleaned(env_proof: bool) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx_env(dir.path(), "test-instance-tagged", env_proof);
        let pf = dir.path().join("pids");
        let tree = ProcessTree::prepare(Some(scope("run-tagged")), Some(ctx.clone())).unwrap();
        let mut child = session_cmd(
            &format!("sleep 300 & echo $! > {}; exit 0", pf.display()),
            tree.tag(),
        )
        .spawn()
        .unwrap();
        tree.attach(child.id());
        child.wait().await.unwrap();
        tree.leader_reaped();
        let orphan = read_pids(&pf, 1).await[0];
        assert_eq!(tree.finish_group().await, FinishOutcome::Gone);
        assert!(wait_all_gone(&[orphan], Duration::from_secs(1)).await);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_tagged_group_kept_after_its_leader_was_reaped_is_still_cleaned() {
        tagged_group_kept_after_the_leader_was_reaped_is_cleaned(true).await;
    }

    #[tokio::test]
    async fn a_group_kept_after_its_leader_was_reaped_is_cleaned_by_start_times() {
        tagged_group_kept_after_the_leader_was_reaped_is_cleaned(false).await;
    }

    fn member(pid: i32, zombie: bool) -> crate::util::procinfo::ProcInfo {
        crate::util::procinfo::ProcInfo {
            pid,
            pgid: 4242,
            uid: crate::util::procinfo::current_uid(),
            start_time_ms: 1,
            zombie,
        }
    }

    /// A recheck that still answers EPERM.
    fn still_eperm() -> KillResult {
        KillResult::NotPermitted
    }

    #[test]
    fn eperm_then_reaped_before_listing_is_gone() {
        // macOS: kill(-pgid) is EPERM while the last member is exiting, and
        // its parent reaps it before the members are listed, so none is
        // visible. The fresh probe then answers ESRCH: the group is gone.
        let reaped = || KillResult::NoSuchGroup;
        assert_eq!(
            classify_kill(KillResult::NotPermitted, Vec::new, reaped),
            Probe::Gone
        );
        assert_eq!(
            signal_result(KillResult::NotPermitted, Vec::new, reaped),
            Err(true)
        );
        // A visible live member is never overridden by the recheck.
        let live = || vec![member(4242, false)];
        assert_eq!(
            classify_kill(KillResult::NotPermitted, live, || panic!("no recheck")),
            Probe::Denied
        );
        // Only ESRCH counts: a recheck that now delivers proves nothing.
        assert_eq!(
            classify_kill(KillResult::NotPermitted, Vec::new, || KillResult::Delivered),
            Probe::Denied
        );
    }

    #[test]
    fn eperm_with_zombie_only_members_is_gone() {
        // macOS: kill(-pgid) is EPERM while only unreaped zombies remain.
        let zombies = || vec![member(4242, true), member(4243, true)];
        assert_eq!(
            classify_kill(KillResult::NotPermitted, zombies, still_eperm),
            Probe::Gone
        );
        assert_eq!(
            signal_result(KillResult::NotPermitted, zombies, still_eperm),
            Err(true)
        );
    }

    #[test]
    fn eperm_with_only_exiting_members_is_gone() {
        // macOS right after SIGKILL: the member is listed as SRUN with
        // P_WEXIT, the live lookup may still read it as running, and
        // kill(-pgid) is already EPERM. It can never run again: Gone.
        use crate::util::procinfo::{MACOS_P_WEXIT, macos_exited_or_exiting, merge_group_members};
        const SRUN: u32 = 2;
        let exiting = || {
            let listed = vec![crate::util::procinfo::ProcInfo {
                zombie: macos_exited_or_exiting(SRUN, MACOS_P_WEXIT),
                ..member(4242, false)
            }];
            merge_group_members(4242, listed, |pid| Some(member(pid, false)))
        };
        assert_eq!(
            classify_kill(KillResult::NotPermitted, exiting, still_eperm),
            Probe::Gone
        );
        assert_eq!(
            signal_result(KillResult::NotPermitted, exiting, still_eperm),
            Err(true)
        );
        // A plain running member (no P_WEXIT) is still could-not-clean.
        let running = || {
            let listed = vec![crate::util::procinfo::ProcInfo {
                zombie: macos_exited_or_exiting(SRUN, 0),
                ..member(4242, false)
            }];
            merge_group_members(4242, listed, |pid| Some(member(pid, false)))
        };
        assert_eq!(
            classify_kill(KillResult::NotPermitted, running, still_eperm),
            Probe::Denied
        );
    }

    #[test]
    fn eperm_with_a_live_member_is_still_could_not_clean() {
        let mixed = || vec![member(4242, true), member(4243, false)];
        assert_eq!(
            classify_kill(KillResult::NotPermitted, mixed, still_eperm),
            Probe::Denied
        );
        assert_eq!(
            signal_result(KillResult::NotPermitted, mixed, still_eperm),
            Err(false)
        );
        // Members we cannot see prove nothing: still EPERM.
        assert_eq!(
            classify_kill(KillResult::NotPermitted, Vec::new, still_eperm),
            Probe::Denied
        );
        assert_eq!(
            signal_result(KillResult::NotPermitted, Vec::new, still_eperm),
            Err(false)
        );
    }

    #[test]
    fn other_kill_results_keep_their_meaning() {
        let zombies = || vec![member(4242, true)];
        let live = || vec![member(4242, false)];
        assert_eq!(
            classify_kill(KillResult::Delivered, zombies, still_eperm),
            Probe::Gone
        );
        assert_eq!(
            classify_kill(KillResult::Delivered, live, still_eperm),
            Probe::Alive
        );
        assert_eq!(
            classify_kill(KillResult::NoSuchGroup, live, still_eperm),
            Probe::Gone
        );
        assert_eq!(
            classify_kill(KillResult::Failed, zombies, still_eperm),
            Probe::Denied
        );
        assert_eq!(
            signal_result(KillResult::Delivered, zombies, still_eperm),
            Ok(())
        );
        assert_eq!(
            signal_result(KillResult::NoSuchGroup, live, still_eperm),
            Err(true)
        );
        assert_eq!(
            signal_result(KillResult::Failed, zombies, still_eperm),
            Err(false)
        );
    }

    #[tokio::test]
    async fn finish_deletes_the_record_once_the_group_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-finish");
        let tree = ProcessTree::prepare(Some(scope("run-fin")), Some(ctx.clone())).unwrap();
        let mut child = session_cmd("sleep 300 & exec sleep 300", tree.tag())
            .spawn()
            .unwrap();
        tree.attach(child.id());
        let path = tree.record_path().unwrap();
        let rec: ProcRecord = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rec.pgid, tree.pgid());
        assert!(rec.leader_start_time_ms.is_some());
        let reap = tokio::spawn(async move { child.wait().await });
        assert_eq!(tree.finish_group().await, FinishOutcome::Gone);
        assert!(!path.exists());
        let _ = reap.await;
        drop(tree);
    }
}
