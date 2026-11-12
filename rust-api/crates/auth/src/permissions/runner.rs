#![forbid(unsafe_code)]

//! Runner permissions (`pi_dash/runner/services/permissions.py`, re-exporting
//! the workspace helpers from `pi_dash/core/permissions.py`).
//!
//! Visibility has a single value today (`Visibility.PRIVATE=0`,
//! `runner/models.py`): private rows are visible only to their owner, even
//! inside a shared workspace. Anything else denies by default, so a future
//! visibility value cannot silently open access.
//!
//! `filter_runs_usable_by_runner` is an ORM queryset transform; the kernel
//! carries its predicate as [`RunUsability`] so domain ports write the same
//! `EXISTS` clauses in SQL: a run qualifies when it was created by the
//! runner's owner, is owned by the owner, its issue involves the owner
//! (created by, or assigned through an `IssueAssignee` row whose
//! `deleted_at` is NULL — the M2M would ignore the soft-delete stamp and
//! keep granting rights to every user ever assigned), or its scheduler
//! binding was authored by the owner. Non-private runners match nothing
//! (`qs.none()`).

use pidash_types::{UserId, WorkspaceId};

use super::scope_allows;
use crate::scope::TenantScope;

/// `Visibility.PRIVATE` (`runner/models.py`).
pub const VISIBILITY_PRIVATE: i32 = 0;

/// Facts for one `(user, runner-or-dev-machine)` check: the row's
/// visibility, whether the user owns it, and the workspace both live in.
pub struct RunnerFacts {
    pub workspace: WorkspaceId,
    pub authenticated: bool,
    pub visibility: i32,
    pub owned_by_requester: bool,
}

/// `runner_visible_to_user_q` / `can_view_dev_machine` / `can_view_runner`:
/// all three reduce to "private and owned by the requester". Anonymous
/// matches nothing (the `pk__isnull` predicate).
pub fn can_view_runner(facts: &RunnerFacts) -> bool {
    if !facts.authenticated {
        return false;
    }
    facts.visibility == VISIBILITY_PRIVATE && facts.owned_by_requester
}

/// `can_use_dev_machine` / `can_use_runner`: aliases of the view check in
/// Python (`return can_view_...`).
pub fn can_use_runner(facts: &RunnerFacts) -> bool {
    can_view_runner(facts)
}

/// Which of the four `filter_runs_usable_by_runner` clauses a run
/// satisfies; each is one `EXISTS` (or direct column match) in the port's
/// SQL, evaluated for the runner's owner.
///
/// `is_engine` / `team_runs` / `automatic` select the built-in-engine
/// branch (PO-3, design `built_engine_as_runner` §8, #2903): a private
/// engine whose machine has `engine_team_runs = false` admits only runs
/// its owner created, plus automatic runs (tick/scheduler trigger, i.e.
/// `not human`) on the owner's issues or from the owner's scheduler
/// bindings. The `owned_by_owner` clause is dropped in that branch.
pub struct RunUsability {
    pub created_by_owner: bool,
    pub owned_by_owner: bool,
    pub issue_involves_owner: bool,
    pub scheduler_authored_by_owner: bool,
    pub is_engine: bool,
    /// `DevMachine.engine_team_runs` is on (own-agent runners pass true).
    pub team_runs: bool,
    /// The run is automatic: its trigger is not in `HUMAN_TRIGGERS`.
    pub automatic: bool,
}

/// Mirror of the annotated `.filter(Q(...) | Q(...) | ...)` for private
/// runners; non-private runners see nothing.
///
/// Built-in engine runners without `engine_team_runs` use the narrower
/// PO-3 rule: owner-created runs, plus automatic runs on owner issues or
/// from owner-authored bindings. Engines with `engine_team_runs` fall
/// back to the ordinary own-agent rule.
pub fn run_visible_to_runner(visibility: i32, usability: &RunUsability) -> bool {
    if visibility != VISIBILITY_PRIVATE {
        return false;
    }
    if usability.is_engine && !usability.team_runs {
        return usability.created_by_owner
            || (usability.automatic
                && (usability.issue_involves_owner || usability.scheduler_authored_by_owner));
    }
    usability.created_by_owner
        || usability.owned_by_owner
        || usability.issue_involves_owner
        || usability.scheduler_authored_by_owner
}

/// Python form of the usage rule (`dispatch_rules.runner_may_take`), for
/// preflight: may `runner` take a run with these facts?
///
/// A run its owner created is always takable. Otherwise the owner must be
/// involved (on the issue, or the binding's author). For an engine, an
/// involved run is takable only when it is automatic (`human == false`)
/// or `team_runs` is on. Note Django has no `owned_by_owner` clause here:
/// `owner` is written from the runner at assignment, so it never admits
/// an unassigned run.
pub fn runner_may_take(
    is_engine: bool,
    team_runs: bool,
    created_by_owner: bool,
    human: bool,
    issue_involves_owner: bool,
    scheduler_authored_by_owner: bool,
) -> bool {
    if created_by_owner {
        return true;
    }
    let involved = issue_involves_owner || scheduler_authored_by_owner;
    if !involved {
        return false;
    }
    if !is_engine {
        return true;
    }
    !human || team_runs
}

