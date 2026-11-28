//! Ownership-proven process-group signalling (PDASHOSS01-417, part 1).
//!
//! The process-group backend of [`crate::util::containment`] gives a run's
//! agent its own process group and, at run end, signals that group. A
//! process-group id is only a number: once every member of the run's group
//! has exited, the kernel may hand the same number to an unrelated
//! process's new group. Signalling a saved pgid without proof can then hit
//! a stranger. The rule here is the one the issue sets: **when ownership of
//! a group cannot be proven, never signal it** — leaving a process running
//! is preferred over killing the wrong one.
//!
//! Proof. A process is identified by its pid *and* its exact kernel start
//! time ([`ProcId`]); a reused pid always has a different start time. The
//! owner remembers the group leader as it was right after spawn, plus every
//! member seen in a proven snapshot. A fresh snapshot of the group is
//! proven when it contains at least one remembered process: a group cannot
//! be recycled while any of its members is alive (or an unreaped zombie),
//! so that member being present means the group never stopped being ours,
//! and every other member in the snapshot belongs to the run too. Proven
//! snapshots extend the remembered set, which is how proof survives the
//! leader's exit; [`GroupOwner::observe`] is called periodically while the
//! run is live for exactly that reason.
//!
//! Anchor (reviews of #3497). Before the run's agent is spawned, the daemon
//! spawns a tiny placeholder process of its own as the leader of a new
//! group ([`Anchor`], [`GroupOwner::new_anchored`]) and does not reap it
//! until the claim is dropped, after cleanup. Every agent of the run is
//! spawned *into* that group, so the claim never depends on reading an
//! agent's pid after spawn, which the bridge may already have reaped. An
//! unreaped child keeps its group id reserved, so from before the first
//! spawn to the end of cleanup the id cannot be handed to anyone else, and
//! every live member of the group is the run's. The anchor ignores SIGTERM
//! (it must outlast the grace period) and dies with the group's SIGKILL;
//! it is never counted or reported as a run process. A group with no
//! anchor is never signalled: nothing pins its id between the last proof
//! and `killpg`.
//!
//! Scans are not atomic (Linux reads one `/proc/<pid>/stat` at a time), so
//! a proven scan is accepted only if a process that anchored it is re-read
//! *after* the scan with the same identity and group: that process was in
//! the group for the whole scan, so the group could not have been recycled
//! in between. An inspection failure is [`Proof::Unknown`], never "empty".
//!
//! Serialisation (review item 1 on #2952). Proof is never cached: every
//! signal takes a fresh kernel snapshot and sends `killpg` straight after
//! it, under the owner's lock, so no reaper-updated flag can claim the
//! leader is alive after it was reaped. The anchor closes the window
//! between that snapshot and `killpg`: the group id cannot be recycled
//! while the daemon's unreaped anchor holds it.
//!
//! Start times are compared exactly on every platform (review item 4):
//! there is no tolerance for a replacement started "close enough".

use std::collections::HashSet;

use crate::util::procinfo::{self, ProcInfo};

/// A process identity that survives pid reuse: pid plus exact start time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcId {
    pub pid: i32,
    pub start: u64,
}

impl From<&ProcInfo> for ProcId {
    fn from(p: &ProcInfo) -> Self {
        Self {
            pid: p.pid,
            start: p.start,
        }
    }
}

/// What a snapshot of the group says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proof {
    /// No live member: nothing to stop (zombies cannot run again).
    Empty,
    /// The group is still the run's; its live members.
    Ours(Vec<ProcInfo>),
    /// Live members, none of them remembered: the group cannot be proven
    /// to be the run's and must not be signalled.
    Unproven(Vec<ProcInfo>),
    /// The group could not be inspected at all: neither empty nor proven.
    /// Nothing is signalled and nothing is learned.
    Unknown,
}

/// Signals [`GroupOwner::signal`] sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupSignal {
    Term,
    Kill,
}

/// The run's claim on one process group.
#[derive(Debug)]
pub struct GroupOwner {
    pgid: i32,
    known: HashSet<ProcId>,
    anchor: Option<Anchor>,
}

/// A placeholder child of the daemon inside the run's group, kept unreaped
/// so the group id stays reserved until the claim is dropped (see the
/// module doc). Dropping it SIGKILLs and reaps it: it is the daemon's own
/// unreaped child, so its pid cannot belong to anyone else.
#[derive(Debug)]
struct Anchor {
    id: ProcId,
    child: Option<std::process::Child>,
}

