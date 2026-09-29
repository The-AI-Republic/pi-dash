//! D-33 permission gates (stage 5, PIDASHCONV-436).
//!
//! Ports the `allow_permission` gates on all 24 D-33 routes (27 fixture
//! rows, one [`RouteGate`] per method), the two `AllowAny` endpoints, and
//! the manual admin checks in:
//!
//! - `apps/api/pi_dash/app/views/integration/git.py:51-158`
//! - `apps/api/pi_dash/app/views/integration/github.py:429-1174`
//! - `apps/api/pi_dash/app/views/webhook/base.py:21-122`
//! - `apps/api/pi_dash/app/views/external/base.py:149-215`
//!
//! Fixture ids: FX-PERM-01
//! (`rust-api/fixtures/app_integrations/fx-perm-01-permission-matrix.json`),
//! FX-GHA-01 (`fx-gha-01-app-flow.json`, HMAC vectors + callback branches).
//!
//! Shape of the port: the module is pure, like the
//! [`pidash_auth::permissions::allow`] kernel it sits on. Membership facts
//! are caller inputs ([`AllowFacts`]); row fetching (the
//! `WorkspaceMember`/`ProjectMember ...exists()` queries with their
//! `workspace__slug=` / `project_id=` / `is_active=True` filters) stays
//! with the handlers, which fetch through the workspace-scoped handle —
//! the `tenant_context` half of the pilot pattern. This module only maps
//! each route to its [`Gate`] and decides through the kernel, so the
//! workspace-admin override (`base.py:56-64`: project member + workspace
//! role exactly ADMIN passes regardless of project role) and the
//! deny-by-default tenant scope come along unchanged.
//!
//! Gate order (preserved, not redesigned):
//!
//! - Decorated routes: DRF authentication (`BaseAPIView`: session auth +
//!   `IsAuthenticated`, `views/base.py:189-194`) runs before the decorator,
//!   and the decorator runs before the handler body — so the
//!   `_feature_enabled` 404 (`github.py:82-87`) fires only *after* a
//!   passing gate on decorated routes.
//! - `AllowAny` routes skip DRF auth entirely: the callback checks the
//!   disabled flag first (302 to the profile URL with `github_app=disabled`,
//!   `github.py:736-737`, never the 404), then redirects anonymous users
//!   with `error=login_required` (`github.py:741-742`, a 302, not a 403);
//!   the webhook checks the disabled flag first, then the HMAC signature
//!   ([`crate::app_integrations::hmac`]) — the signature *is* the auth,
//!   which is why `throttle_classes = []` (`github.py:864-867`).
//!
//! Denial bodies (byte-exact, compact DRF rendering):
//!
//! - Decorator denial: `{"error":"You don't have the required permissions."}`
//!   (403, `app/permissions/base.py:80-84`); see
//!   [`crate::permissions::PERMISSION_DENIED_BODY`].
//! - Install-start manual check: 403
//!   `{"error":"You must be a workspace admin to install the GitHub App"}`
//!   (`github.py:674-678`).
//! - Refresh manual check: 403
//!   `{"error":"You must be a workspace admin to refresh this connection"}`
//!   (`github.py:708-712`).
//! - Disabled flag: 404
//!   `{"error":"GitHub integration is disabled on this instance"}`
//!   (`github.py:82-87`) — except the callback's 302 redirect above.
//! - Webhook bad signature: 401 `{"error":"Invalid signature"}`
//!   (`github.py:873-874`).
//! - Anonymous callers on non-`AllowAny` routes never reach a gate:
//!   `IsAuthenticated` denies first (401/403 per the DRF/session default,
//!   fixture `anonymous` legend).
//!
//! Ported bugs (translate, don't redesign — both live outside this
//! module's scope and are recorded here only because the gate table walks
//! past them):
//!
//! - BUG webhook PATCH context (`webhook/base.py:84`): the update path
//!   passes `context={request: request}` (the request object as the key),
//!   so the request-host domain is never appended on update. Fixture B2.
//! - BUG unsplash page param (`external/base.py:235`): the search URL
//!   renders `page=${page}` with a stray `$`; port byte-for-byte.
//!   Fixture B3.

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_types::WorkspaceId;

/// Exact bytes of the install-start manual 403 (`github.py:674-678`).
pub const INSTALL_ADMIN_REQUIRED_BODY: &str =
    r#"{"error":"You must be a workspace admin to install the GitHub App"}"#;