/// `engine_takes_pod_work` (`dispatch_rules.py`): an engine takes pins and
/// unpinned pod work only while it is a `pod_member` and its machine's
/// engine is not paused. `pod_member` is `pod_dispatch == 'pod_member'`;
/// `paused` is `Runner.engine_paused`. Callers map their data to these.
pub fn engine_takes_pod_work(pod_member: bool, paused: bool) -> bool {
    pod_member && !paused
}

/// `locality_pin_allowed` (`dispatch_rules.py`): may a continuation be
/// pinned to this runner for repo locality? Own-agent runners: always.
/// Engines: only while they take pod work. `runner_present` is false
/// when there is no runner.
pub fn locality_pin_allowed(
    runner_present: bool,
    is_engine: bool,
    pod_member: bool,
    paused: bool,
) -> bool {
    if !runner_present {
        return false;
    }
    if !is_engine {
        return true;
    }
    engine_takes_pod_work(pod_member, paused)
}

/// Facts for `can_manage_runner`: the view check plus ownership and the
/// workspace-admin rule. `requester` is the acting user's id; it is `None`
/// for anonymous callers (who already fail the view check).
pub struct ManageFacts {
    pub workspace: WorkspaceId,
    pub requester: Option<UserId>,
    pub visibility: i32,
    pub owned_by_requester: bool,
    /// Active workspace membership with role exactly `ADMIN`.
    pub is_workspace_admin: bool,
}