impl Anchor {
    /// Spawn the placeholder as the leader of a new process group. It
    /// ignores SIGTERM, SIGINT and SIGHUP and exits when the daemon closes
    /// its stdin (including when the daemon dies), so it never outlives the
    /// daemon.
    #[cfg(unix)]
    fn spawn_leader() -> Option<Self> {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "trap '' TERM INT HUP; read _"])
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let joined = i32::try_from(child.id())
            .ok()
            .and_then(procinfo::info)
            .filter(|p| p.pgid == p.pid && !p.zombie);
        match joined {
            Some(p) => Some(Self {
                id: ProcId::from(&p),
                child: Some(child),
            }),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                None
            }
        }
    }

    #[cfg(not(unix))]
    fn spawn_leader() -> Option<Self> {
        None
    }
}

impl Drop for Anchor {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl GroupOwner {
    /// Claim the group led by `leader`, right after spawning it into a
    /// group of its own (`process_group(0)`, so pgid == pid). `None` when
    /// the leader is already unreadable or is not a group leader: then
    /// nothing can ever be proven and the group is never signalled.
    pub fn claim(leader: &ProcInfo) -> Option<Self> {
        if leader.pid <= 0 || leader.pgid != leader.pid {
            return None;
        }
        Some(Self {
            pgid: leader.pgid,
            known: HashSet::from([ProcId::from(leader)]),
            anchor: None,
        })
    }

    /// Create the run's group, led by a fresh [`Anchor`], before any agent
    /// is spawned; agents then join it with `process_group(pgid)`. `None`
    /// when the anchor cannot be spawned or read: the run then has no
    /// pinned group, and none of its processes is ever signalled.
    pub fn new_anchored() -> Option<Self> {
        let anchor = Anchor::spawn_leader()?;
        Some(Self {
            pgid: anchor.id.pid,
            known: HashSet::new(),
            anchor: Some(anchor),
        })
    }

    /// Whether an [`Anchor`] keeps the group id reserved.
    pub fn is_anchored(&self) -> bool {
        self.anchor.is_some()
    }

    /// Pid of the anchor, if any (tests only).
    #[cfg(test)]
    pub fn anchor_pid(&self) -> Option<i32> {
        self.anchor.as_ref().map(|a| a.id.pid)
    }

    /// Treat `id` as the group's anchor without spawning one (tests only).
    #[cfg(test)]
    fn with_anchor_id(mut self, id: ProcId) -> Self {
        self.anchor = Some(Anchor { id, child: None });
        self
    }

    fn anchor_id(&self) -> Option<ProcId> {
        self.anchor.as_ref().map(|a| a.id)
    }

    /// Whether `id` was known to be the run's before the current snapshot.
    fn remembers(&self, id: &ProcId) -> bool {
        self.known.contains(id) || self.anchor_id() == Some(*id)
    }

    pub fn pgid(&self) -> i32 {
        self.pgid
    }

    /// Whether some remembered process has this pid (tests only).
    #[cfg(test)]
    pub fn knows_pid(&self, pid: i32) -> bool {
        self.known.iter().any(|k| k.pid == pid)
    }

    /// Judge a snapshot without touching the remembered set.
    pub fn judge(&self, snapshot: Vec<ProcInfo>) -> Proof {
        let members: Vec<ProcInfo> = snapshot
            .into_iter()
            .filter(|p| p.pgid == self.pgid)
            .collect();
        let anchored = members.iter().any(|p| self.remembers(&ProcId::from(p)));
        let anchor = self.anchor_id();
        let live: Vec<ProcInfo> = members
            .iter()
            .filter(|p| !p.zombie && Some(ProcId::from(*p)) != anchor)
            .cloned()
            .collect();
        if live.is_empty() {
            Proof::Empty
        } else if anchored {
            Proof::Ours(live)
        } else {
            Proof::Unproven(live)
        }
    }

    /// Judge a snapshot and, when it is proven, remember its members so
    /// proof survives the exit of the processes remembered so far.
    pub fn accept(&mut self, snapshot: Vec<ProcInfo>) -> Proof {
        let proof = self.judge(snapshot);
        if let Proof::Ours(live) = &proof {
            self.known.extend(live.iter().map(ProcId::from));
        }
        proof
    }

    /// Take a fresh kernel snapshot and [`accept`](Self::accept) it.
    pub fn observe(&mut self) -> Proof {
        self.observe_with(procinfo::group_members, procinfo::info)
    }