/// Exact bytes of the refresh manual 403 (`github.py:708-712`).
pub const REFRESH_ADMIN_REQUIRED_BODY: &str =
    r#"{"error":"You must be a workspace admin to refresh this connection"}"#;
/// Exact bytes of the disabled-flag 404 (`github.py:82-87`).
pub const GITHUB_DISABLED_BODY: &str =
    r#"{"error":"GitHub integration is disabled on this instance"}"#;
/// Exact bytes of the webhook bad-signature 401 (`github.py:873-874`).
pub const WEBHOOK_BAD_SIGNATURE_BODY: &str = r#"{"error":"Invalid signature"}"#;

/// How one D-33 route+method authorizes, before any handler logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// No DRF auth at all (`permission_classes = [AllowAny]`): the
    /// callback (`github.py:733`) and the webhook (`github.py:864`, with
    /// `throttle_classes = []`). Branch logic (login redirect / HMAC)
    /// lives in the handler.
    AllowAny,
    /// `BaseAPIView` default (`IsAuthenticated`) with no decorator: any
    /// logged-in user reaches the handler body, which enforces its own
    /// rule (manual `_is_workspace_admin` re-check on install-start and
    /// refresh; ADMIN-membership filter on app-status; no check at all on
    /// unsplash).
    Authenticated,
    /// `@allow_permission(..., level="WORKSPACE")`: active workspace
    /// membership with a listed role (`base.py:44-51`).
    Workspace { roles: &'static [i32] },
    /// `@allow_permission(...)` at the default `"PROJECT"` level: active
    /// project membership with a listed role, or workspace-admin override
    /// (`base.py:53-64`).
    Project { roles: &'static [i32] },
}

/// One row of the FX-PERM-01 matrix: a route+method and its gate.
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

/// All 24 D-33 routes, one entry per method, in FX-PERM-01 order.
pub static GATES: &[RouteGate] = &[
    RouteGate {
        method: "POST",
        path: "users/me/integrations/github/app/install/",
        gate: Gate::Authenticated,
        source: "github.py:658 (no decorator; manual _is_workspace_admin 674-678)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/integrations/github/app/",
        gate: Gate::Authenticated,
        source: "github.py:613 (no decorator; ADMIN-membership filter 628-646)",
    },
    RouteGate {
        method: "POST",
        path: "users/me/integrations/github/app/refresh/",
        gate: Gate::Authenticated,
        source: "github.py:697 (no decorator; manual _is_workspace_admin 708-712)",
    },
    RouteGate {
        method: "GET",
        path: "integrations/github/app/callback/",
        gate: Gate::AllowAny,
        source: "github.py:730-734 (AllowAny; login_required redirect 741-742)",
    },
    RouteGate {
        method: "POST",
        path: "integrations/github/app/webhook/",
        gate: Gate::AllowAny,
        source: "github.py:860-867 (AllowAny, throttle_classes=[]; HMAC 873)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/integrations/github/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "github.py:547",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/integrations/github/connect/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "github.py:429",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/integrations/github/disconnect/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "github.py:502",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/integrations/github/repos/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "github.py:572",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/integrations/git/providers/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "git.py:51",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/integrations/git/accounts/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "git.py:58",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/integrations/git/accounts/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "git.py:63-70",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/integrations/git/accounts/<uuid>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "git.py:98",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/integrations/git/accounts/<uuid>/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "git.py:101-104",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/integrations/git/accounts/<uuid>/repos/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "git.py:118",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/repository/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "git.py:133 (widest read in D-33)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<id>/repository/",
        gate: Gate::Project { roles: ADMIN },
        source: "git.py:140",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/repository/",
        gate: Gate::Project { roles: ADMIN },
        source: "git.py:147",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/repository/bind/",
        gate: Gate::Project { roles: ADMIN },
        source: "git.py:157",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/github/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "github.py:1093",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<id>/github/",
        gate: Gate::Project { roles: ADMIN },
        source: "github.py:1148",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/github/",
        gate: Gate::Project { roles: ADMIN },
        source: "github.py:1174",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/github/bind/",
        gate: Gate::Project { roles: ADMIN },
        source: "github.py:981",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/webhooks/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "webhook/base.py:22",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/webhooks/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "webhook/base.py:39",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/webhooks/<uuid>/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "webhook/base.py:78",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/webhooks/<uuid>/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "webhook/base.py:104",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/webhooks/<uuid>/regenerate/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "webhook/base.py:111",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/webhook-logs/<uuid>/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "webhook/base.py:121",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/ai-assistant/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "external/base.py:149 (@allow_permission([ADMIN, MEMBER]), default PROJECT level)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/ai-assistant/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER,
        },
        source: "external/base.py:185",
    },
    RouteGate {
        method: "GET",
        path: "unsplash/",
        gate: Gate::Authenticated,
        source: "external/base.py:215 (no decorator; BaseAPIView IsAuthenticated only)",
    },
];

