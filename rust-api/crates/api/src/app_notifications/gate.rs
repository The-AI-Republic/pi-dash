//! D-34 notification permission gates (stage 5, PIDASHCONV-300).
//!
//! Ports the `@allow_permission` gates on all 7 D-34 routes (12
//! method+path rows: the uniform workspace gate, the two undecorated
//! preference endpoints, and the two undecorated retrieve/destroy
//! actions) in:
//!
//! - `apps/api/pi_dash/app/views/notification/base.py:47` (list)
//! - `base.py:151` (partial_update), `:163` (mark_read), `:171`
//!   (mark_unread), `:179` (archive), `:187` (unarchive)
//! - `base.py:199` (`UnreadNotificationEndpoint.get`)
//! - `base.py:233` (`MarkAllReadNotificationViewSet.create`)
//! - `base.py:291-308` (`UserNotificationPreferenceEndpoint` get/patch,
//!   no decorator — authenticated session only)
//!
//! Every decorator line reads
//! `@allow_permission(allowed_roles=[ROLE.ADMIN, ROLE.MEMBER, ROLE.GUEST],
//! level="WORKSPACE")`, so the whole domain shares one gate shape. The
//! module is pure, like the [`pidash_auth::permissions::allow`] kernel it
//! sits on and like the D-33 [`crate::app_integrations::gates`] table it
//! mirrors. Membership facts are caller inputs ([`AllowFacts`]); row
//! fetching (the `WorkspaceMember ...exists()` query with its
//! `workspace__slug=`, `role__in=[20, 15, 5]`, `is_active=True` filters
//! from `app/permissions/base.py:44-51`) stays with the handlers, which
//! fetch through the workspace-scoped handle — the `tenant_context` half
//! of the pilot pattern. This module only maps each route to its
//! [`Gate`] and decides through the kernel. D-34 never sets
//! `creator`/`model`, so the creator bypass is always off.
//!
//! Gate order (preserved, not redesigned):
//!
//! - Decorated routes: DRF authentication (`BaseAPIView`: session auth +
//!   `IsAuthenticated`, `views/base.py`) runs before the decorator, and
//!   the decorator runs before the handler body.
//! - Preference endpoints (`users/me/notification-preferences/`, no
//!   decorator): `IsAuthenticated` is the only check — any logged-in
//!   user reaches the handler, including non-members of every
//!   workspace. There is no workspace slug on these paths, so no
//!   workspace role check can run; same as Python.
//! - retrieve/destroy carry no decorator either (inherited `BaseViewSet`
//!   mixins, no local lines): `IsAuthenticated` runs, then the
//!   receiver-scoped `get_queryset` answers 404 for foreign rows. The
//!   contract suite pins this (`test_access.py`:
//!   outsiders see 404, never 403) — handlers must NOT apply the
//!   workspace role gate here. [`Gate::QuerysetScoped`] marks those rows.
//!
//! Denial bodies (byte-exact, compact DRF rendering):
//!
//! - Decorator denial: `{"error":"You don't have the required
//!   permissions."}` (403, `app/permissions/base.py:80-84`); see
//!   [`crate::permissions::PERMISSION_DENIED_BODY`], reused here by
//!   reference, not redefined.
//! - Anonymous callers never reach a gate: `IsAuthenticated` denies
//!   first with [`UNAUTHENTICATED_BODY`] (401).
//!
//! Observed, not ported: unlike issues (`app_issues::Gate::guest_scoped`),
//! this domain has NO guest `created_by` scoping. Guests read exactly
//! what members read; per-user isolation is `receiver_id = request.user`
//! in the queryset (`base.py:36-45`, queries layer), never a
//! creator-bypass branch in the gate.
//!
//! Ported bugs: none in this unit. The over-broad `snoozed=true` filter
//! branch lives in the queries layer and was ported verbatim there
//! (PIDASHCONV-299); the gate has no filter logic to carry it.
//!
//! Goldens: `rust-api/contract-tests/app_notifications/test_access.py`
//! (DENIED 403 on the 8 decorated handlers, 404 on retrieve/destroy,
//! 401 anonymous, cross-workspace 403/404) and `test_preferences.py`
//! (session-only access, 401 anonymous). The `#[cfg(test)]` matrix below
//! replays the same decisions against [`decide_gate`].

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_types::WorkspaceId;

/// Exact bytes of the DRF `IsAuthenticated` denial (401): what anonymous
/// callers on every D-34 route receive before any gate runs. Pinned by
/// `test_access.py::test_unauthenticated_denied` and
/// `test_preferences.py::test_preferences_unauthenticated`.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;

