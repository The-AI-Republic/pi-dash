//! D-27 permission gates (stage 5, PIDASHCONV-290).
//!
//! Ports the `@allow_permission` gates on all 14 D-27 cycle routes (26
//! method+path rows: 19 decorated actions plus the 7 undecorated
//! fallthroughs that reach `BaseViewSet`/`ModelViewSet` defaults), from:
//!
//! - `apps/api/pi_dash/app/views/cycle/base.py` (13 gated units)
//! - `apps/api/pi_dash/app/views/cycle/archive.py` (3 gates)
//! - `apps/api/pi_dash/app/views/cycle/issue.py` (3 gates)
//!
//! Fixture id: F-C27-07 (`rust-api/fixtures/app_cycles/perms.json` +
//! `TRACE.md`).
//!
//! Shape of the port: the module is pure, like the
//! [`pidash_auth::permissions::allow`] kernel it sits on. Membership facts
//! are caller inputs ([`AllowFacts`]); row fetching (the
//! `WorkspaceMember`/`ProjectMember ...exists()` queries with their
//! `workspace__slug=` / `project_id=` / `is_active=True` filters) stays
//! with the handlers, which fetch through the workspace-scoped handle —
//! the `tenant_context` half of the pilot pattern. This module only maps
//! each route to its [`Gate`] and decides through the kernel, so the
//! PROJECT-level workspace-admin override (`app/permissions/base.py`:
//! project member + workspace role exactly ADMIN passes regardless of
//! project role) and the deny-by-default tenant scope come along
//! unchanged.
//!
//! Gate order (preserved, not redesigned):
//!
//! - Decorated routes: Django-session authentication (`BaseViewSet` /
//!   `BaseAPIView`: `BaseSessionAuthentication` + `IsAuthenticated`,
//!   `app/views/base.py:84-194`) runs before the decorator, and the
//!   decorator runs before the handler body. Anonymous callers never
//!   reach a gate ([`GateOutcome::Unauthenticated`]).
//! - Undecorated fallthroughs ([`Gate::Authenticated`]): PUT
//!   `cycles/<uuid>/` reaches `UpdateModelMixin.update` with
//!   `CycleSerializer` (all fields read-only: body ignored, 200 with the
//!   current representation — fixture `undecorated_put_update`); issue
//!   retrieve/update and the favorites GET list reach the `ModelViewSet`
//!   defaults. No role check on these paths beyond `IsAuthenticated`.
//!   The handler issues own the body behavior; this table pins the gate
//!   half (any authenticated caller passes).
//! - `WorkspaceViewerPermission` is explicitly OUT of scope: it guards
//!   `app/views/workspace/cycle.py`, which is D-24's route.
//!
//! Denial bodies (byte-exact, compact DRF rendering):
//!
//! - Decorator denial: `{"error":"You don't have the required permissions."}`
//!   (403, `app/permissions/base.py`); see [`FORBIDDEN_BODY`].
//! - Anonymous: `{"detail":"Authentication credentials were not provided."}`
//!   (401); see [`ANON_BODY`]. Every D-27 route carries the
//!   `IsAuthenticated` default, so anon denies before any gate runs.
//!
//! Throttles (verified, not ported): no D-27 view declares
//! `throttle_classes`, `throttle_scope`, or any `throttle` reference —
//! `grep throttle` over `app/views/cycle/` and `app/views/base.py` is
//! empty. Verified 2026-09-30; same split as the D-35 guards.
//!
//! Ported quirks (translate, don't redesign): none in gate scope beyond
//! the PUT-update fallthrough above, whose gate half is the
//! [`Gate::Authenticated`] row and whose body half belongs to the
//! handlers (PIDASHCONV-321). The split review already corrected the
//! one matrix misreading (user-properties patch is GUEST, `base.py:626`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_types::WorkspaceId;

/// Exact bytes of the `@allow_permission` 403
/// (`app/permissions/base.py`). Alias of
/// [`crate::permissions::PERMISSION_DENIED_BODY`] so handlers have one home.
pub const FORBIDDEN_BODY: &str = crate::permissions::PERMISSION_DENIED_BODY;
/// Exact bytes of the DRF `IsAuthenticated` / `NotAuthenticated` 401:
/// what anonymous callers get on every D-27 route before any gate runs.
pub const ANON_BODY: &str = r#"{"detail":"Authentication credentials were not provided."}"#;

