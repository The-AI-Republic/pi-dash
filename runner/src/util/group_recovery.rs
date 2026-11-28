//! Crash recovery for the process-group backend (PDASHOSS01-417, part 3).
//!
//! The cgroup backend recovers after a daemon crash by sweeping its
//! leftover `pidash-run-*` cgroups. The process-group backend (macOS) had
//! nothing: a group id is only a number, so a daemon that died mid-run left
//! its agents' processes running, and a later daemon could not tell the
//! run's group from a stranger's that reused the id.
//!
//! Each anchored run group (see [`crate::util::group_ownership`]) gets a
//! record in `<data_dir>/run_groups/<run_id>.json`: the group id, the
//! anchor's pid and exact start time, and the pid of the daemon that owns
//! it. The record is removed once the anchor is reaped, so only a daemon
//! that never finished cleanup leaves one behind. On macOS the anchor
//! outlives such a daemon and keeps the group id reserved, because it ends
//! only by SIGKILL ([`crate::util::group_ownership::anchor_script`]).
//!
//! At daemon start every record is judged before anything is signalled:
//! * the anchor must still be alive with the recorded pid, exact start
//!   time and group, or the group cannot be proven and is left running;
//! * the anchor's parent must no longer be the recorded daemon. The anchor
//!   is that daemon's direct child and is reparented only when it dies, so
//!   a second daemon on the same data dir never stops the groups of a live
//!   one. Such records are left for their owner.
//!
//! A proven group then gets the run-end treatment, TERM → grace → KILL,
//! and every signal re-proves the group from a fresh snapshot first
//! (review item 5 on #2952): the anchor is re-read after each scan with its
//! exact identity, and a group that no longer proves is left running. The
//! anchor itself is stopped last, after one more check of its identity, and
//! the record is removed. A group that cannot be emptied keeps its record
//! (and its anchor) for the next start.
//!
//! Only macOS recovers ([`RECOVERS`]). Linux exposes start times in clock
//! ticks, which are a fingerprint, not proof (review of #3496), and an
//! orphaned anchor is no longer anyone's unreaped child, so on Linux the
//! anchor exits with its daemon and no record is written; Linux runs are
//! recovered by the cgroup sweep.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::util::containment::CleanupOutcome;
use crate::util::group_ownership::{GroupOwner, ProcId};
use crate::util::procinfo::{self, ProcInfo};

/// Whether this platform records anchored groups and recovers them after a
/// crash (see the module doc).
pub const RECOVERS: bool = cfg!(target_os = "macos");

const RECORD_DIR: &str = "run_groups";

static INSTALLED_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where records live under the daemon's data dir.
pub fn record_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(RECORD_DIR)
}

/// Record every anchored group this daemon creates from now on in
/// `data_dir`. Called once at daemon start; a no-op where groups are not
/// recovered.
pub fn install(data_dir: &Path) {
    if RECOVERS {
        let _ = INSTALLED_DIR.set(record_dir(data_dir));
    }
}

/// A run group as the daemon that created it recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupRecord {
    pub run_id: Uuid,
    pub pgid: i32,
    pub anchor_pid: i32,
    pub anchor_start: u64,
    /// Pid of the daemon whose child the anchor is.
    pub owner_pid: i32,
    /// The runner the group's run belongs to, when known: an unresolved
    /// recovery keeps that runner reserved (every runner when unknown).
    #[serde(default)]
    pub runner_id: Option<Uuid>,
    /// Members (pid, exact start) that survived a forced cleanup that also
    /// ended the anchor. While one of them is alive with that identity the
    /// group id is still reserved by it, so the group is still the run's:
    /// it is tracked (never signalled again, nothing pins it through
    /// delivery, and their SIGKILL is already pending) until they are gone.
    #[serde(default)]
    pub survivors: Vec<(i32, u64)>,
}

impl GroupRecord {
    fn anchor(&self) -> ProcId {
        ProcId {
            pid: self.anchor_pid,
            start: self.anchor_start,
        }
    }
}

/// Record a freshly anchored group if [`install`] was called. Dropping the
/// owner's anchor removes the record.
pub fn record_new_group(run_id: Uuid, runner_id: Option<Uuid>, owner: &mut GroupOwner) {
    if let Some(dir) = INSTALLED_DIR.get() {
        record_in(dir, run_id, runner_id, owner);
    }
}

/// [`record_new_group`] into `dir`. A record that cannot be written only
/// costs recovery for this run; the run itself goes ahead.
pub(crate) fn record_in(dir: &Path, run_id: Uuid, runner_id: Option<Uuid>, owner: &mut GroupOwner) {
    let Some(anchor) = owner.anchor_identity() else {
        return;
    };
    let record = GroupRecord {
        run_id,
        pgid: owner.pgid(),
        anchor_pid: anchor.pid,
        anchor_start: anchor.start,
        owner_pid: std::process::id() as i32,
        runner_id,
        survivors: Vec::new(),
    };
    match write_record(dir, &record) {
        Ok(path) => owner.set_record(path),
        Err(e) => tracing::warn!(
            run_id = %run_id, error = %e,
            "run group record not written; this run's processes cannot be recovered after a crash"
        ),
    }
}

/// Rewrite the record at `path` atomically.
fn write_record_at(path: &Path, record: &GroupRecord) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let written = write_record(dir, record)?;
    if written != path {
        std::fs::rename(&written, path)?;
    }
    Ok(())
}

fn record_path(dir: &Path, run_id: Uuid) -> PathBuf {
    dir.join(format!("{run_id}.json"))
}

