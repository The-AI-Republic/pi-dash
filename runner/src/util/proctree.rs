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
//! * EPERM means "could not clean", never "empty";
//! * crash recovery acts only on records of this daemon's instance id, and
//!   only on a group that still carries our tag (or, where the environment is
//!   unreadable, only started after the recorded leader).
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
}

impl ProcContext {
    pub fn new(instance_id: impl Into<String>, data_dir: &Path) -> Self {
        Self {
            instance_id: instance_id.into(),
            proc_dir: data_dir.join("proc"),
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
}

fn record_path(dir: &Path, id: Uuid) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Write `rec` atomically: tmp file, fsync, rename, fsync the directory.
pub fn write_record(dir: &Path, rec: &ProcRecord) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = record_path(dir, rec.record_id);
    let tmp = dir.join(format!(".{}.json.tmp", rec.record_id));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&serde_json::to_vec(rec).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(path)
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

/// `kill(-pgid, 0)`, refined so a group holding only zombies counts as gone
/// (they cannot run again, and waiting on them would only time out).
pub fn probe_group(pgid: i32) -> Probe {
    #[cfg(unix)]
    {
        use nix::errno::Errno;
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        if pgid <= 1 {
            return Probe::Denied;
        }
        match kill(Pid::from_raw(-pgid), None) {
            Ok(()) => {
                let members = crate::util::procinfo::group_members(pgid);
                if !members.is_empty() && members.iter().all(|m| m.zombie) {
                    Probe::Gone
                } else {
                    Probe::Alive
                }
            }
            Err(Errno::ESRCH) => Probe::Gone,
            Err(_) => Probe::Denied,
        }
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

/// Signal a whole group. `Err(true)` on ESRCH (gone), `Err(false)` otherwise.
fn signal_group(pgid: i32, sig: Sig) -> Result<(), bool> {
    #[cfg(unix)]
    {
        use nix::errno::Errno;
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;
        if pgid <= 1 {
            return Err(false);
        }
        let s = match sig {
            Sig::Term => Signal::SIGTERM,
            Sig::Kill => Signal::SIGKILL,
        };
        match killpg(Pid::from_raw(pgid), s) {
            Ok(()) => Ok(()),
            Err(Errno::ESRCH) => Err(true),
            Err(_) => Err(false),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (pgid, sig);
        Err(true)
    }
}

/// Poll `probe_group` until it stops reporting `Alive` or `bound` passes.
async fn wait_group(pgid: i32, bound: Duration) -> Probe {
    let deadline = tokio::time::Instant::now() + bound;
    loop {
        let p = probe_group(pgid);
        if p != Probe::Alive || tokio::time::Instant::now() >= deadline {
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
    /// recovery uses ([`group_identity`]): a member carries our tag, or,
    /// without a readable tag, every member started at or after our leader.
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
            match group_identity(pgid, self.tag.as_deref(), leader_start) {
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
    accepting: AtomicBool,
    live: Mutex<HashMap<Uuid, Weak<TreeInner>>>,
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| Registry {
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

/// Daemon shutdown: stop accepting spawns, snapshot every live tree and
/// finish them in parallel, bounded by `bound`. Returns how many trees were
/// still unresolved.
pub async fn shutdown_all(bound: Duration) -> usize {
    REGISTRY.accepting.store(false, Ordering::SeqCst);
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
/// SIGKILL, poll ≤ 2 s.
async fn clean_foreign_group(pgid: i32) -> bool {
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

/// Is the live group `pgid` still the one whose leader started at
/// `leader_start` and whose members carry `tag`? `Some(true)`: a member
/// carries the tag, or no member's environment could be checked and every
/// member started at or after the leader. `Some(false)`: a member is older
/// than the leader, so the pgid was reused. `None`: cannot prove either way
/// (or no live member is left). Used by crash recovery and, once the leader
/// is reaped, before every signal of a live tree.
fn group_identity(pgid: i32, tag: Option<&str>, leader_start: Option<u64>) -> Option<bool> {
    let members: Vec<_> = crate::util::procinfo::group_members(pgid)
        .into_iter()
        .filter(|m| !m.zombie)
        .collect();
    if members.is_empty() {
        return None;
    }
    let mut any_readable = false;
    if let Some(tag) = tag {
        for m in &members {
            match tagged(m.pid, tag) {
                Some(true) => return Some(true),
                Some(false) => any_readable = true,
                None => {}
            }
        }
    }
    let leader_start = leader_start?;
    let all_after = members.iter().all(|m| m.start_time_ms >= leader_start);
    if !any_readable && all_after {
        return Some(true);
    }
    if !all_after {
        // A member older than our leader: the pgid was reused, so our group
        // is gone.
        return Some(false);
    }
    // Readable, untagged, but started after our leader: cannot prove either
    // way. Keep the record and retry later.
    None
}

/// Crash recovery for `records` (already filtered to our instance id by
/// [`list_records`]; call it before any new spawn so live records are not in
/// the list). Deletes a record only once its group is confirmed gone.
pub async fn recover(records: Vec<(PathBuf, ProcRecord)>) -> RecoveryReport {
    // Records are independent, so resolve them in parallel: the whole pass
    // takes about one record's worst case (SIGTERM 3 s + SIGKILL 2 s), which
    // keeps daemon start short while it waits for recovery.
    let results = futures_util::future::join_all(records.into_iter().map(recover_one)).await;
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

async fn recover_one((path, rec): (PathBuf, ProcRecord)) -> Recovered {
    let outcome = match rec.pgid {
        // Crash between record and spawn (or before the pgid was filled
        // in): resolve by tag alone.
        None => {
            if clean_tagged_pids(&rec.tag).await {
                Recovered::Resolved
            } else {
                Recovered::Kept
            }
        }
        Some(pgid) => match probe_group(pgid) {
            Probe::Gone => Recovered::Resolved,
            Probe::Denied => Recovered::Kept,
            Probe::Alive => match group_identity(pgid, Some(&rec.tag), rec.leader_start_time_ms) {
                Some(true) => {
                    tracing::info!(pgid, record_id = %rec.record_id, "recovering leftover agent process group");
                    if clean_foreign_group(pgid).await {
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
            },
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

    #[tokio::test]
    async fn recovery_cleans_a_leaderless_group_left_by_a_crashed_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-crash");
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
        // Crash: no finish, no drop backstop.
        tree.abandon();

        let records = list_records(&ctx.proc_dir, &ctx.instance_id);
        assert_eq!(records.len(), 1, "unresolved record survives the crash");
        assert_eq!(records[0].1.pgid, Some(pgid));
        let report = recover(records).await;
        assert_eq!(report.cleaned, 1, "{report:?}");
        assert!(wait_all_gone(&[orphan], Duration::from_secs(1)).await);
        assert!(list_records(&ctx.proc_dir, &ctx.instance_id).is_empty());
    }

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
        let report = recover(records).await;
        assert_eq!(report.kept, 0, "{report:?}");
        let _ = child.wait().await;
        assert!(!alive(pid));
        assert!(list_records(&ctx.proc_dir, &ctx.instance_id).is_empty());
    }

    #[tokio::test]
    async fn recovery_ignores_a_reused_pgid_whose_members_are_not_ours() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-reuse");
        // A group that is alive, untagged, and older than the "recorded"
        // leader: its pgid was reused after our group died.
        let mut child = session_cmd("exec sleep 300", None).spawn().unwrap();
        let pid = child.id().unwrap() as i32;
        let rec = ProcRecord {
            record_id: Uuid::new_v4(),
            instance_id: ctx.instance_id.clone(),
            runner_id: None,
            kind: ScopeKind::Run,
            run_or_chat_id: "old".into(),
            tag: format!("{}:run:old", ctx.instance_id),
            scratch: None,
            pgid: Some(pid),
            leader_start_time_ms: Some(u64::MAX / 2),
        };
        write_record(&ctx.proc_dir, &rec).unwrap();
        let report = recover(list_records(&ctx.proc_dir, &ctx.instance_id)).await;
        assert_eq!(report.cleaned, 0);
        assert_eq!(report.resolved, 1);
        assert!(
            alive(pid),
            "a group we cannot prove is ours is never signalled"
        );
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn an_unprovable_record_is_kept_for_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-keep");
        // Untagged but started after the recorded leader: cannot prove
        // either way, so neither signal nor forget.
        let mut child = session_cmd("exec sleep 300", None).spawn().unwrap();
        let pid = child.id().unwrap() as i32;
        let rec = ProcRecord {
            record_id: Uuid::new_v4(),
            instance_id: ctx.instance_id.clone(),
            runner_id: None,
            kind: ScopeKind::Run,
            run_or_chat_id: "keep".into(),
            tag: format!("{}:run:keep", ctx.instance_id),
            scratch: None,
            pgid: Some(pid),
            leader_start_time_ms: Some(0),
        };
        write_record(&ctx.proc_dir, &rec).unwrap();
        let report = recover(list_records(&ctx.proc_dir, &ctx.instance_id)).await;
        assert_eq!(report.kept, 1);
        assert!(alive(pid));
        assert_eq!(list_records(&ctx.proc_dir, &ctx.instance_id).len(), 1);
        let _ = child.start_kill();
        let _ = child.wait().await;
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
        assert_eq!(recover(records).await, RecoveryReport::default());
        assert!(alive(pid));
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    #[tokio::test]
    async fn a_group_kept_after_its_leader_was_reaped_is_revalidated_before_any_signal() {
        // After the leader is reaped an empty group's pgid can be reused. Model
        // that: the tree is armed on a live group that is untagged (the
        // reuser), and the leader has been reported reaped.
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-revalidate");
        let tree = ProcessTree::prepare(Some(scope("run-reval")), Some(ctx.clone())).unwrap();
        let mut impostor = session_cmd("exec sleep 300", None).spawn().unwrap();
        let pid = impostor.id().unwrap() as i32;
        tree.attach(impostor.id());
        tree.leader_reaped();
        assert_eq!(tree.pgid(), Some(pid), "a live group stays armed");
        let outcome = tree.finish_group().await;
        assert!(
            matches!(outcome, FinishOutcome::Unresolved(_)),
            "{outcome:?}"
        );
        assert!(
            alive(pid),
            "an unprovable group is never signalled by finish"
        );
        drop(tree);
        assert!(alive(pid), "nor by the drop backstop");
        let _ = impostor.start_kill();
        let _ = impostor.wait().await;
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

    #[tokio::test]
    async fn a_tagged_group_kept_after_its_leader_was_reaped_is_still_cleaned() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path(), "test-instance-tagged");
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