/// Outcome of a gate check, before denial rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The handler body runs.
    Allow,
    /// The handler body does not run: answer the allow-style 403
    /// ([`crate::permissions::PERMISSION_DENIED_BODY`]).
    Deny,
    /// Anonymous on a non-`AllowAny` route: never reaches a gate; DRF
    /// `IsAuthenticated` denies first (401/403 per the session default).
    Unauthenticated,
}

fn spec_for(gate: &Gate) -> AllowSpec {
    let level = match gate {
        Gate::Workspace { .. } => AllowLevel::Workspace,
        Gate::Project { .. } => AllowLevel::Project,
        Gate::AllowAny | Gate::Authenticated => AllowLevel::Workspace,
    };
    AllowSpec {
        level,
        creator_gate: CreatorGate::App,
        creator_bypass: false,
    }
}

/// Decide one gate from pre-fetched membership facts.
///
/// `facts` mirrors one `(user, slug[, project_id])` row set: the active-row
/// filters (`is_active=True`) and the `workspace__slug=` / `project_id=`
/// scoping are the caller's SQL (the `tenant_context` half); the scope
/// check denies facts fetched for a different workspace. D-33 never sets
/// `creator`/`model`, so the creator bypass is always off.
pub fn decide_gate(gate: &Gate, scope: &TenantScope, facts: &AllowFacts) -> GateOutcome {
    if !facts.authenticated {
        return match gate {
            Gate::AllowAny => GateOutcome::Allow,
            _ => GateOutcome::Unauthenticated,
        };
    }
    match gate {
        Gate::AllowAny | Gate::Authenticated => GateOutcome::Allow,
        Gate::Workspace { .. } | Gate::Project { .. } => {
            if decide_allow(&spec_for(gate), scope, facts) {
                GateOutcome::Allow
            } else {
                GateOutcome::Deny
            }
        }
    }
}

/// Manual workspace-admin re-check shared by the install-start
/// (`github.py:674`) and refresh (`github.py:708`) bodies: the caller is
/// already authenticated (the [`Gate::Authenticated`] gate passed); the
/// body answers its own 403 when the caller is not an active workspace
/// admin. Pure: `is_admin` is the `_is_workspace_admin` exists() row.
pub fn manual_admin_allows(is_admin: bool) -> bool {
    is_admin
}