/// How one D-27 route+method authorizes, before any handler logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// No decorator: the `IsAuthenticated` default is the whole gate
    /// (undecorated PUT-update, issue retrieve/update, favorites GET
    /// list). Any authenticated caller reaches the handler body; anon
    /// denies first with [`ANON_BODY`].
    Authenticated,
    /// `@allow_permission(...)` at the default `"PROJECT"` level: active
    /// project membership with a listed role, or the workspace-admin
    /// override (`app/permissions/base.py`). Denies with
    /// [`FORBIDDEN_BODY`].
    Project { roles: &'static [i32] },
    /// `@allow_permission([ADMIN], creator=True, model=Cycle)`
    /// (`base.py:477`): the `app`-copy creator branch
    /// (`app/permissions/base.py:24-38`) — workspace membership is
    /// checked first, then `Cycle.created_by == user` passes any role;
    /// only non-creators fall to the ADMIN role check. Denies with
    /// [`FORBIDDEN_BODY`].
    ProjectCreator { roles: &'static [i32] },
}

/// One row of the F-C27-07 matrix: a route+method and its gate.
pub struct RouteGate {
    pub method: &'static str,
    pub path: &'static str,
    pub gate: Gate,
    /// Python source of the gate for this row.
    pub source: &'static str,
}

const ADMIN: &[i32] = &[ROLE_ADMIN];
const ADMIN_MEMBER: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER];
const ADMIN_MEMBER_GUEST: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];

/// All 14 D-27 routes, one entry per method, in `app/urls/cycle.py` order.
pub static GATES: &[RouteGate] = &[
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:183 (CycleViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:270 (CycleViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:410 (CycleViewSet.retrieve)",
    },
    RouteGate {
        method: "PUT",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        gate: Gate::Authenticated,
        source: "base.py (no def update; PUT falls to UpdateModelMixin.update)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:335 (CycleViewSet.partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        gate: Gate::ProjectCreator { roles: ADMIN },
        source: "base.py:477-478 (CycleViewSet.destroy, creator + ADMIN)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "issue.py:109 (CycleIssueViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "issue.py:223 (CycleIssueViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/",
        gate: Gate::Authenticated,
        source: "issue.py (no def retrieve; falls to ModelViewSet default)",
    },
    RouteGate {
        method: "PUT",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/",
        gate: Gate::Authenticated,
        source: "issue.py (no def update; falls to UpdateModelMixin.update)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/",
        gate: Gate::Authenticated,
        source: "issue.py (no def partial_update; falls to the update default)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "issue.py:299 (CycleIssueViewSet.destroy)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/date-check/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:521 (CycleDateCheckEndpoint.post)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/user-favorite-cycles/",
        gate: Gate::Authenticated,
        source: "base.py (no def list; GET falls to ModelViewSet default list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/user-favorite-cycles/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:571 (CycleFavoriteViewSet.create)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/user-favorite-cycles/<uuid>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:581 (CycleFavoriteViewSet.destroy)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/transfer-issues/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:595 (TransferCycleIssueEndpoint.post)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/user-properties/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:626 (CycleUserPropertiesEndpoint.patch)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/user-properties/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:646 (CycleUserPropertiesEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/progress/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:659 (CycleProgressEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/analytics/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:787 (CycleAnalyticsEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/archive/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "archive.py:271 (CycleArchiveUnarchiveEndpoint.get; pk=None list branch)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/archive/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "archive.py:586 (CycleArchiveUnarchiveEndpoint.post)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/archive/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "archive.py:606 (CycleArchiveUnarchiveEndpoint.delete)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/archived-cycles/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "archive.py:271 (CycleArchiveUnarchiveEndpoint.get; list branch)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/archived-cycles/<uuid>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "archive.py:271 (CycleArchiveUnarchiveEndpoint.get; pk detail branch)",
    },
];

/// Outcome of a gate check, before denial rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The handler body runs.
    Allow,
    /// The handler body does not run: answer 403 with [`deny_body`].
    Deny,
    /// Anonymous on a D-27 route: never reaches a gate; Django-session
    /// authN + `IsAuthenticated` denies first with the 401 [`ANON_BODY`].
    Unauthenticated,
}

/// Which 403/401 body a gated route renders when it does not run the
/// handler: the decorator body for role gates, the 401 for the
/// auth-only fallthroughs (their only pre-body denial is anonymous).
pub fn deny_body(gate: &Gate) -> &'static str {
    match gate {
        Gate::Authenticated => ANON_BODY,
        Gate::Project { .. } | Gate::ProjectCreator { .. } => FORBIDDEN_BODY,
    }
}