/// Write atomically (temp file, then rename), so a crash never leaves a
/// half-written record.
fn write_record(dir: &Path, record: &GroupRecord) -> std::io::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let path = record_path(dir, record.run_id);
    let tmp = dir.join(format!(".{}.tmp", record.run_id));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec(record).map_err(std::io::Error::other)?)?;
    f.sync_all()?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// Every record file in `dir`, parsed (`None`: unreadable or malformed).
fn list_records(dir: &Path) -> Vec<(PathBuf, Option<GroupRecord>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let record = std::fs::read(&p)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            (p, record)
        })
        .collect()
}

/// What a record proves about its group right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The anchor still belongs to the live daemon that recorded it.
    OwnerAlive,
    /// The anchor is gone or is not the recorded process: the group cannot
    /// be proven and is never signalled.
    Unproven(&'static str),
    /// The recorded anchor is alive, holds the group, and its daemon died.
    Orphaned,
    /// The anchor is gone, but a recorded survivor of a forced cleanup is
    /// alive with its exact identity and still holds the group: the group
    /// is still the run's and not empty, but nothing pins it through
    /// delivery, so it is only tracked, never signalled.
    Survivors,
    /// A process the record names could not be inspected: nothing is
    /// proven either way, so the record is kept and nothing is signalled.
    Unknown(&'static str),
}

/// [`judge_read`] over a reader that cannot fail (tests).
#[cfg(test)]
fn judge(record: &GroupRecord, read: impl Fn(i32) -> Option<(ProcInfo, i32)>) -> Verdict {
    judge_read(record, |pid| Ok(read(pid)))
}

/// Judge `record` from one read of its anchor (`read`: process info and
/// parent pid from the same kernel record; `Ok(None)`: the process is gone,
/// `Err`: it could not be inspected).
fn judge_read(
    record: &GroupRecord,
    read: impl Fn(i32) -> std::io::Result<Option<(ProcInfo, i32)>>,
) -> Verdict {
    if !record.survivors.is_empty() {
        let mut unknown = false;
        let held = record
            .survivors
            .iter()
            .any(|&(pid, start)| match read(pid) {
                Ok(found) => found
                    .is_some_and(|(p, _)| p.start == start && p.pgid == record.pgid && !p.zombie),
                Err(_) => {
                    unknown = true;
                    false
                }
            });
        return if held {
            Verdict::Survivors
        } else if unknown {
            Verdict::Unknown("a survivor of its forced cleanup could not be inspected")
        } else {
            Verdict::Unproven("the survivors of its forced cleanup are gone")
        };
    }
    let (anchor, parent) = match read(record.anchor_pid) {
        Ok(Some(found)) => found,
        Ok(None) => return Verdict::Unproven("anchor is gone"),
        Err(_) => return Verdict::Unknown("anchor could not be inspected"),
    };
    if ProcId::from(&anchor) != record.anchor()
        || anchor.pgid != record.pgid
        || anchor.pgid != anchor.pid
    {
        return Verdict::Unproven("anchor pid now belongs to another process");
    }
    if anchor.zombie {
        return Verdict::Unproven("anchor has exited");
    }
    if parent == record.owner_pid {
        return Verdict::OwnerAlive;
    }
    Verdict::Orphaned
}

/// What recovery did with one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Its daemon is still alive: left to it, record kept.
    LeftToOwner,
    /// Ownership could not be proven: nothing signalled, record removed.
    LeftRunning,
    /// The orphaned group was stopped (or could not be emptied: then the
    /// record is kept for the next start).
    Stopped(CleanupOutcome),
    /// The record could not be parsed: removed.
    Discarded,
}

impl Recovery {
    /// Whether the group may still have the run's processes in it: its
    /// record is kept and its runner stays reserved.
    pub fn is_unresolved(self) -> bool {
        matches!(self, Recovery::Stopped(CleanupOutcome::Unresolved))
    }
}

/// Groups whose recovery is unresolved, for runner admission: a runner a
/// held group affects stays reserved, since the old run's processes may
/// still use its working copy.
#[derive(Debug, Default)]
pub struct RecoveryHold {
    dir: PathBuf,
    held: Vec<GroupRecord>,
}

impl RecoveryHold {
    /// Whether nothing is held.
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// Whether a held group may belong to `runner_id` (a record without a
    /// runner may belong to any).
    pub fn affects(&self, runner_id: Uuid) -> bool {
        self.held
            .iter()
            .any(|r| r.runner_id.is_none_or(|id| id == runner_id))
    }

    /// A hold of records for `runners` (tests only).
    #[cfg(test)]
    pub fn for_tests(runners: Vec<Option<Uuid>>) -> Self {
        Self {
            dir: PathBuf::new(),
            held: runners
                .into_iter()
                .map(|runner_id| GroupRecord {
                    run_id: Uuid::new_v4(),
                    pgid: 0,
                    anchor_pid: 0,
                    anchor_start: 0,
                    owner_pid: 0,
                    runner_id,
                    survivors: Vec::new(),
                })
                .collect(),
        }
    }

    /// Recover the held records again; keep those still unresolved.
    pub async fn recheck(&mut self) {
        let dir = self.dir.clone();
        match recover_dir(dir, crate::util::containment::CLEANUP_GRACE).await {
            Some(done) => self.retain_unresolved(done),
            // Skipped (another daemon holds the lock) or failed: nothing was
            // proven, so every record still on disk stays held (review of
            // #3501 at ba9c1c3, MUST 1). The next recheck tries again.
            None => {
                self.held = list_records(&self.dir)
                    .into_iter()
                    .filter_map(|(_, record)| record)
                    .collect();
            }
        }
    }