/// Look up the gate for one route+method; `None` is not a D-33 route.
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

    /// Facts for an authenticated caller whose workspace role and project
    /// role are both `role` (active rows in the request workspace), for a
    /// gate with the given allowed-role list.
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

    fn decide_path(path: &str, method: &str, role: Option<i32>) -> GateOutcome {
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
            (Gate::AllowAny, _) | (Gate::Authenticated, _) => facts_for(
                role.unwrap_or(ROLE_ADMIN),
                &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST],
            ),
        };
        decide_gate(&row.gate, &scope(), &facts)
    }

    #[test]
    fn table_covers_every_fixture_row() {
        assert_eq!(GATES.len(), 32, "24 routes, one row per method");
        assert!(gate_for("GET", "nope/").is_none());
    }

    /// Full FX-PERM-01 matrix: (method, path, admin, member, guest) with
    /// `true` = handler runs.
    const MATRIX: &[(&str, &str, bool, bool, bool)] = &[
        (
            "POST",
            "users/me/integrations/github/app/install/",
            true,
            true,
            true,
        ),
        ("GET", "users/me/integrations/github/app/", true, true, true),
        (
            "POST",
            "users/me/integrations/github/app/refresh/",
            true,
            true,
            true,
        ),
        ("GET", "integrations/github/app/callback/", true, true, true),
        ("POST", "integrations/github/app/webhook/", true, true, true),
        (
            "GET",
            "workspaces/<slug>/integrations/github/",
            true,
            true,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/integrations/github/connect/",
            true,
            false,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/integrations/github/disconnect/",
            true,
            false,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/integrations/github/repos/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/integrations/git/providers/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/integrations/git/accounts/",
            true,
            true,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/integrations/git/accounts/",
            true,
            false,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/integrations/git/accounts/<uuid>/",
            true,
            true,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/integrations/git/accounts/<uuid>/",
            true,
            false,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/integrations/git/accounts/<uuid>/repos/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<id>/repository/",
            true,
            true,
            true,
        ),
        (
            "PATCH",
            "workspaces/<slug>/projects/<id>/repository/",
            true,
            false,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/projects/<id>/repository/",
            true,
            false,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/projects/<id>/repository/bind/",
            true,
            false,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<id>/github/",
            true,
            true,
            true,
        ),
        (
            "PATCH",
            "workspaces/<slug>/projects/<id>/github/",
            true,
            false,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/projects/<id>/github/",
            true,
            false,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/projects/<id>/github/bind/",
            true,
            false,
            false,
        ),
        ("POST", "workspaces/<slug>/webhooks/", true, false, false),
        ("GET", "workspaces/<slug>/webhooks/", true, false, false),
        (
            "PATCH",
            "workspaces/<slug>/webhooks/<uuid>/",
            true,
            false,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/webhooks/<uuid>/",
            true,
            false,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/webhooks/<uuid>/regenerate/",
            true,
            false,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/webhook-logs/<uuid>/",
            true,
            false,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/projects/<id>/ai-assistant/",
            true,
            true,
            false,
        ),
        ("POST", "workspaces/<slug>/ai-assistant/", true, true, false),
        ("GET", "unsplash/", true, true, true),
    ];

    #[test]
    fn perm_matrix_green_for_every_role() {
        for (method, path, admin, member, guest) in MATRIX {
            for (role, want) in [
                (ROLE_ADMIN, *admin),
                (ROLE_MEMBER, *member),
                (ROLE_GUEST, *guest),
            ] {
                let got = decide_path(path, method, Some(role));
                let allowed = got == GateOutcome::Allow;
                assert_eq!(
                    allowed, want,
                    "{method} {path} role={role}: got {got:?}, want allow={want}"
                );
            }
        }
    }

    #[test]
    fn anonymous_reaches_only_allow_any() {
        for row in GATES {
            let got = decide_path(row.path, row.method, None);
            match row.gate {
                Gate::AllowAny => assert_eq!(
                    got,
                    GateOutcome::Allow,
                    "{} {} anonymous must reach the handler",
                    row.method,
                    row.path
                ),
                _ => assert_eq!(
                    got,
                    GateOutcome::Unauthenticated,
                    "{} {} anonymous must not reach a gate",
                    row.method,
                    row.path
                ),
            }
        }
    }

    #[test]
    fn cross_workspace_facts_deny() {
        let row = gate_for("GET", "workspaces/<slug>/integrations/github/").unwrap();
        let mut facts = facts_for(ROLE_ADMIN, ADMIN_MEMBER);
        facts.workspace = WorkspaceId::from("other");
        assert_eq!(decide_gate(&row.gate, &scope(), &facts), GateOutcome::Deny);
    }

    #[test]
    fn workspace_admin_override_passes_project_gate() {
        // Project ADMIN-only gate, caller is a workspace admin whose project
        // role is MEMBER: `base.py:56-64` passes them anyway.
        let row = gate_for("POST", "workspaces/<slug>/projects/<id>/github/bind/").unwrap();
        let facts = AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: false,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: true,
            is_workspace_admin: true,
        };
        assert_eq!(decide_gate(&row.gate, &scope(), &facts), GateOutcome::Allow);
    }

    #[test]
    fn manual_admin_checks_match_python_bodies() {
        assert!(manual_admin_allows(true));
        assert!(!manual_admin_allows(false));
        assert_eq!(
            INSTALL_ADMIN_REQUIRED_BODY,
            r#"{"error":"You must be a workspace admin to install the GitHub App"}"#
        );
        assert_eq!(
            REFRESH_ADMIN_REQUIRED_BODY,
            r#"{"error":"You must be a workspace admin to refresh this connection"}"#
        );
        assert_eq!(
            GITHUB_DISABLED_BODY,
            r#"{"error":"GitHub integration is disabled on this instance"}"#
        );
    }
}
