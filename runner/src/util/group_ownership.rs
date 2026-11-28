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
//! Serialisation (review item 1 on #2952). Proof is never cached: every
//! signal takes a fresh kernel snapshot and sends `killpg` straight after
//! it, under the owner's lock, so no reaper-updated flag can claim the
//! leader is alive after it was reaped. The only remaining window is the
//! few microseconds between that snapshot and the `killpg` call, during
//! which the whole group would have to exit and a new process would have
//! to be allocated the very same id.
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
        })
    }

    /// Read the leader from the kernel and [`claim`](Self::claim) it.
    pub fn claim_pid(pid: i32) -> Option<Self> {
        Self::claim(&procinfo::info(pid)?)
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
        let anchored = members
            .iter()
            .any(|p| self.known.contains(&ProcId::from(p)));
        let live: Vec<ProcInfo> = members.iter().filter(|p| !p.zombie).cloned().collect();
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
        let snapshot = procinfo::group_members(self.pgid);
        self.accept(snapshot)
    }

    /// Signal the group only if a fresh snapshot proves it is still ours.
    /// Returns the proof the decision was made on.
    pub fn signal(&mut self, sig: GroupSignal) -> Proof {
        let proof = self.observe();
        if matches!(proof, Proof::Ours(_)) {
            send_killpg(self.pgid, sig);
        }
        proof
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
    fn members_of_other_groups_are_ignored() {
        let owner = GroupOwner::claim(&p(10, 10, 500)).unwrap();
        assert_eq!(owner.judge(vec![p(10, 99, 500)]), Proof::Empty);
    }
}
