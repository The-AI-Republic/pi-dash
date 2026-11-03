//! D-35 permission gates (stage 5, PIDASHCONV-358).
//!
//! Ports the `@allow_permission` gates on all 14 D-35 routes (18
//! method+path rows: the analytic-view viewset serves GET/POST on the
//! list and GET/PATCH/DELETE on the detail) and the
//! `WorkSpaceAdminPermission` class on `AnalyticViewViewset`, from:
//!
//! - `apps/api/pi_dash/app/views/analytic/base.py` (6 gated units)
//! - `apps/api/pi_dash/app/views/analytic/advance.py` (3 gates + base)
//! - `apps/api/pi_dash/app/views/analytic/project_analytics.py` (3 gates + base)
//! - `apps/api/pi_dash/app/views/exporter/base.py`
//!   (`ExportIssuesEndpoint`, GET + POST)
//!
//! Fixture id: FX-A-G-01
//! (`rust-api/fixtures/app_analytics/guards/analytics_guards.golden.json`;
//! tripwires pinned by
//! `rust-api/contract-tests/app_analytics/test_permissions.py`).
//!
//! Shape of the port: the module is pure, like the
//! [`pidash_auth::permissions::allow`] kernel it sits on. Membership facts
//! are caller inputs ([`AllowFacts`]); row fetching (the
//! `WorkspaceMember`/`ProjectMember ...exists()` queries with their
//! `workspace__slug=` / `project_id=` / `is_active=True` filters) stays
//! with the handlers, which fetch through the workspace-scoped handle —
//! the `tenant_context` half of the pilot pattern (`app_issues/mod.rs`
//! `resolve_gate`). This module only maps each route to its [`Gate`] and
//! decides through the kernel, so the PROJECT-level workspace-admin
//! override (`app/permissions/base.py`: project member + workspace role
//! exactly ADMIN passes regardless of project role) and the
//! deny-by-default tenant scope come along unchanged.
//!
//! Gate order (preserved, not redesigned):
//!
//! - Decorated routes: DRF authentication (`BaseAPIView`: session auth +
//!   `IsAuthenticated`, `app/views/base.py`) runs before the decorator,
//!   and the decorator runs before the handler body.
//! - The viewset carries `permission_classes = [WorkSpaceAdminPermission]`
//!   *instead of* the `IsAuthenticated` default (`base.py:177-178`).
//!   Anonymous callers still 401: DRF `permission_denied` raises
//!   `NotAuthenticated` when authenticators exist but no session
//!   authenticated, so anon on the viewset answers the same ANON body
//!   (pinned by `test_unauthenticated_is_401`, which covers the
//!   analytic-view paths).
//! - Anonymous callers on every other D-35 route never reach a gate:
//!   `IsAuthenticated` denies first with the 401 ANON body.
//!
//! Denial bodies (byte-exact, compact DRF rendering):
//!
//! - Decorator denial: `{"error":"You don't have the required permissions."}`
//!   (403, `app/permissions/base.py`); see [`FORBIDDEN_BODY`].
//! - Viewset class denial: DRF-default `PermissionDenied` body
//!   (`{"detail":"You do not have permission to perform this action."}`,
//!   403 — `WorkSpaceAdminPermission` sets no `message`); see
//!   [`VIEWSET_FORBIDDEN_BODY`]. Use [`deny_body`] to pick per gate.
//! - Anonymous: `{"detail":"Authentication credentials were not provided."}`
//!   (401); see [`ANON_BODY`].
//! - Missing object: `{"error":"The required object does not exist."}`
//!   (404, `handle_exception`'s `ObjectDoesNotExist` branch); see
//!   [`NOT_FOUND_BODY`]. Handlers render it; recorded here so the matrix
//!   has one home.
//!
//! Throttles (verified, not ported): no D-35 view declares
//! `throttle_classes`, `throttle_scope`, or any `throttle` reference —
//! `grep throttle` over `app/views/analytic/`, `app/views/exporter/` and
//! `app/views/base.py` is empty. Rate limiting on these routes comes only
//! from shared `BaseAPIView` infrastructure, which is cross-cutting and
//! outside this guard port (same split as PIDASHCONV-367).
//!
//! Ported quirks (translate, don't redesign): none in gate scope. The
//! class name `WorkSpaceAdminPermission` suggests admins only, but the
//! code allows roles `[Admin, Member]` (`workspace.py:61-71`); the table
//! pins code behavior (MEMBER allow), matching the fixture. The
//! PROJECT-level default (`project_analytics.py:84,165`: no `level=`
//! argument, so the decorator default `"PROJECT"` applies) keeps the
//! workspace-admin bypass via the kernel.
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
/// Exact bytes of the DRF-default class-denial 403 the viewset renders
/// (`WorkSpaceAdminPermission` sets no `message`). Alias of
/// [`crate::permissions::DEFAULT_DENIED_BODY`].
pub const VIEWSET_FORBIDDEN_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;
/// Exact bytes of the DRF `IsAuthenticated` / `NotAuthenticated` 401.
pub const ANON_BODY: &str = r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (404), rendered by
/// handlers when a gated lookup misses.
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;