/// How one D-34 route+method authorizes, before any handler logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `@allow_permission([ADMIN, MEMBER, GUEST], level="WORKSPACE")`:
    /// active workspace membership with a listed role
    /// (`app/permissions/base.py:44-51`). All 8 decorated D-34 handlers
    /// share this shape.
    Workspace { roles: &'static [i32] },
    /// No decorator and no workspace slug
    /// (`UserNotificationPreferenceEndpoint`, `base.py:291-308`): any
    /// authenticated session reaches the handler body.
    Authenticated,
    /// No decorator but workspace-scoped (`retrieve`/`destroy` inherited
    /// mixins): any authenticated session reaches the handler body;
    /// row visibility comes from the receiver-scoped queryset (404 for
    /// foreign rows), never from a role check.
    QuerysetScoped,
}

/// One row of the gate table: a route+method and its gate.
pub struct RouteGate {
    pub method: &'static str,
    pub path: &'static str,
    pub gate: Gate,
    /// Python source of the gate for this row.
    pub source: &'static str,
}

const ADMIN_MEMBER_GUEST: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];

/// All 7 D-34 routes, one entry per method, in URL order
/// (`apps/api/pi_dash/app/urls/notification.py`).
pub static GATES: &[RouteGate] = &[
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/users/notifications/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:47 (list)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/users/notifications/<uuid>/",
        gate: Gate::QuerysetScoped,
        source: "inherited retrieve mixin (no decorator; receiver-scoped 404)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/users/notifications/<uuid>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:151 (partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/users/notifications/<uuid>/",
        gate: Gate::QuerysetScoped,
        source: "inherited destroy mixin (no decorator; receiver-scoped 404)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/users/notifications/<uuid>/read/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:163 (mark_read)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/users/notifications/<uuid>/read/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:171 (mark_unread)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/users/notifications/<uuid>/archive/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:179 (archive)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/users/notifications/<uuid>/archive/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:187 (unarchive)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/users/notifications/unread/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:199 (UnreadNotificationEndpoint.get)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/users/notifications/mark-all-read/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:233 (MarkAllReadNotificationViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/notification-preferences/",
        gate: Gate::Authenticated,
        source: "base.py:291 (preference get; no decorator)",
    },
    RouteGate {
        method: "PATCH",
        path: "users/me/notification-preferences/",
        gate: Gate::Authenticated,
        source: "base.py:296 (preference patch; no decorator)",
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
    /// Anonymous: never reaches a gate; DRF `IsAuthenticated` denies
    /// first with [`UNAUTHENTICATED_BODY`] (401).
    Unauthenticated,
}

/// Decide one gate from pre-fetched membership facts.
///
/// `facts` mirrors one `(user, slug)` row set: the active-row filters
/// (`is_active=True`) and the `workspace__slug=` scoping are the
/// caller's SQL (the `tenant_context` half); the scope check denies
/// facts fetched for a different workspace. D-34 never sets
/// `creator`/`model`, so the creator bypass is always off, and there is
/// no guest `created_by` scoping (see the module docs).
pub fn decide_gate(gate: &Gate, scope: &TenantScope, facts: &AllowFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    match gate {
        Gate::Authenticated | Gate::QuerysetScoped => GateOutcome::Allow,
        Gate::Workspace { .. } => {
            let spec = AllowSpec {
                level: AllowLevel::Workspace,
                creator_gate: CreatorGate::App,
                creator_bypass: false,
            };
            if decide_allow(&spec, scope, facts) {
                GateOutcome::Allow
            } else {
                GateOutcome::Deny
            }
        }
    }
}

/// Look up the gate for one route+method; `None` is not a D-34 route.
pub fn gate_for(method: &str, path: &str) -> Option<&'static RouteGate> {
    GATES
        .iter()
        .find(|row| row.method == method && row.path == path)
}