/// `can_manage_runner`: private runners are owner-managed only; any other
/// visibility allows the owner or a workspace admin. Shared by the web
/// session views and the `X-Api-Key` v1 endpoint.
pub fn can_manage_runner(scope: &TenantScope, facts: &ManageFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) {
        return false;
    }
    let Some(_) = facts.requester else {
        return false;
    };
    let view = RunnerFacts {
        workspace: facts.workspace.clone(),
        authenticated: true,
        visibility: facts.visibility,
        owned_by_requester: facts.owned_by_requester,
    };
    if !can_view_runner(&view) {
        return false;
    }
    if facts.visibility == VISIBILITY_PRIVATE {
        return facts.owned_by_requester;
    }
    facts.owned_by_requester || facts.is_workspace_admin
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(owner: bool, visibility: i32) -> RunnerFacts {
        RunnerFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            visibility,
            owned_by_requester: owner,
        }
    }

    fn usability() -> RunUsability {
        RunUsability {
            created_by_owner: false,
            owned_by_owner: false,
            issue_involves_owner: false,
            scheduler_authored_by_owner: false,
            is_engine: false,
            team_runs: true,
            automatic: true,
        }
    }

    #[test]
    fn private_visible_only_to_owner() {
        assert!(can_view_runner(&view(true, VISIBILITY_PRIVATE)));
        assert!(!can_view_runner(&view(false, VISIBILITY_PRIVATE)));
        let mut anon = view(true, VISIBILITY_PRIVATE);
        anon.authenticated = false;
        assert!(!can_view_runner(&anon));
    }

    #[test]
    fn unknown_visibility_denies_by_default() {
        assert!(!can_view_runner(&view(true, 99)));
        assert!(!can_view_runner(&view(false, 99)));
    }

    #[test]
    fn use_is_view() {
        assert!(can_use_runner(&view(true, VISIBILITY_PRIVATE)));
        assert!(!can_use_runner(&view(false, VISIBILITY_PRIVATE)));
    }

    #[test]
    fn run_filter_needs_one_clause() {
        let mut u = usability();
        assert!(!run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        u.issue_involves_owner = true;
        assert!(run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        u.issue_involves_owner = false;
        u.scheduler_authored_by_owner = true;
        assert!(run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        // Non-private runners match nothing, whatever the clauses say.
        u.created_by_owner = true;
        u.owned_by_owner = true;
        assert!(!run_visible_to_runner(99, &u));
    }

    #[test]
    fn engine_branch_drops_owner_clause() {
        // PO-3: private engine, team_runs off.
        let mut u = usability();
        u.is_engine = true;
        u.team_runs = false;
        u.owned_by_owner = true;
        // owner_id alone never admits for an engine.
        assert!(!run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        // Owner-created runs still admit.
        u.owned_by_owner = false;
        u.created_by_owner = true;
        assert!(run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        // Automatic runs on owner issues/bindings admit...
        u.created_by_owner = false;
        u.automatic = true;
        u.issue_involves_owner = true;
        assert!(run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        // ...but human-triggered ones do not.
        u.automatic = false;
        assert!(!run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        // team_runs on restores the ordinary rule (owner clause back).
        u.team_runs = true;
        assert!(run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        u.issue_involves_owner = false;
        u.owned_by_owner = true;
        assert!(run_visible_to_runner(VISIBILITY_PRIVATE, &u));
        // Non-private engine sees nothing.
        assert!(!run_visible_to_runner(99, &u));
    }

    #[test]
    fn may_take_matches_django() {
        // (is_engine, team, created, human, issue, sched, expected)
        let cases: &[(bool, bool, bool, bool, bool, bool, bool)] = &[
            // Created by owner: always, even human click on a stranger issue.
            (true, false, true, true, false, false, true),
            (true, false, true, false, false, false, true),
            (false, false, true, true, false, false, true),
            // No involvement: never.
            (true, false, false, false, false, false, false),
            (false, false, false, false, false, false, false),
            (true, true, false, false, false, false, false),
            // Own-agent involved: always.
            (false, false, false, true, true, false, true),
            (false, false, false, false, false, true, true),
            // Engine involved, automatic: yes; human click: team gate.
            (true, false, false, false, true, false, true),
            (true, false, false, false, false, true, true),
            (true, false, false, true, true, false, false),
            (true, false, false, true, false, true, false),
            (true, true, false, true, true, false, true),
            (true, true, false, true, false, true, true),
        ];
        for &(is_engine, team, created, human, issue, sched, expected) in cases {
            assert_eq!(
                runner_may_take(is_engine, team, created, human, issue, sched),
                expected,
                "is_engine={is_engine} team={team} created={created} \
                 human={human} issue={issue} sched={sched}"
            );
        }
    }

    #[test]
    fn locality_tables() {
        // engine_takes_pod_work: pod_member AND not paused.
        assert!(engine_takes_pod_work(true, false));
        assert!(!engine_takes_pod_work(true, true));
        assert!(!engine_takes_pod_work(false, false));
        assert!(!engine_takes_pod_work(false, true));
        // locality_pin_allowed: none -> false; own-agent -> true;
        // engine -> pod-member + unpaused.
        assert!(!locality_pin_allowed(false, false, true, false));
        assert!(!locality_pin_allowed(false, true, true, false));
        assert!(locality_pin_allowed(true, false, true, true));
        assert!(locality_pin_allowed(true, false, false, false));
        assert!(locality_pin_allowed(true, true, true, false));
        assert!(!locality_pin_allowed(true, true, true, true));
        assert!(!locality_pin_allowed(true, true, false, false));
        assert!(!locality_pin_allowed(true, true, false, true));
    }

    #[test]
    fn manage_private_is_owner_only() {
        let scope = TenantScope::new(WorkspaceId::from("acme"));
        let owner = ManageFacts {
            workspace: WorkspaceId::from("acme"),
            requester: Some(UserId::from("u-1")),
            visibility: VISIBILITY_PRIVATE,
            owned_by_requester: true,
            is_workspace_admin: false,
        };
        assert!(can_manage_runner(&scope, &owner));
        let admin = ManageFacts {
            workspace: WorkspaceId::from("acme"),
            requester: Some(UserId::from("u-2")),
            visibility: VISIBILITY_PRIVATE,
            owned_by_requester: false,
            is_workspace_admin: true,
        };
        // A workspace admin who does not own the private runner cannot
        // even view it, so management denies.
        assert!(!can_manage_runner(&scope, &admin));
    }

    #[test]
    fn manage_other_visibility_owner_or_admin() {
        let scope = TenantScope::new(WorkspaceId::from("acme"));
        // (No such visibility exists today; the branch is unreachable until
        // one is added, and view-denial keeps it closed meanwhile.)
        let stranger = ManageFacts {
            workspace: WorkspaceId::from("acme"),
            requester: Some(UserId::from("u-3")),
            visibility: 99,
            owned_by_requester: false,
            is_workspace_admin: true,
        };
        assert!(!can_manage_runner(&scope, &stranger));
        let anon = ManageFacts {
            workspace: WorkspaceId::from("acme"),
            requester: None,
            visibility: VISIBILITY_PRIVATE,
            owned_by_requester: false,
            is_workspace_admin: false,
        };
        assert!(!can_manage_runner(&scope, &anon));
        let other = TenantScope::new(WorkspaceId::from("other"));
        let owner = ManageFacts {
            workspace: WorkspaceId::from("acme"),
            requester: Some(UserId::from("u-1")),
            visibility: VISIBILITY_PRIVATE,
            owned_by_requester: true,
            is_workspace_admin: false,
        };
        assert!(!can_manage_runner(&other, &owner));
    }
}