/// How one D-35 route+method authorizes, before any handler logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `@allow_permission(..., level="WORKSPACE")`: active workspace
    /// membership with a listed role (`app/permissions/base.py`). Denies
    /// with [`FORBIDDEN_BODY`].
    Workspace { roles: &'static [i32] },
    /// `@allow_permission(...)` at the default `"PROJECT"` level: active
    /// project membership with a listed role, or the workspace-admin
    /// override (`base.py`). Denies with [`FORBIDDEN_BODY`].
    Project { roles: &'static [i32] },
    /// `AnalyticViewViewset.permission_classes =
    /// [WorkSpaceAdminPermission]` (`analytic/base.py:177-178`): active
    /// workspace membership with role ADMIN or MEMBER
    /// (`permissions/workspace.py:61-71`). Denies with
    /// [`VIEWSET_FORBIDDEN_BODY`], not the decorator body.
    ViewsetAdmin,
}

/// One row of the FX-A-G-01 matrix: a route+method and its gate.
pub struct RouteGate {
    pub method: &'static str,
    pub path: &'static str,
    pub gate: Gate,
    /// Python source of the gate for this row.
    pub source: &'static str,
}

const ADMIN_MEMBER: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER];
const ADMIN_MEMBER_GUEST: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];

/// All 14 D-35 routes, one entry per method, in FX-A-G-01 order.
pub static GATES: &[RouteGate] = &[
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/analytics/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "analytic/base.py:39 (AnalyticsEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/analytic-view/",
        gate: Gate::ViewsetAdmin,
        source: "analytic/base.py:178 (AnalyticViewViewset list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/analytic-view/",
        gate: Gate::ViewsetAdmin,
        source: "analytic/base.py:178 (AnalyticViewViewset create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/analytic-view/<uuid>/",
        gate: Gate::ViewsetAdmin,
        source: "analytic/base.py:178 (AnalyticViewViewset retrieve)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/analytic-view/<uuid>/",
        gate: Gate::ViewsetAdmin,
        source: "analytic/base.py:178 (AnalyticViewViewset partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/analytic-view/<uuid>/",
        gate: Gate::ViewsetAdmin,
        source: "analytic/base.py:178 (AnalyticViewViewset destroy)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/saved-analytic-view/<uuid>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "analytic/base.py:191 (SavedAnalyticEndpoint.get)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/export-analytics/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "analytic/base.py:224 (ExportAnalyticsEndpoint.post)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/default-analytics/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "analytic/base.py:253 (DefaultAnalyticsEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/project-stats/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "analytic/base.py:392 (ProjectStatsEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/advance-analytics/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "analytic/advance.py:137 (AdvanceAnalyticsEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/advance-analytics-stats/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "analytic/advance.py:191 (AdvanceAnalyticsStatsEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/advance-analytics-charts/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "analytic/advance.py:318 (AdvanceAnalyticsChartEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/advance-analytics/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "analytic/project_analytics.py:84 (default PROJECT level)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/advance-analytics-stats/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "analytic/project_analytics.py:165 (default PROJECT level)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/advance-analytics-charts/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "analytic/project_analytics.py:317 (GUEST allowed)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/export-issues/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "exporter/base.py:67 (ExportIssuesEndpoint.get)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/export-issues/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "exporter/base.py:22 (ExportIssuesEndpoint.post)",
    },
];

/// Outcome of a gate check, before denial rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The handler body runs.
    Allow,
    /// The handler body does not run: answer 403 with [`deny_body`].
    Deny,
    /// Anonymous on a D-35 route: never reaches a gate; DRF
    /// `IsAuthenticated` (or `permission_denied` → `NotAuthenticated` on
    /// the viewset) denies first with the 401 [`ANON_BODY`].
    Unauthenticated,
}

/// Which 403 body a denied gate renders: the decorator body, except the
/// viewset class gate, which renders the DRF-default body.
pub fn deny_body(gate: &Gate) -> &'static str {
    match gate {
        Gate::ViewsetAdmin => VIEWSET_FORBIDDEN_BODY,
        Gate::Workspace { .. } | Gate::Project { .. } => FORBIDDEN_BODY,
    }
}

fn spec_for(gate: &Gate) -> AllowSpec {
    let level = match gate {
        Gate::Workspace { .. } => AllowLevel::Workspace,
        Gate::Project { .. } => AllowLevel::Project,
        // Same membership check as a WORKSPACE ADMIN_MEMBER gate
        // (`workspace.py:61-71`); only the denial body differs.
        Gate::ViewsetAdmin => AllowLevel::Workspace,
    };
    AllowSpec {
        level,
        creator_gate: CreatorGate::App,
        creator_bypass: false,
    }
}