fn spec_for(gate: &Gate) -> AllowSpec {
    match gate {
        Gate::Authenticated => AllowSpec {
            // Unreached: `decide_gate` returns before consulting the
            // kernel for auth-only rows.
            level: AllowLevel::Workspace,
            creator_gate: CreatorGate::App,
            creator_bypass: false,
        },
        Gate::Project { .. } => AllowSpec {
            level: AllowLevel::Project,
            creator_gate: CreatorGate::App,
            creator_bypass: false,
        },
        // Only `CycleViewSet.destroy` sets `creator`/`model`
        // (`base.py:477`); the `app`-copy membership gate applies.
        Gate::ProjectCreator { .. } => AllowSpec {
            level: AllowLevel::Project,
            creator_gate: CreatorGate::App,
            creator_bypass: true,
        },
    }
}

/// Decide one gate from pre-fetched membership facts.
///
/// `facts` mirrors one `(user, slug, project_id)` row set: the
/// active-row filters (`is_active=True`) and the `workspace__slug=` /
/// `project_id=` scoping are the caller's SQL (the `tenant_context`
/// half); the scope check denies facts fetched for a different
/// workspace.
pub fn decide_gate(gate: &Gate, scope: &TenantScope, facts: &AllowFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    match gate {
        Gate::Authenticated => GateOutcome::Allow,
        Gate::Project { .. } | Gate::ProjectCreator { .. } => {
            if decide_allow(&spec_for(gate), scope, facts) {
                GateOutcome::Allow
            } else {
                GateOutcome::Deny
            }
        }
    }
}

/// Look up the gate for one route+method; `None` is not a D-27 route.
pub fn gate_for(method: &str, path: &str) -> Option<&'static RouteGate> {
    GATES
        .iter()
        .find(|row| row.method == method && row.path == path)
}