/// Tenant context for one request: the workspace the URL names (or, for
/// the slug-less preference paths, the workspace the caller's facts were
/// fetched for — preference rows carry no gate, so the scope only needs
/// to match the facts). Handlers build membership facts only for this
/// slug, so [`decide_gate`] denies cross-workspace facts even when the
/// rows exist.
pub fn tenant_context(slug: &str) -> TenantScope {
    TenantScope::new(WorkspaceId::from(slug))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> TenantScope {
        tenant_context("acme")
    }

    fn anon_facts() -> AllowFacts {
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: false,
            is_workspace_member: false,
            has_allowed_workspace_role: false,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        }
    }

    /// Facts for an authenticated caller whose workspace role is `role`
    /// (active row in the request workspace), against a gate allowing
    /// `allowed`. `member=false` models the contract suite's outsider:
    /// authenticated but a member of no workspace in play.
    fn authed_facts(role: Option<i32>, member: bool) -> AllowFacts {
        let allowed = ADMIN_MEMBER_GUEST;
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: member,
            has_allowed_workspace_role: role
                .map(|r| member && allowed.contains(&r))
                .unwrap_or(false),
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: role == Some(ROLE_ADMIN) && member,
        }
    }

    /// The contract-suite matrix: (method, path, admin, member, guest,
    /// outsider) with `true` = handler runs. Anonymous denies before any
    /// gate (401), so it is asserted separately.
    const MATRIX: &[(&str, &str, bool, bool, bool, bool)] = &[
        (
            "GET",
            "workspaces/<slug>/users/notifications/",
            true,
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/users/notifications/<uuid>/",
            true,
            true,
            true,
            true,
        ),
        (
            "PATCH",
            "workspaces/<slug>/users/notifications/<uuid>/",
            true,
            true,
            true,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/users/notifications/<uuid>/",
            true,
            true,
            true,
            true,
        ),
        (
            "POST",
            "workspaces/<slug>/users/notifications/<uuid>/read/",
            true,
            true,
            true,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/users/notifications/<uuid>/read/",
            true,
            true,
            true,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/users/notifications/<uuid>/archive/",
            true,
            true,
            true,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/users/notifications/<uuid>/archive/",
            true,
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/users/notifications/unread/",
            true,
            true,
            true,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/users/notifications/mark-all-read/",
            true,
            true,
            true,
            false,
        ),
        (
            "GET",
            "users/me/notification-preferences/",
            true,
            true,
            true,
            true,
        ),
        (
            "PATCH",
            "users/me/notification-preferences/",
            true,
            true,
            true,
            true,
        ),
    ];

    fn decide_path(method: &str, path: &str, caller: Option<(Option<i32>, bool)>) -> GateOutcome {
        let row = gate_for(method, path).expect("gate row must exist");
        let facts = match caller {
            None => anon_facts(),
            Some((role, member)) => authed_facts(role, member),
        };
        decide_gate(&row.gate, &scope(), &facts)
    }

    #[test]
    fn table_covers_all_seven_url_routes() {
        assert_eq!(
            GATES.len(),
            MATRIX.len(),
            "12 method+path rows over 7 URL routes"
        );
        for (method, path, _, _, _, _) in MATRIX {
            assert!(
                gate_for(method, path).is_some(),
                "{method} {path} has a gate"
            );
        }
        assert!(gate_for("GET", "nope/").is_none());
    }

    #[test]
    fn contract_matrix_admin_member_guest_outsider() {
        for (method, path, admin, member, guest, outsider) in MATRIX {
            let want = |allow: bool| {
                if allow {
                    GateOutcome::Allow
                } else {
                    GateOutcome::Deny
                }
            };
            assert_eq!(
                decide_path(method, path, Some((Some(ROLE_ADMIN), true))),
                want(*admin),
                "{method} {path} admin"
            );
            assert_eq!(
                decide_path(method, path, Some((Some(ROLE_MEMBER), true))),
                want(*member),
                "{method} {path} member"
            );
            assert_eq!(
                decide_path(method, path, Some((Some(ROLE_GUEST), true))),
                want(*guest),
                "{method} {path} guest"
            );
            assert_eq!(
                decide_path(method, path, Some((None, false))),
                want(*outsider),
                "{method} {path} outsider"
            );
        }
    }

    #[test]
    fn anonymous_never_reaches_a_gate() {
        for row in GATES {
            assert_eq!(
                decide_path(row.method, row.path, None),
                GateOutcome::Unauthenticated,
                "{} {} anonymous",
                row.method,
                row.path
            );
        }
    }

    #[test]
    fn cross_workspace_facts_deny() {
        let row = gate_for("GET", "workspaces/<slug>/users/notifications/").expect("list row");
        let facts = AllowFacts {
            workspace: WorkspaceId::from("other"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        };
        assert_eq!(decide_gate(&row.gate, &scope(), &facts), GateOutcome::Deny);
    }

    #[test]
    fn denial_bodies_match_contract_goldens() {
        // `test_access.py::DENIED` and the `test_preferences.py` 401s.
        assert_eq!(
            crate::permissions::PERMISSION_DENIED_BODY,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
    }
}