/// Decide one gate from pre-fetched membership facts.
///
/// `facts` mirrors one `(user, slug[, project_id])` row set: the
/// active-row filters (`is_active=True`) and the `workspace__slug=` /
/// `project_id=` scoping are the caller's SQL (the `tenant_context`
/// half); the scope check denies facts fetched for a different
/// workspace. D-35 never sets `creator`/`model`, so the creator bypass
/// is always off.
pub fn decide_gate(gate: &Gate, scope: &TenantScope, facts: &AllowFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    // `ViewsetAdmin` shares the kernel's WORKSPACE branch: anonymous
    // already returned above, and the caller sets
    // `has_allowed_workspace_role` for `[ADMIN, MEMBER]`
    // (`workspace.py:61-71`). Only the denial body differs (see
    // [`deny_body`]).
    if decide_allow(&spec_for(gate), scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::Deny
    }
}

/// Look up the gate for one route+method; `None` is not a D-35 route.
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

    fn scope() -> TenantScope {
        tenant_context("acme")
    }

    /// Facts for an authenticated caller holding `role` in both the
    /// workspace and the project (the contract-suite world: admin, member
    /// and guest are members of both), for a gate with the given
    /// allowed-role list.
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
            (Gate::Workspace { roles }, Some(role)) => facts_for(role, roles),
            (Gate::Project { roles }, Some(role)) => facts_for(role, roles),
            // The class gate allows the same roles as a WORKSPACE
            // ADMIN_MEMBER gate; only the denial body differs.
            (Gate::ViewsetAdmin, Some(role)) => facts_for(role, ADMIN_MEMBER),
        };
        decide_gate(&row.gate, &scope(), &facts)
    }

    #[test]
    fn table_covers_every_fixture_row() {
        assert_eq!(GATES.len(), 18, "14 routes, one row per method+path");
        assert!(gate_for("GET", "nope/").is_none());
        for row in GATES {
            assert!(
                gate_for(row.method, row.path).is_some(),
                "row must round-trip: {} {}",
                row.method,
                row.path
            );
        }
    }

    /// Full FX-A-G-01 matrix: (method, path, admin, member, guest) with
    /// `true` = handler runs.
    const MATRIX: &[(&str, &str, bool, bool, bool)] = &[
        ("GET", "workspaces/<slug>/analytics/", true, true, false),
        ("GET", "workspaces/<slug>/analytic-view/", true, true, false),
        (
            "POST",
            "workspaces/<slug>/analytic-view/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/analytic-view/<uuid>/",
            true,
            true,
            false,
        ),
        (
            "PATCH",
            "workspaces/<slug>/analytic-view/<uuid>/",
            true,
            true,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/analytic-view/<uuid>/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/saved-analytic-view/<uuid>/",
            true,
            true,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/export-analytics/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/default-analytics/",
            true,
            true,
            true,
        ),
        ("GET", "workspaces/<slug>/project-stats/", true, true, true),
        (
            "GET",
            "workspaces/<slug>/advance-analytics/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/advance-analytics-stats/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/advance-analytics-charts/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<id>/advance-analytics/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<id>/advance-analytics-stats/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<id>/advance-analytics-charts/",
            true,
            true,
            true,
        ),
        ("GET", "workspaces/<slug>/export-issues/", true, true, false),
        (
            "POST",
            "workspaces/<slug>/export-issues/",
            true,
            true,
            false,
        ),
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
    fn anonymous_never_reaches_a_gate() {
        // Every D-35 route answers 401 ANON before any gate runs
        // (IsAuthenticated default; permission_denied → NotAuthenticated
        // on the viewset).
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
            VIEWSET_FORBIDDEN_BODY,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(
            ANON_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            NOT_FOUND_BODY,
            r#"{"error":"The required object does not exist."}"#
        );
        // The viewset class gate renders the DRF-default body; every
        // decorator gate renders the allow-style body.
        for row in GATES {
            let expected = match row.gate {
                Gate::ViewsetAdmin => VIEWSET_FORBIDDEN_BODY,
                Gate::Workspace { .. } | Gate::Project { .. } => FORBIDDEN_BODY,
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
    fn cross_workspace_facts_deny() {
        // An outsider holds valid rows in another workspace: facts
        // fetched for the wrong slug deny even with an ADMIN role.
        let other_facts = AllowFacts {
            workspace: WorkspaceId::from("other"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            is_creator: false,
            has_allowed_project_role: true,
            is_project_member: true,
            is_workspace_admin: true,
        };
        for gate in [
            Gate::Workspace {
                roles: ADMIN_MEMBER,
            },
            Gate::Workspace {
                roles: ADMIN_MEMBER_GUEST,
            },
            Gate::Project {
                roles: ADMIN_MEMBER,
            },
            Gate::ViewsetAdmin,
        ] {
            assert_eq!(
                decide_gate(&gate, &scope(), &other_facts),
                GateOutcome::Deny,
                "{gate:?}"
            );
        }
    }

    #[test]
    fn project_admin_bypass_needs_project_membership() {
        // The PROJECT-level override (base.py): a workspace admin who is
        // not a project member still denies.
        let ws_admin_no_project = AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: true,
        };
        let gate = Gate::Project {
            roles: ADMIN_MEMBER,
        };
        assert_eq!(
            decide_gate(&gate, &scope(), &ws_admin_no_project),
            GateOutcome::Deny
        );
    }
}