    /// [`observe`](Self::observe) with the kernel reads injected: `scan`
    /// lists the group, `reread` inspects one process after the scan.
    pub fn observe_with(
        &mut self,
        scan: impl FnOnce(i32) -> std::io::Result<Vec<ProcInfo>>,
        reread: impl Fn(i32) -> Option<ProcInfo>,
    ) -> Proof {
        let Ok(snapshot) = scan(self.pgid) else {
            return Proof::Unknown;
        };
        let anchors: Vec<ProcId> = snapshot
            .iter()
            .filter(|p| p.pgid == self.pgid)
            .map(ProcId::from)
            .filter(|id| self.remembers(id))
            .collect();
        match self.judge(snapshot) {
            Proof::Ours(live) => {
                let pgid = self.pgid;
                let confirmed = anchors.iter().any(|a| {
                    reread(a.pid).is_some_and(|r| r.pgid == pgid && ProcId::from(&r) == *a)
                });
                if !confirmed {
                    return Proof::Unproven(live);
                }
                self.known.extend(live.iter().map(ProcId::from));
                Proof::Ours(live)
            }
            other => other,
        }
    }

    /// Signal the group only if a fresh snapshot proves it is still ours
    /// and an anchor pins its id through delivery. Returns the proof the
    /// decision was made on; a proven but unpinned group is reported as
    /// [`Proof::Unproven`] and left alone.
    pub fn signal(&mut self, sig: GroupSignal) -> Proof {
        match self.observe() {
            Proof::Ours(live) if self.anchor.is_none() => Proof::Unproven(live),
            Proof::Ours(live) => {
                send_killpg(self.pgid, sig);
                Proof::Ours(live)
            }
            other => other,
        }
    }
}

#[cfg(unix)]
fn send_killpg(pgid: i32, sig: GroupSignal) {
    if pgid <= 1 {
        return;
    }
    let sig = match sig {
        GroupSignal::Term => nix::sys::signal::Signal::SIGTERM,
        GroupSignal::Kill => nix::sys::signal::Signal::SIGKILL,
    };
    let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid), sig);
}