    fn retain_unresolved(&mut self, done: Vec<(PathBuf, Recovery, Option<GroupRecord>)>) {
        self.held = done
            .into_iter()
            .filter(|(_, r, _)| r.is_unresolved())
            .filter_map(|(_, _, record)| record)
            .collect();
    }
}

/// Recover every group a dead daemon left in `data_dir`. Runs at daemon
/// start, before any run; a no-op where groups are not recovered. The
/// groups that could not be resolved are returned so their runners stay
/// reserved (review of #3501 at 1524d8e).
///
/// `containment_on` is false under `PIDASH_CONTAINMENT=off`: that opt-out
/// also stops startup recovery from signalling anything (as it stops the
/// cgroup sweep). The records stay on disk for a later start with
/// containment on.
pub async fn recover_orphaned_groups(data_dir: &Path, containment_on: bool) -> RecoveryHold {
    recover_in(record_dir(data_dir), RECOVERS, containment_on).await
}

/// [`recover_orphaned_groups`] for one record dir; `recovers` is
/// [`RECOVERS`] outside tests.
async fn recover_in(dir: PathBuf, recovers: bool, containment_on: bool) -> RecoveryHold {
    let mut hold = RecoveryHold {
        dir,
        held: Vec::new(),
    };
    if !recovers {
        return hold;
    }
    if !containment_on {
        if !list_records(&hold.dir).is_empty() {
            tracing::warn!(
                "PIDASH_CONTAINMENT=off: process groups a previous daemon left are not recovered"
            );
        }
        return hold;
    }
    hold.recheck().await;
    hold
}

/// [`recover_orphaned_groups`] for one record dir.
/// Only one daemon recovers a record dir at a time (`.recovery.lock`, held
/// for the pass): two daemons starting together after a crash would
/// otherwise both adopt a group, and one could kill an anchor the other
/// had just proven. A daemon that finds the lock held skips recovery:
/// `None`, as when the pass fails, so callers never read a skipped pass as
/// an empty one.
pub(crate) async fn recover_dir(
    dir: PathBuf,
    grace: Duration,
) -> Option<Vec<(PathBuf, Recovery, Option<GroupRecord>)>> {
    tokio::task::spawn_blocking(move || {
        let _lock = match recovery_lock(&dir) {
            Lock::Held(file) => file,
            Lock::NothingToRecover => return Some(Vec::new()),
            Lock::Unavailable => return None,
        };
        let done = list_records(&dir)
            .into_iter()
            .map(|(path, record)| {
                let done = match &record {
                    Some(record) => recover_record(&path, record, grace),
                    None => {
                        tracing::warn!(path = %path.display(), "unreadable run group record discarded");
                        let _ = std::fs::remove_file(&path);
                        Recovery::Discarded
                    }
                };
                // The record as it is now on disk (a forced cleanup may
                // have rewritten it with its survivors).
                let record = record.map(|r| read_record(&path).unwrap_or(r));
                (path, done, record)
            })
            .collect();
        Some(done)
    })
    .await
    .ok()
    .flatten()
}

fn read_record(path: &Path) -> Option<GroupRecord> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

const RECOVERY_LOCK: &str = ".recovery.lock";

/// The outcome of taking the recovery lock of a record dir.
enum Lock {
    /// Held for the pass.
    Held(std::fs::File),
    /// No record dir: nothing to recover.
    NothingToRecover,
    /// Another daemon holds it, or it could not be opened.
    Unavailable,
}

/// The exclusive recovery lock of `dir`.
fn recovery_lock(dir: &Path) -> Lock {
    use fs2::FileExt;
    if !dir.is_dir() {
        return Lock::NothingToRecover;
    }
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join(RECOVERY_LOCK))
    {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!(dir = %dir.display(), error = %e,
                "run group recovery skipped: lock unavailable");
            return Lock::Unavailable;
        }
    };
    match file.try_lock_exclusive() {
        Ok(()) => Lock::Held(file),
        Err(e) => {
            tracing::warn!(dir = %dir.display(), error = %e,
                "run group recovery skipped: another daemon is recovering");
            Lock::Unavailable
        }
    }
}

fn recover_record(path: &Path, record: &GroupRecord, grace: Duration) -> Recovery {
    recover_record_with(path, record, grace, procinfo::read_with_parent)
}

/// [`recover_record`] judging the record through `read`.
fn recover_record_with(
    path: &Path,
    record: &GroupRecord,
    grace: Duration,
    read: impl Fn(i32) -> std::io::Result<Option<(ProcInfo, i32)>>,
) -> Recovery {
    match judge_read(record, read) {
        Verdict::OwnerAlive => {
            tracing::debug!(run_id = %record.run_id, owner_pid = record.owner_pid,
                "run group belongs to a live daemon; left alone");
            Recovery::LeftToOwner
        }
        Verdict::Unproven(why) => {
            tracing::warn!(run_id = %record.run_id, pgid = record.pgid, reason = why,
                "orphaned run group cannot be proven to belong to the run; left running");
            let _ = std::fs::remove_file(path);
            Recovery::LeftRunning
        }
        Verdict::Unknown(why) => {
            tracing::warn!(run_id = %record.run_id, pgid = record.pgid, reason = why,
                "orphaned run group cannot be judged; record kept, nothing signalled");
            Recovery::Stopped(CleanupOutcome::Unresolved)
        }
        Verdict::Orphaned => Recovery::Stopped(stop_adopted(path, record, grace)),
        Verdict::Survivors => {
            tracing::warn!(run_id = %record.run_id, pgid = record.pgid,
                "orphaned run group still has survivors of its forced cleanup; record kept");
            Recovery::Stopped(CleanupOutcome::Unresolved)
        }
    }
}