/// Tenant context for one request: the workspace the URL names. Handlers
/// build membership facts only for this slug, so [`decide_gate`] denies
/// cross-workspace facts even when the rows exist.
pub fn tenant_context(slug: &str) -> TenantScope {
    TenantScope::new(WorkspaceId::from(slug))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CYCLES: &str = "workspaces/<slug>/projects/<id>/cycles/";
    const CYCLE: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/";
    const ISSUES: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/";
    const ISSUE: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/";
    const FAVS: &str = "workspaces/<slug>/projects/<id>/user-favorite-cycles/";
    const FAV: &str = "workspaces/<slug>/projects/<id>/user-favorite-cycles/<uuid>/";
    const TRANSFER: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/transfer-issues/";
    const USERPROPS: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/user-properties/";
    const PROGRESS: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/progress/";
    const ANALYTICS: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/analytics/";
    const ARCHIVE: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/archive/";
    const ARCHIVED: &str = "workspaces/<slug>/projects/<id>/archived-cycles/";
    const ARCHIVED_ONE: &str = "workspaces/<slug>/projects/<id>/archived-cycles/<uuid>/";
    const DATE_CHECK: &str = "workspaces/<slug>/projects/<id>/cycles/date-check/";

    fn scope() -> TenantScope {
        tenant_context("acme")
    }

    /// Facts for an authenticated caller holding `role` in both the
    /// workspace and the project (the contract-suite world: admin,
    /// member and guest are members of both), for a gate with the given
    /// allowed-role list. Non-creator: the destroy row denies
    /// member/guest here; the creator bypass has its own test.
    fn facts_for(role: i32, allowed: &[i32]) -> AllowFacts {
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: allowed.contains(&role),
            is_creator: false,
            has_allowed_project_role: allowed.contains(&role),
            is_project_member: true,
            is_workspace_admin: role == ROLE_ADMIN,
        }
    }

    fn decide_path(method: &str, path: &str, role: Option<i32>) -> GateOutcome {
        let row = gate_for(method, path).expect("fixture route must have a gate");
        let facts = match (row.gate, role) {
            (_, None) => AllowFacts {
                workspace: WorkspaceId::from("acme"),
                authenticated: false,
                is_workspace_member: false,
                has_allowed_workspace_role: false,
                is_creator: false,
                has_allowed_project_role: false,
                is_project_member: false,
                is_workspace_admin: false,
            },
            (Gate::Project { roles }, Some(role))
            | (Gate::ProjectCreator { roles }, Some(role)) => facts_for(role, roles),
            // Auth-only fallthroughs: any authenticated role passes.
            (Gate::Authenticated, Some(role)) => {
                facts_for(role, &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST])
            }
        };
        decide_gate(&row.gate, &scope(), &facts)
    }

    #[test]
    fn table_covers_every_gated_site() {
        assert_eq!(GATES.len(), 26, "14 routes, one row per method+path");
        assert!(gate_for("GET", "nope/").is_none());
        for row in GATES {
            assert!(
                gate_for(row.method, row.path).is_some(),
                "row must round-trip: {} {}",
                row.method,
                row.path
            );
        }
        // Exactly one creator gate: only destroy sets creator/model.
        assert_eq!(
            GATES
                .iter()
                .filter(|row| matches!(row.gate, Gate::ProjectCreator { .. }))
                .count(),
            1,
            "only CycleViewSet.destroy carries the creator bypass"
        );
    }

    /// Full F-C27-07 matrix: (method, path, admin, member, guest) with
    /// `true` = handler runs. Member/guest on DELETE are non-creators
    /// (the creator bypass has its own test); every MEMBER-only `false`
    /// under guest is a denied-permission probe, one per protected
    /// action.
    const MATRIX: &[(&str, &str, bool, bool, bool)] = &[
        ("GET", CYCLES, true, true, true),
        ("POST", CYCLES, true, true, false),
        ("GET", CYCLE, true, true, false),
        ("PUT", CYCLE, true, true, true),
        ("PATCH", CYCLE, true, true, false),
        ("DELETE", CYCLE, true, false, false),
        ("GET", ISSUES, true, true, false),
        ("POST", ISSUES, true, true, false),
        ("GET", ISSUE, true, true, true),
        ("PUT", ISSUE, true, true, true),
        ("PATCH", ISSUE, true, true, true),
        ("DELETE", ISSUE, true, true, false),
        ("POST", DATE_CHECK, true, true, false),
        ("GET", FAVS, true, true, true),
        ("POST", FAVS, true, true, false),
        ("DELETE", FAV, true, true, false),
        ("POST", TRANSFER, true, true, false),
        ("PATCH", USERPROPS, true, true, true),
        ("GET", USERPROPS, true, true, true),
        ("GET", PROGRESS, true, true, true),
        ("GET", ANALYTICS, true, true, true),
        ("GET", ARCHIVE, true, true, false),
        ("POST", ARCHIVE, true, true, false),
        ("DELETE", ARCHIVE, true, true, false),
        ("GET", ARCHIVED, true, true, false),
        ("GET", ARCHIVED_ONE, true, true, false),
    ];

    #[test]
    fn matrix_matches_django_allow_deny() {
        assert_eq!(MATRIX.len(), GATES.len());
        for (method, path, admin, member, guest) in MATRIX {
            for (role, role_name, expected) in [
                (ROLE_ADMIN, "ADMIN", *admin),
                (ROLE_MEMBER, "MEMBER", *member),
                (ROLE_GUEST, "GUEST", *guest),
            ] {
                let outcome = decide_path(method, path, Some(role));
                let expected_outcome = if expected {
                    GateOutcome::Allow
                } else {
                    GateOutcome::Deny
                };
                assert_eq!(
                    outcome, expected_outcome,
                    "{method} {path} role {role_name}"
                );
            }
        }
    }

    #[test]
    fn destroy_creator_gate_runs_before_the_role_check() {
        // `base.py:477` + `app/permissions/base.py:24-38`: the creator
        // passes regardless of role; only non-creators reach ADMIN.
        let row = gate_for("DELETE", CYCLE).expect("destroy row");
        let mut creator_member = facts_for(ROLE_MEMBER, ADMIN);
        creator_member.is_creator = true;
        assert_eq!(
            decide_gate(&row.gate, &scope(), &creator_member),
            GateOutcome::Allow,
            "creator MEMBER passes"
        );
        let mut creator_guest = facts_for(ROLE_GUEST, ADMIN);
        creator_guest.is_creator = true;
        assert_eq!(
            decide_gate(&row.gate, &scope(), &creator_guest),
            GateOutcome::Allow,
            "creator GUEST passes"
        );
        // The `app`-copy membership gate fires first: a creator who is
        // not a workspace member is refused without reaching the bypass.
        let mut outsider_creator = facts_for(ROLE_MEMBER, ADMIN);
        outsider_creator.is_creator = true;
        outsider_creator.is_workspace_member = false;
        assert_eq!(
            decide_gate(&row.gate, &scope(), &outsider_creator),
            GateOutcome::Deny,
            "non-member creator is refused first"
        );
        // Non-creator MEMBER was already denied by the matrix; pin the
        // ADMIN non-creator allow explicitly (role check after bypass).
        assert_eq!(
            decide_path("DELETE", CYCLE, Some(ROLE_ADMIN)),
            GateOutcome::Allow
        );
    }

    #[test]
    fn anonymous_never_reaches_a_gate() {
        // Every D-27 route answers 401 ANON before any gate runs
        // (session authN + IsAuthenticated default on both base views).
        for row in GATES {
            assert_eq!(
                decide_path(row.method, row.path, None),
                GateOutcome::Unauthenticated,
                "anon {} {}",
                row.method,
                row.path
            );
        }
    }

    #[test]
    fn denial_bodies_are_byte_identical() {
        assert_eq!(
            FORBIDDEN_BODY,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(
            ANON_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        // Role gates render the decorator body; auth-only rows only ever
        // render the 401 (their sole pre-body denial is anonymous).
        for row in GATES {
            let expected = match row.gate {
                Gate::Authenticated => ANON_BODY,
                Gate::Project { .. } | Gate::ProjectCreator { .. } => FORBIDDEN_BODY,
            };
            assert_eq!(
                deny_body(&row.gate),
                expected,
                "{} {}",
                row.method,
                row.path
            );
        }
    }

    #[test]
    fn cross_workspace_facts_deny_every_protected_action() {
        // Tenant-isolation probe, one per protected action: an ADMIN
        // holding valid rows in another workspace denies on every role
        // gate, even with the ADMIN role. Auth-only fallthroughs pass
        // any authenticated caller at the gate (isolation there is the
        // handlers' queryset scoping, not a role check).
        let other_facts = AllowFacts {
            workspace: WorkspaceId::from("other"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            is_creator: true,
            has_allowed_project_role: true,
            is_project_member: true,
            is_workspace_admin: true,
        };
        let mut protected = 0;
        for row in GATES {
            match row.gate {
                Gate::Project { .. } | Gate::ProjectCreator { .. } => {
                    protected += 1;
                    assert_eq!(
                        decide_gate(&row.gate, &scope(), &other_facts),
                        GateOutcome::Deny,
                        "isolation {} {}",
                        row.method,
                        row.path
                    );
                }
                Gate::Authenticated => {
                    assert_eq!(
                        decide_gate(&row.gate, &scope(), &other_facts),
                        GateOutcome::Allow,
                        "auth-only {} {}",
                        row.method,
                        row.path
                    );
                }
            }
        }
        // 19 decorated actions over 21 rows: archive GET serves three
        // paths (`archive/`, `archived-cycles/`,
        // `archived-cycles/<uuid>/`), so every protected action is
        // probed at least once.
        assert_eq!(protected, 21, "one isolation probe per protected row");
    }

    #[test]
    fn project_admin_bypass_needs_project_membership() {
        // The PROJECT-level override (`app/permissions/base.py`): an
        // active project member whose workspace role is ADMIN passes a
        // MEMBER gate on any project role — including GUEST.
        let mut bypass = facts_for(ROLE_GUEST, ADMIN_MEMBER);
        bypass.is_workspace_admin = true;
        let gate = Gate::Project {
            roles: ADMIN_MEMBER,
        };
        assert_eq!(
            decide_gate(&gate, &scope(), &bypass),
            GateOutcome::Allow,
            "ws-admin project member passes regardless of project role"
        );
        // ...but a workspace admin who is not a project member denies.
        let mut no_project = facts_for(ROLE_ADMIN, ADMIN_MEMBER);
        no_project.has_allowed_project_role = false;
        no_project.is_project_member = false;
        assert_eq!(decide_gate(&gate, &scope(), &no_project), GateOutcome::Deny);
    }
}