#[cfg(not(unix))]
fn send_killpg(_pgid: i32, _sig: GroupSignal) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: i32, pgid: i32, start: u64) -> ProcInfo {
        ProcInfo {
            pid,
            pgid,
            start,
            zombie: false,
            name: format!("p{pid}"),
        }
    }

    fn zombie(pid: i32, pgid: i32, start: u64) -> ProcInfo {
        ProcInfo {
            zombie: true,
            ..p(pid, pgid, start)
        }
    }

    #[test]
    fn only_a_group_leader_can_be_claimed() {
        assert!(GroupOwner::claim(&p(10, 10, 1)).is_some());
        assert!(GroupOwner::claim(&p(10, 7, 1)).is_none());
        assert!(GroupOwner::claim(&p(0, 0, 1)).is_none());
    }

    #[test]
    fn a_live_leader_with_its_exact_start_proves_the_group() {
        let owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        let proof = owner.judge(vec![p(10, 10, 500), p(11, 10, 600)]);
        assert_eq!(proof, Proof::Ours(vec![p(10, 10, 500), p(11, 10, 600)]));
    }

    #[test]
    fn a_reused_pgid_is_never_proven() {
        // Review item 1: the leader was reaped, the group emptied and the
        // kernel handed the id to an unrelated group. Its leader has the
        // same pid but another start time.
        let owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        let proof = owner.judge(vec![p(10, 10, 9_000), p(12, 10, 9_100)]);
        assert!(matches!(proof, Proof::Unproven(_)), "{proof:?}");
    }

    #[test]
    fn start_times_must_match_exactly() {
        // Review item 4: no "within two seconds" allowance. A replacement
        // started one unit (1 µs on macOS, one tick on Linux) later is a
        // different process.
        let owner = GroupOwner::claim(&p(10, 10, 1_000_000)).unwrap();
        for start in [999_999, 1_000_001, 1_000_000 + 2_000_000] {
            let proof = owner.judge(vec![p(10, 10, start)]);
            assert!(matches!(proof, Proof::Unproven(_)), "start {start}");
        }
    }

    #[test]
    fn proof_survives_the_leader_through_remembered_members() {
        let mut owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        // While the leader lives, its child is learned.
        assert!(matches!(
            owner.accept(vec![p(10, 10, 500), p(11, 10, 600)]),
            Proof::Ours(_)
        ));
        // Leader gone; the remembered child still anchors the group, and a
        // grandchild seen next to it is learned too.
        assert!(matches!(
            owner.accept(vec![p(11, 10, 600), p(13, 10, 700)]),
            Proof::Ours(_)
        ));
        assert!(matches!(owner.accept(vec![p(13, 10, 700)]), Proof::Ours(_)));
    }

    #[test]
    fn an_unproven_snapshot_teaches_nothing() {
        let mut owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        assert!(matches!(
            owner.accept(vec![p(12, 10, 9_100)]),
            Proof::Unproven(_)
        ));
        // 12 was not learned, so it cannot anchor a later snapshot.
        assert!(matches!(
            owner.accept(vec![p(12, 10, 9_100)]),
            Proof::Unproven(_)
        ));
    }

    #[test]
    fn zombies_anchor_but_need_no_signal() {
        let owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        // An unreaped leader keeps the id reserved: its live child is ours.
        assert_eq!(
            owner.judge(vec![zombie(10, 10, 500), p(11, 10, 600)]),
            Proof::Ours(vec![p(11, 10, 600)])
        );
        assert_eq!(owner.judge(vec![zombie(10, 10, 500)]), Proof::Empty);
        assert_eq!(owner.judge(Vec::new()), Proof::Empty);
    }

    #[test]
    fn a_group_recycled_during_the_scan_is_not_proven() {
        // Review item (#3497): the Linux scan reads one /proc entry at a
        // time. The remembered leader was read early; then the group
        // emptied and its id went to a stranger whose member was read
        // later. Re-reading the anchor after the scan exposes it.
        let mut owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        let proof = owner.observe_with(|_| Ok(vec![p(10, 10, 500), p(12, 10, 9_100)]), |_pid| None);
        assert!(matches!(proof, Proof::Unproven(_)), "{proof:?}");
        assert!(!owner.knows_pid(12), "a stranger was learned");
        // A re-read that still finds the anchor proves the scan.
        let proof = owner.observe_with(
            |_| Ok(vec![p(10, 10, 500), p(11, 10, 600)]),
            |pid| (pid == 10).then(|| p(10, 10, 500)),
        );
        assert!(matches!(proof, Proof::Ours(_)), "{proof:?}");
        assert!(owner.knows_pid(11));
    }

    #[test]
    fn a_re_read_with_another_start_or_group_does_not_revalidate() {
        let mut owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        let proof = owner.observe_with(|_| Ok(vec![p(10, 10, 500)]), |_| Some(p(10, 10, 501)));
        assert!(matches!(proof, Proof::Unproven(_)), "{proof:?}");
        let proof = owner.observe_with(|_| Ok(vec![p(10, 10, 500)]), |_| Some(p(10, 77, 500)));
        assert!(matches!(proof, Proof::Unproven(_)), "{proof:?}");
    }

    #[test]
    fn an_anchor_proves_the_group_after_the_leader_is_gone() {
        // Review item (#3497): the leader was reaped before its child was
        // ever observed; the anchor still holds the group id.
        let anchor = ProcId {
            pid: 20,
            start: 700,
        };
        let mut owner = GroupOwner::claim(&p(10, 10, 500))
            .unwrap()
            .with_anchor_id(anchor);
        let proof = owner.observe_with(
            |_| Ok(vec![p(20, 10, 700), p(11, 10, 600)]),
            |pid| (pid == 20).then(|| p(20, 10, 700)),
        );
        // The anchor proves the group but is never one of its processes.
        assert_eq!(proof, Proof::Ours(vec![p(11, 10, 600)]));
        assert_eq!(owner.judge(vec![p(20, 10, 700)]), Proof::Empty);
        assert_eq!(
            owner.judge(vec![zombie(20, 10, 700), p(11, 10, 600)]),
            Proof::Ours(vec![p(11, 10, 600)])
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_new_group_is_led_by_a_real_anchor_that_is_reaped_on_drop() {
        let owner = GroupOwner::new_anchored().expect("anchored group");
        let anchor = owner.anchor_pid().expect("anchored");
        let a = procinfo::info(anchor).expect("anchor alive");
        assert_eq!((a.pid, a.pgid), (owner.pgid(), owner.pgid()));
        drop(owner);
        assert!(
            procinfo::info(anchor).is_none(),
            "anchor must be killed and reaped with the claim"
        );
    }

    #[test]
    fn a_failed_scan_is_unknown_and_teaches_nothing() {
        // Review item (#3497): an inspection failure is not an empty group.
        let mut owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        let proof = owner.observe_with(
            |_| Err(std::io::Error::from(std::io::ErrorKind::OutOfMemory)),
            |_| Some(p(10, 10, 500)),
        );
        assert_eq!(proof, Proof::Unknown);
    }

    #[test]
    fn members_of_other_groups_are_ignored() {
        let owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        assert_eq!(owner.judge(vec![p(10, 99, 500)]), Proof::Empty);
    }
}