/// What to keep of `record` after a forced cleanup that left its group
/// unresolved (review of #3501 at 1524d8e): `verdict` judges the anchor
/// now, `survivors` are the members a fresh proven scan still found. While
/// the anchor lingers the record stays as it is. Once the group's SIGKILL
/// has also ended the anchor, the record can only prove the group through
/// its survivors, so it is rewritten with them; with none, nothing can
/// prove the group any more and the record is dropped (`None`).
fn after_forced_cleanup(
    record: &GroupRecord,
    verdict: Verdict,
    survivors: Vec<(i32, u64)>,
) -> Option<GroupRecord> {
    if matches!(verdict, Verdict::Orphaned | Verdict::Unknown(_)) {
        return Some(record.clone());
    }
    if survivors.is_empty() {
        return None;
    }
    Some(GroupRecord {
        survivors,
        ..record.clone()
    })
}

/// Keep a live daemon's group provable after a forced cleanup that left it
/// unresolved: `live` are the members a fresh proven scan still found. The
/// group's SIGKILL also ends its anchor, and once the anchor is gone the
/// record can only prove the group through those members, so they are
/// written into it (as [`stop_adopted`] does during recovery). While the
/// anchor is still alive the record is left as it is.
pub(crate) fn keep_survivors(owner: &GroupOwner, live: &[ProcInfo]) {
    let (Some(path), Some(anchor)) = (owner.record_path(), owner.anchor_identity()) else {
        return;
    };
    if live.is_empty() {
        return;
    }
    let anchor_alive = procinfo::info(anchor.pid)
        .is_some_and(|p| ProcId::from(&p) == anchor && p.pgid == owner.pgid() && !p.zombie);
    if anchor_alive {
        return;
    }
    let Some(record) = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<GroupRecord>(&b).ok())
    else {
        return;
    };
    let kept = GroupRecord {
        survivors: live.iter().map(|p| (p.pid, p.start)).collect(),
        ..record
    };
    if let Err(e) = write_record_at(path, &kept) {
        tracing::warn!(run_id = %kept.run_id, error = %e,
            "survivors of the forced cleanup not recorded");
    }
}

/// Stop an orphaned group judged [`Verdict::Orphaned`]. Every signal
/// re-proves the group from a fresh snapshot (`stop_owned_group`), so an
/// anchor that changed after the judgement stops everything.
fn stop_adopted(path: &Path, record: &GroupRecord, grace: Duration) -> CleanupOutcome {
    let owner = Mutex::new(GroupOwner::adopt(record.pgid, record.anchor()));
    let (stopped, outcome) =
        crate::util::containment::stop_owned_group(record.run_id, &owner, grace);
    crate::util::containment::cleanup_report(record.run_id, "process-group-recovery", &stopped);
    if outcome != CleanupOutcome::Clean {
        let survivors = match crate::util::containment::lock_owner(&owner).observe() {
            crate::util::group_ownership::Proof::Ours(live) => {
                live.iter().map(|p| (p.pid, p.start)).collect()
            }
            _ => Vec::new(),
        };
        let verdict = judge_read(record, procinfo::read_with_parent);
        match after_forced_cleanup(record, verdict, survivors) {
            Some(kept) => {
                if kept != *record
                    && let Err(e) = write_record_at(path, &kept)
                {
                    tracing::warn!(run_id = %record.run_id, error = %e,
                        "survivors of the forced cleanup not recorded");
                }
                tracing::warn!(run_id = %record.run_id, pgid = record.pgid,
                    "orphaned run group could not be emptied; kept for the next check");
            }
            None => {
                tracing::warn!(run_id = %record.run_id, pgid = record.pgid,
                    "orphaned run group could not be emptied and can no longer be proven; left running");
                let _ = std::fs::remove_file(path);
            }
        }
        return outcome;
    }
    // The group is empty but the lingering anchor may still hold it. Stop
    // it only while it is still the recorded, orphaned anchor; it ends only
    // by SIGKILL, so it cannot be replaced between this read and the kill.
    match judge_read(record, procinfo::read_with_parent) {
        Verdict::Orphaned => send_kill(record.anchor_pid),
        // The anchor cannot be judged: keep the record for the next check.
        Verdict::Unknown(_) => return outcome,
        _ => {}
    }
    let _ = std::fs::remove_file(path);
    outcome
}

#[cfg(unix)]
fn send_kill(pid: i32) {
    if pid > 1 {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

#[cfg(not(unix))]
fn send_kill(_pid: i32) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> GroupRecord {
        GroupRecord {
            run_id: Uuid::nil(),
            pgid: 10,
            anchor_pid: 10,
            anchor_start: 500,
            owner_pid: 77,
            runner_id: None,
            survivors: Vec::new(),
        }
    }

    fn anchor(pid: i32, pgid: i32, start: u64, zombie: bool) -> ProcInfo {
        ProcInfo {
            pid,
            pgid,
            start,
            zombie,
            name: "sleep".into(),
        }
    }

    /// Review of #3501 at 1524d8e (MUST 3): the group's SIGKILL also ends
    /// the lingering anchor, so a record kept for a group that could not be
    /// emptied must still prove that group at the next check: through the
    /// survivors' exact identities, which keep the group id reserved.
    #[test]
    fn a_forced_cleanup_keeps_the_group_provable_through_its_survivors() {
        let r = rec();
        let gone = Verdict::Unproven("anchor is gone");
        let kept = after_forced_cleanup(&r, gone, vec![(11, 600)]).expect("record kept");
        let survivor = |start: u64, pgid: i32| {
            move |pid: i32| (pid == 11).then(|| (anchor(11, pgid, start, false), 1))
        };
        assert_eq!(judge(&kept, survivor(600, 10)), Verdict::Survivors);
        // A reused pid, a moved process, a dead survivor prove nothing.
        assert!(matches!(
            judge(&kept, survivor(601, 10)),
            Verdict::Unproven(_)
        ));
        assert!(matches!(
            judge(&kept, survivor(600, 12)),
            Verdict::Unproven(_)
        ));
        assert!(matches!(judge(&kept, |_| None), Verdict::Unproven(_)));
        // While the anchor lingers the record stays as it is; with neither
        // anchor nor survivors there is nothing left to prove the group.
        assert_eq!(
            after_forced_cleanup(&r, Verdict::Orphaned, vec![(11, 600)]),
            Some(r.clone())
        );
        assert_eq!(after_forced_cleanup(&r, gone, Vec::new()), None);
    }

    /// Review of #3501 at 1524d8e (MUST 2): a group whose recovery is
    /// unresolved keeps its runner reserved (every runner when the record
    /// names none) until a re-check resolves it.
    #[test]
    fn an_unresolved_recovery_holds_its_runner() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut for_a = rec();
        for_a.runner_id = Some(a);
        let mut hold = RecoveryHold::default();
        hold.retain_unresolved(vec![
            (
                PathBuf::from("a"),
                Recovery::Stopped(CleanupOutcome::Unresolved),
                Some(for_a.clone()),
            ),
            (PathBuf::from("c"), Recovery::LeftRunning, Some(rec())),
        ]);
        assert!(hold.affects(a) && !hold.affects(b));
        hold.retain_unresolved(vec![(
            PathBuf::from("n"),
            Recovery::Stopped(CleanupOutcome::Unresolved),
            Some(rec()),
        )]);
        assert!(
            hold.affects(a) && hold.affects(b),
            "an unknown runner holds all"
        );
        hold.retain_unresolved(vec![(
            PathBuf::from("a"),
            Recovery::Stopped(CleanupOutcome::Clean),
            Some(for_a),
        )]);
        assert!(hold.is_empty() && !hold.affects(a));
    }

    #[test]
    fn a_record_is_judged_from_its_anchor_identity_and_parent() {
        let r = rec();
        let read = |info: Option<(ProcInfo, i32)>| move |_pid: i32| info.clone();
        assert_eq!(
            judge(&r, read(Some((anchor(10, 10, 500, false), 1)))),
            Verdict::Orphaned
        );
        assert_eq!(
            judge(&r, read(Some((anchor(10, 10, 500, false), 77)))),
            Verdict::OwnerAlive
        );
        assert!(matches!(judge(&r, read(None)), Verdict::Unproven(_)));
        // A reused pid (another start), a moved process, an exited anchor.
        for (info, parent) in [
            (anchor(10, 10, 501, false), 1),
            (anchor(10, 12, 500, false), 1),
            (anchor(10, 10, 500, true), 1),
        ] {
            assert!(
                matches!(judge(&r, read(Some((info, parent)))), Verdict::Unproven(_)),
                "must not prove"
            );
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod process_tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    const GRACE: Duration = Duration::from_millis(300);

    /// A lingering anchor (as macOS spawns it) leading a new group.
    fn spawn_anchor() -> (Child, ProcInfo) {
        spawn_anchor_with_path(&std::env::var("PATH").unwrap_or_default())
    }

    /// [`spawn_anchor`] resolving `sleep` through `path`.
    fn spawn_anchor_with_path(path: &str) -> (Child, ProcInfo) {
        let child = Command::new("/bin/sh")
            .args(["-c", crate::util::group_ownership::anchor_script(true)])
            .env("PATH", path)
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let info = procinfo::info(child.id() as i32).unwrap();
        (child, info)
    }

    fn spawn_member(pgid: i32) -> Child {
        Command::new("sleep")
            .arg("30")
            .process_group(pgid)
            .spawn()
            .unwrap()
    }

    /// The (path, outcome) pairs of a recovery pass.
    fn outcomes(
        done: Option<Vec<(PathBuf, Recovery, Option<GroupRecord>)>>,
    ) -> Vec<(PathBuf, Recovery)> {
        let done = done.expect("the recovery pass was skipped");
        done.into_iter().map(|(p, r, _)| (p, r)).collect()
    }

    /// The pid of a process that has exited and been reaped: a daemon that
    /// is gone.
    fn dead_pid() -> i32 {
        let mut c = Command::new("true").spawn().unwrap();
        let pid = c.id() as i32;
        c.wait().unwrap();
        pid
    }

    /// Close the anchor's stdin, as a dying daemon does, and wait until it
    /// lingers: its `sleep` child has joined the group.
    fn orphan(anchor: &mut Child) {
        drop(anchor.stdin.take());
        let pid = anchor.id() as i32;
        for _ in 0..50 {
            if procinfo::group_members(pid).is_ok_and(|m| m.iter().any(|p| p.pid != pid)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("anchor did not linger");
    }

    fn record_for(anchor: &ProcInfo, owner_pid: i32) -> GroupRecord {
        GroupRecord {
            run_id: Uuid::new_v4(),
            pgid: anchor.pgid,
            anchor_pid: anchor.pid,
            anchor_start: anchor.start,
            owner_pid,
            runner_id: None,
            survivors: Vec::new(),
        }
    }

    fn running(child: &mut Child) -> bool {
        child.try_wait().unwrap().is_none()
    }

    /// Reap `child`, killing it first if it is still running.
    fn reap(child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Reap a test anchor, killing its whole group first so its lingering
    /// `sleep` does not outlive the test.
    fn reap_anchor(anchor: &mut Child) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(anchor.id() as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
        let _ = anchor.wait();
    }

    #[tokio::test]
    async fn an_orphaned_group_is_stopped_at_start_and_its_record_removed() {
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let mut member = spawn_member(a.pgid);
        let record = record_for(&a, dead_pid());
        let path = write_record(dir.path(), &record).unwrap();
        orphan(&mut anchor);
        // The lingering anchor ignores TERM: only KILL ends it.
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(a.pid),
            nix::sys::signal::Signal::SIGTERM,
        );
        std::thread::sleep(Duration::from_millis(100));
        assert!(running(&mut anchor), "the anchor must survive TERM");

        let done = outcomes(recover_dir(dir.path().to_path_buf(), GRACE).await);

        assert_eq!(
            done,
            vec![(path.clone(), Recovery::Stopped(CleanupOutcome::Clean))]
        );
        let status = member.wait().unwrap();
        assert!(!status.success(), "the member must have been signalled");
        let _ = anchor.wait();
        assert!(!path.exists(), "the record must be removed");
    }

    /// Review of #3501 at 5542f04 (MUST 5): with containment off, startup
    /// recovery signals nothing and keeps the record, as the cgroup sweep
    /// does under the same opt-out.
    #[tokio::test]
    async fn containment_off_skips_startup_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let mut member = spawn_member(a.pgid);
        let record = record_for(&a, dead_pid());
        let path = write_record(dir.path(), &record).unwrap();
        orphan(&mut anchor);

        let hold = recover_in(dir.path().to_path_buf(), true, false).await;

        assert!(hold.is_empty());
        std::thread::sleep(Duration::from_millis(100));
        let alive = running(&mut member) && running(&mut anchor);
        reap(&mut member);
        reap_anchor(&mut anchor);
        assert!(alive, "recovery signalled a group with containment off");
        assert!(path.exists(), "the record must be kept");
    }

    #[test]
    fn a_lingering_anchor_survives_every_signal_but_sigkill() {
        // Orphaned, the anchor is reaped by its new parent as soon as it
        // dies, so only SIGKILL may end it: anything else would free its pid
        // and group id between recovery's proof and its signal.
        let (mut anchor, _) = spawn_anchor();
        orphan(&mut anchor);
        use nix::sys::signal::Signal;
        let pid = anchor.id() as i32;
        for sig in [
            Signal::SIGHUP,
            Signal::SIGINT,
            Signal::SIGQUIT,
            Signal::SIGTRAP,
            Signal::SIGABRT,
            Signal::SIGBUS,
            Signal::SIGFPE,
            Signal::SIGILL,
            Signal::SIGSEGV,
            Signal::SIGSYS,
            Signal::SIGPIPE,
            Signal::SIGALRM,
            Signal::SIGTERM,
            Signal::SIGXCPU,
            Signal::SIGXFSZ,
            Signal::SIGVTALRM,
            Signal::SIGPROF,
            Signal::SIGUSR1,
            Signal::SIGUSR2,
        ] {
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), sig).unwrap();
            std::thread::sleep(Duration::from_millis(30));
            assert!(running(&mut anchor), "{sig} ended the anchor");
        }
        reap_anchor(&mut anchor);
    }

    #[test]
    fn a_lingering_anchor_does_not_rely_on_its_sleep_keeping_ignored_signals() {
        // Some `sleep` builds reset signal handlers when they start, so a
        // `sleep` cannot be the anchor itself. This one restores the default
        // action of every signal, then sleeps.
        let bin = tempfile::tempdir().unwrap();
        let fake = bin.path().join("sleep");
        std::fs::write(
            &fake,
            "#!/usr/bin/env python3\n\
             import signal, time\n\
             for s in signal.valid_signals():\n\
             \x20   try: signal.signal(s, signal.SIG_DFL)\n\
             \x20   except (OSError, ValueError): pass\n\
             time.sleep(1e6)\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!(
            "{}:{}",
            bin.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let (mut anchor, _) = spawn_anchor_with_path(&path);
        orphan(&mut anchor);
        let pid = nix::unistd::Pid::from_raw(anchor.id() as i32);
        for sig in [
            nix::sys::signal::Signal::SIGHUP,
            nix::sys::signal::Signal::SIGTERM,
            nix::sys::signal::Signal::SIGSEGV,
        ] {
            nix::sys::signal::kill(pid, sig).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            assert!(running(&mut anchor), "{sig} ended the anchor");
        }
        reap_anchor(&mut anchor);
    }

    #[tokio::test]
    async fn an_orphaned_anchor_alone_is_stopped_too() {
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let path = write_record(dir.path(), &record_for(&a, dead_pid())).unwrap();
        orphan(&mut anchor);

        let done = outcomes(recover_dir(dir.path().to_path_buf(), GRACE).await);

        assert_eq!(
            done,
            vec![(path.clone(), Recovery::Stopped(CleanupOutcome::Clean))]
        );
        let status = anchor.wait().unwrap();
        assert!(!status.success(), "the lingering anchor must be killed");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn a_group_whose_daemon_is_alive_is_left_alone() {
        // A second daemon on the same data dir: the anchor's parent is the
        // recorded owner, which is still alive (this test process).
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let mut member = spawn_member(a.pgid);
        let path = write_record(dir.path(), &record_for(&a, std::process::id() as i32)).unwrap();

        let done = outcomes(recover_dir(dir.path().to_path_buf(), GRACE).await);

        assert_eq!(done, vec![(path.clone(), Recovery::LeftToOwner)]);
        assert!(running(&mut member) && running(&mut anchor));
        assert!(path.exists(), "a live owner's record is kept");
        reap(&mut member);
        reap_anchor(&mut anchor);
    }

    #[tokio::test]
    async fn a_record_whose_anchor_was_replaced_is_never_signalled() {
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let mut member = spawn_member(a.pgid);
        let mut record = record_for(&a, dead_pid());
        record.anchor_start += 1; // the pid now belongs to another process
        let path = write_record(dir.path(), &record).unwrap();
        orphan(&mut anchor);

        let done = outcomes(recover_dir(dir.path().to_path_buf(), GRACE).await);

        assert_eq!(done, vec![(path.clone(), Recovery::LeftRunning)]);
        assert!(running(&mut member) && running(&mut anchor));
        assert!(!path.exists(), "an unprovable record is dropped");
        reap(&mut member);
        reap_anchor(&mut anchor);
    }

    /// Review of #3502 at 968b972 (MUST 7): a record whose anchor (or
    /// survivor) cannot be inspected is not proof the group is gone. The
    /// record and its runner's reservation are kept, nothing is signalled.
    #[test]
    fn a_failed_inspection_keeps_the_record_and_the_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let record = record_for(&a, dead_pid());
        let path = write_record(dir.path(), &record).unwrap();
        let failing = |_: i32| Err(std::io::Error::other("sysctl failed"));

        let anchored = recover_record_with(&path, &record, GRACE, failing);
        let kept = path.exists();
        let survivors = GroupRecord {
            survivors: vec![(a.pid, a.start)],
            ..record.clone()
        };
        let judged = judge_read(&survivors, failing);

        let alive = running(&mut anchor);
        reap_anchor(&mut anchor);
        assert!(
            anchored.is_unresolved(),
            "a failed read released the runner: {anchored:?}"
        );
        assert!(kept, "a failed read deleted the group's record");
        assert!(alive, "a group that could not be judged was signalled");
        assert!(matches!(judged, Verdict::Unknown(_)), "{judged:?}");
    }

    #[test]
    fn ownership_is_re_proven_before_sigterm() {
        // Review item 5 on #2952: the record was judged orphaned, then the
        // anchor went away before the first signal. The group is still
        // held by the member, but nothing proves it is the run's any more.
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let mut member = spawn_member(a.pgid);
        let record = record_for(&a, dead_pid());
        let path = write_record(dir.path(), &record).unwrap();
        orphan(&mut anchor);
        assert_eq!(
            judge(&record, procinfo::info_with_parent),
            Verdict::Orphaned
        );
        // Only the anchor goes; its lingering `sleep` stays in the group.
        reap(&mut anchor);

        let outcome = stop_adopted(&path, &record, GRACE);

        assert_eq!(outcome, CleanupOutcome::Unresolved);
        assert!(
            running(&mut member),
            "an unproven group must not be signalled"
        );
        reap(&mut member);
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(a.pgid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }

    #[test]
    fn a_spawned_group_is_recorded_until_its_anchor_is_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner = GroupOwner::new_anchored().expect("anchored group");
        let run_id = Uuid::new_v4();
        record_in(dir.path(), run_id, None, &mut owner);
        let path = record_path(dir.path(), run_id);
        let record: GroupRecord = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let anchor = owner.anchor_identity().unwrap();
        assert_eq!(
            record,
            GroupRecord {
                run_id,
                pgid: owner.pgid(),
                anchor_pid: anchor.pid,
                anchor_start: anchor.start,
                owner_pid: std::process::id() as i32,
                runner_id: None,
                survivors: Vec::new(),
            }
        );
        // A cleanup that confirms the group empty retires the record.
        let owner = std::sync::Mutex::new(owner);
        let (_, outcome) = crate::util::containment::stop_owned_group(run_id, &owner, GRACE);
        assert_eq!(outcome, CleanupOutcome::Clean);
        drop(owner);
        assert!(
            !path.exists(),
            "the record goes with a confirmed-empty group"
        );
    }

    /// Review of #3501 at ba9c1c3 (MUST 2/5): an owner dropped while its
    /// cleanup is unresolved (daemon shutdown during a held cleanup, or a
    /// cancelled detached cleanup) must keep the record and leave its
    /// anchor unreaped, so the group id stays reserved and the next
    /// daemon's recovery can still judge the group.
    #[test]
    fn an_unresolved_owner_dropped_at_shutdown_keeps_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner = GroupOwner::new_anchored().expect("anchored group");
        let run_id = Uuid::new_v4();
        record_in(dir.path(), run_id, None, &mut owner);
        let path = record_path(dir.path(), run_id);
        let anchor = owner.anchor_identity().unwrap();

        drop(owner);

        let still = procinfo::info(anchor.pid);
        // Reap or end the anchor this test left behind before asserting.
        let pid = nix::unistd::Pid::from_raw(anchor.pid);
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
        let _ = nix::sys::wait::waitpid(pid, None);
        assert!(path.exists(), "the only record of a live group was deleted");
        let record: GroupRecord = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            (record.anchor_pid, record.anchor_start),
            (anchor.pid, anchor.start)
        );
        assert!(
            still.is_some_and(|p| ProcId::from(&p) == anchor),
            "the anchor was reaped, freeing the group id"
        );
    }

    /// Review of #3502 at bf9a2ba (MUST 2): a live daemon's forced cleanup
    /// SIGKILLs the whole group, anchor included. If a member is still
    /// alive afterwards, the record must name it: otherwise, once the
    /// daemon is gone and the dead anchor is reaped, the next daemon finds
    /// the anchor gone, discards the record and admits work while the
    /// member still runs.
    #[test]
    fn a_forced_cleanup_that_ends_the_anchor_records_its_survivors() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner = GroupOwner::new_anchored().expect("anchored group");
        let run_id = Uuid::new_v4();
        record_in(dir.path(), run_id, None, &mut owner);
        let path = record_path(dir.path(), run_id);
        let anchor = owner.anchor_identity().unwrap();
        let mut member = spawn_member(owner.pgid());
        let live = match owner.observe() {
            crate::util::group_ownership::Proof::Ours(live) => live,
            other => panic!("the run's group was not proven: {other:?}"),
        };
        // The group's SIGKILL ends the anchor; the member is what survived.
        let pid = nix::unistd::Pid::from_raw(anchor.pid);
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
        while !procinfo::info(anchor.pid).is_some_and(|p| p.zombie) {
            std::thread::sleep(Duration::from_millis(10));
        }

        keep_survivors(&owner, &live);
        // The daemon goes away: its owner is dropped unresolved and the
        // dead anchor is reaped, as init would after a crash.
        drop(owner);
        let _ = nix::sys::wait::waitpid(pid, None);

        let record: GroupRecord = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let verdict = judge_read(&record, procinfo::read_with_parent);
        reap(&mut member);
        assert_eq!(
            verdict,
            Verdict::Survivors,
            "the next daemon could no longer prove the group its survivor holds"
        );
    }

    /// While the anchor is alive the record already proves the group;
    /// survivors would only stop recovery from signalling it.
    #[test]
    fn survivors_are_not_recorded_while_the_anchor_is_alive() {
        let dir = tempfile::tempdir().unwrap();
        let mut owner = GroupOwner::new_anchored().expect("anchored group");
        let run_id = Uuid::new_v4();
        record_in(dir.path(), run_id, None, &mut owner);
        let path = record_path(dir.path(), run_id);
        let before = std::fs::read(&path).unwrap();
        let mut member = spawn_member(owner.pgid());
        let live = match owner.observe() {
            crate::util::group_ownership::Proof::Ours(live) => live,
            other => panic!("the run's group was not proven: {other:?}"),
        };
        keep_survivors(&owner, &live);
        let after = std::fs::read(&path).unwrap();
        reap(&mut member);
        owner.confirm_empty();
        drop(owner);
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn recovery_is_skipped_while_another_daemon_recovers() {
        use fs2::FileExt;
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let path = write_record(dir.path(), &record_for(&a, dead_pid())).unwrap();
        orphan(&mut anchor);
        let held = std::fs::File::create(dir.path().join(RECOVERY_LOCK)).unwrap();
        held.lock_exclusive().unwrap();

        let done = recover_dir(dir.path().to_path_buf(), GRACE).await;

        assert!(done.is_none(), "a skipped pass must not look empty");
        assert!(running(&mut anchor) && path.exists());
        drop(held);
        assert_eq!(
            outcomes(recover_dir(dir.path().to_path_buf(), GRACE).await),
            vec![(path, Recovery::Stopped(CleanupOutcome::Clean))]
        );
        let _ = anchor.wait();
    }

    /// Review of #3501 at ba9c1c3 (MUST 1): a pass skipped because another
    /// daemon holds the recovery lock proves nothing, so it must neither
    /// admit work at start nor release runners already held.
    #[tokio::test]
    async fn a_skipped_recovery_pass_keeps_the_runners_reserved() {
        use fs2::FileExt;
        let dir = tempfile::tempdir().unwrap();
        let (mut anchor, a) = spawn_anchor();
        let runner = Uuid::new_v4();
        let mut record = record_for(&a, dead_pid());
        record.runner_id = Some(runner);
        let path = write_record(dir.path(), &record).unwrap();
        orphan(&mut anchor);
        let held = std::fs::File::create(dir.path().join(RECOVERY_LOCK)).unwrap();
        held.lock_exclusive().unwrap();

        let mut hold = recover_in(dir.path().to_path_buf(), true, true).await;
        let at_start = hold.affects(runner);
        hold.recheck().await;
        let after_recheck = hold.affects(runner);

        drop(held);
        hold.recheck().await;
        let after_release = hold.is_empty();
        let _ = anchor.wait();
        assert!(at_start, "a skipped startup pass admitted the runner");
        assert!(after_recheck, "a skipped recheck released the runner");
        assert!(
            after_release && !path.exists(),
            "the next pass must recover it"
        );
    }

    #[tokio::test]
    async fn an_unreadable_record_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("junk.json");
        std::fs::write(&path, b"{").unwrap();
        let done = outcomes(recover_dir(dir.path().to_path_buf(), GRACE).await);
        assert_eq!(done, vec![(path.clone(), Recovery::Discarded)]);
        assert!(!path.exists());
    }
}
