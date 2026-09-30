//! D-28 permission gates (stage 5, PIDASHCONV-379).
//!
//! Ports the guard closure on all 13 D-28 routes (29 method+path rows)
//! from:
//!
//! - `apps/api/pi_dash/app/views/module/base.py` (8 gated units:
//!   `ModuleViewSet` create/list/retrieve/partial_update/destroy,
//!   `ModuleFavoriteViewSet`, `ModuleUserPropertiesEndpoint`)
//! - `apps/api/pi_dash/app/views/module/archive.py`
//!   (`ModuleArchiveUnarchiveEndpoint`, gated by class)
//! - `apps/api/pi_dash/app/views/module/issue.py` (4 gated units:
//!   `ModuleIssueViewSet` list/create_module_issues/create_issue_modules/
//!   destroy)
//! - `apps/api/pi_dash/app/urls/module.py` (13 routes)
//!
//! Fixture id: FX-MOD-04
//! (`rust-api/fixtures/app_modules/guards/permissions.golden.json`;
//! tripwires pinned by
//! `rust-api/contract-tests/app_modules/test_permissions.py`).
//!
//! Shape of the port: the module is pure, like the F-06 kernels it sits
//! on. Membership facts are caller inputs ([`AllowFacts`] for the
//! `@allow_permission` family, [`ProjectFacts`] for the permission-class
//! family); row fetching (the `WorkspaceMember`/`ProjectMember`
//! `...exists()` queries with their `workspace__slug=` / `project_id=`
//! / `is_active=True` filters) stays with the handlers, which fetch
//! through the workspace-scoped handle — the `tenant_context` half of
//! the pilot pattern (`app_issues/mod.rs` `resolve_gate`). This module
//! only maps each route to its [`Gate`] and decides through the kernel,
//! so the PROJECT-level workspace-admin override
//! (`app/permissions/base.py`: project member + workspace role exactly
//! ADMIN passes regardless of project role) and the deny-by-default
//! tenant scope come along unchanged.
//!
//! Gate order (preserved, not redesigned):
//!
//! - Decorated routes: DRF authentication (`BaseViewSet`/`BaseAPIView`:
//!   session auth + `IsAuthenticated`, `app/views/base.py:87,191`) runs
//!   before the decorator, and the decorator runs before the handler
//!   body.
//! - Class-gated viewsets (`ModuleLinkViewSet`,
//!   `ModuleArchiveUnarchiveEndpoint`, `ModuleFavoriteViewSet`) carry
//!   their `permission_classes` *instead of* the `IsAuthenticated`
//!   default. Anonymous callers still 401: DRF `permission_denied`
//!   raises `NotAuthenticated` when authenticators exist but no session
//!   authenticated, so anon on those routes answers the same ANON body.
//! - Undecorated default actions (no `@allow_permission`, no class
//!   override) run under the `IsAuthenticated` default only: any
//!   authenticated caller reaches the body (see [`Gate::Open`] and the
//!   PUT quirk below).
//!
//! Denial bodies (byte-exact, compact DRF rendering):
//!
//! - Decorator denial: `{"error":"You don't have the required permissions."}`
//!   (403, `app/permissions/base.py`); see [`FORBIDDEN_BODY`].
//! - Class denial: DRF-default `PermissionDenied` body
//!   (`{"detail":"You do not have permission to perform this action."}`,
//!   403 — neither `ProjectEntityPermission` nor
//!   `ProjectLitePermission` sets a `message`); see
//!   [`VIEWSET_FORBIDDEN_BODY`]. Use [`deny_body`] to pick per gate.
//! - Anonymous: `{"detail":"Authentication credentials were not provided."}`
//!   (401); see [`ANON_BODY`].
//! - Missing object: `{"error":"The required object does not exist."}`
//!   (404, `handle_exception`'s `ObjectDoesNotExist` branch); see
//!   [`NOT_FOUND_BODY`]. Handlers render it; recorded here so the matrix
//!   has one home.
//!
//! Per-user row scoping (verified, handler-side — not gate logic):
//! `ModuleFavoriteViewSet.get_queryset` filters
//! `user=self.request.user` (`base.py:795-802`), and the user-properties
//! endpoint reads `get_or_create(user=request.user, ...)`
//! (`base.py:846-855`). Guests pass the gate on these routes but still
//! see only their own rows; the handler port preserves those filters.
//! No `created_by` filter exists anywhere in the module views —
//! `created_by=` appears only as a write attribute on
//! `ModuleIssue` creation (`issue.py:223,263`).
//!
//! Throttles (verified, not ported): no D-28 view declares
//! `throttle_classes`, `throttle_scope`, or any `throttle` reference —
//! `grep throttle` over `app/views/module/` and `app/views/base.py` is
//! empty. Rate limiting on these routes comes only from shared
//! `BaseAPIView`/`BaseViewSet` infrastructure, which is cross-cutting
//! and outside this guard port (same split as PIDASHCONV-367).
//!
//! Ported quirks (translate, don't redesign):
//!
//! - PUT has no gate: `ModuleViewSet` defines no custom `update` and no
//!   `get_permissions` override anywhere in `base.py`, so PUT on
//!   `modules/<pk>/` falls to DRF `ModelViewSet.update` under
//!   `IsAuthenticated` only — any authenticated user (any role, any
//!   project membership) can full-update a module, bypassing both the
//!   role gate and the `archived_at` guard in `partial_update`
//!   (`:651-721`). Pinned here as [`Gate::Open`].
//! - `ModuleIssueViewSet` detail retrieve/update/partial_update
//!   (`modules/<module_id>/issues/<issue_id>/` GET/PUT/PATCH) are likewise
//!   undecorated `ModelViewSet` defaults under `IsAuthenticated` only.
//! - Favorites list 500s *after* the gate: `ModuleFavoriteViewSet` has
//!   no `serializer_class`, so DRF's default list raises
//!   `AssertionError` into the 500 handler (and the queryset raises
//!   `FieldError` on `select_related('module')` first — either way the
//!   suite pins `test_favorites_properties.py:78-84`). The
//!   `ProjectLitePermission` gate still runs first.
//! - Archive-detail GET with a `module_id` kwarg 500s *after* the gate:
//!   `get(self, request, slug, project_id, pk=None)` receives
//!   `module_id`, raising `TypeError` into the 500 handler (suite pins
//!   `test_archive.py:58-64`). The `ProjectEntityPermission` gate still
//!   runs first.
//! - Destroy's creator fast-path bypasses the role check entirely: a
//!   workspace member with `Module.objects.filter(id=pk,
//!   created_by=user).exists()` proceeds without reaching the
//!   `allowed_roles=[ADMIN]` check (`base.py:19-88`), so a MEMBER
//!   creator is ALLOWED. The suite pins only the MEMBER *non-creator*
//!   denial (`test_modules.py:144-157`). NOTE for the domain gate:
//!   the FX-MOD-04 fixture line claiming "MEMBER creator (403)" via a
//!   "fall-through role check" contradicts the traced source — there is
//!   no fall-through after `if obj: return view_func(...)` — and the
//!   F-06 kernel; this table implements the Python, not the fixture
//!   line.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::project::{decide_project_entity, decide_project_lite, ProjectFacts};
use pidash_auth::permissions::{scope_allows, ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_types::WorkspaceId;

/// Exact bytes of the `@allow_permission` 403
/// (`app/permissions/base.py`). Alias of
/// [`crate::permissions::PERMISSION_DENIED_BODY`] so handlers have one home.
pub const FORBIDDEN_BODY: &str = crate::permissions::PERMISSION_DENIED_BODY;
/// Exact bytes of the DRF-default class-denial 403 (neither
/// `ProjectEntityPermission` nor `ProjectLitePermission` sets a
/// `message`). Alias of [`crate::permissions::DEFAULT_DENIED_BODY`].
pub const VIEWSET_FORBIDDEN_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;
/// Exact bytes of the DRF `IsAuthenticated` / `NotAuthenticated` 401.
pub const ANON_BODY: &str = r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (404), rendered by
/// handlers when a gated lookup misses.
pub const NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;

/// How one D-28 route+method authorizes, before any handler logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `@allow_permission([...])` at the default `"PROJECT"` level: active
    /// project membership with a listed role, or the workspace-admin
    /// override (`app/permissions/base.py`). Denies with [`FORBIDDEN_BODY`].
    Project { roles: &'static [i32] },
    /// `@allow_permission([ADMIN], creator=True, model=Module)`
    /// (`base.py:723`): workspace members who created the module pass on
    /// the fast path; everyone else falls through to the ADMIN role check
    /// (so MEMBER non-creators still deny). Denies with [`FORBIDDEN_BODY`].
    ProjectCreator { roles: &'static [i32] },
    /// `permission_classes = [ProjectEntityPermission]`
    /// (`base.py:763`, `archive.py:43`): safe methods need any active
    /// project membership; writes need project ADMIN/MEMBER. Denies with
    /// [`VIEWSET_FORBIDDEN_BODY`], not the decorator body.
    Entity,
    /// `permission_classes = [ProjectLitePermission]` (`base.py:793`):
    /// any active project membership, every method. Denies with
    /// [`VIEWSET_FORBIDDEN_BODY`].
    Lite,
    /// No gate beyond the `IsAuthenticated` default (undecorated
    /// `ModelViewSet` defaults: module PUT, module-issue detail
    /// GET/PUT/PATCH). Any authenticated caller passes; anonymous
    /// answers [`ANON_BODY`]. There is no 403 body for this gate.
    Open,
}

/// One row of the FX-MOD-04 matrix: a route+method and its gate.
pub struct RouteGate {
    pub method: &'static str,
    pub path: &'static str,
    pub gate: Gate,
    /// Python source of the gate for this row.
    pub source: &'static str,
}

const ADMIN_MEMBER: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER];
const ADMIN_MEMBER_GUEST: &[i32] = &[ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];
const ADMIN_ONLY: &[i32] = &[ROLE_ADMIN];

/// All 13 D-28 routes, one entry per method, in URL-file order.
pub static GATES: &[RouteGate] = &[
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/modules/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:353 (ModuleViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/modules/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:294 (ModuleViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:395 (ModuleViewSet.retrieve)",
    },
    // PUT quirk: no custom update, IsAuthenticated only.
    RouteGate {
        method: "PUT",
        path: "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
        gate: Gate::Open,
        source: "base.py (no update method; ModelViewSet.update default)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:651 (ModuleViewSet.partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
        gate: Gate::ProjectCreator { roles: ADMIN_ONLY },
        source: "base.py:723 (ModuleViewSet.destroy, creator=True)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/issues/<issue_id>/modules/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "issue.py:248 (ModuleIssueViewSet.create_issue_modules)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "issue.py:95 (ModuleIssueViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "issue.py:209 (ModuleIssueViewSet.create_module_issues)",
    },
    // Undecorated ModelViewSet defaults on the module-issue detail route.
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
        gate: Gate::Open,
        source: "issue.py (no retrieve override; ModelViewSet default)",
    },
    RouteGate {
        method: "PUT",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
        gate: Gate::Open,
        source: "issue.py (no update override; ModelViewSet default)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
        gate: Gate::Open,
        source: "issue.py (no partial_update override; ModelViewSet default)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "issue.py:317 (ModuleIssueViewSet.destroy)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/",
        gate: Gate::Entity,
        source: "base.py:763 (ModuleLinkViewSet list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/",
        gate: Gate::Entity,
        source: "base.py:763 (ModuleLinkViewSet create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
        gate: Gate::Entity,
        source: "base.py:763 (ModuleLinkViewSet retrieve)",
    },
    RouteGate {
        method: "PUT",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
        gate: Gate::Entity,
        source: "base.py:763 (ModuleLinkViewSet update)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
        gate: Gate::Entity,
        source: "base.py:763 (ModuleLinkViewSet partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
        gate: Gate::Entity,
        source: "base.py:763 (ModuleLinkViewSet destroy)",
    },
    // Favorites list 500s after the gate (no serializer_class); the gate
    // still runs first.
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/user-favorite-modules/",
        gate: Gate::Lite,
        source: "base.py:793 (ModuleFavoriteViewSet list; 500 after gate)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/user-favorite-modules/",
        gate: Gate::Lite,
        source: "base.py:793 (ModuleFavoriteViewSet create)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/user-favorite-modules/<module_id>/",
        gate: Gate::Lite,
        source: "base.py:793 (ModuleFavoriteViewSet destroy)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/user-properties/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:826 (ModuleUserPropertiesEndpoint.patch)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/user-properties/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:846 (ModuleUserPropertiesEndpoint.get)",
    },
    // Archive-detail GET with a module_id kwarg 500s after the gate
    // (TypeError on the pk=None signature); the gate still runs first.
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
        gate: Gate::Entity,
        source: "archive.py:43 (ModuleArchiveUnarchiveEndpoint.get; 500 after gate)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
        gate: Gate::Entity,
        source: "archive.py:544 (ModuleArchiveUnarchiveEndpoint.post)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
        gate: Gate::Entity,
        source: "archive.py:561 (ModuleArchiveUnarchiveEndpoint.delete)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/archived-modules/",
        gate: Gate::Entity,
        source: "archive.py:258 (ModuleArchiveUnarchiveEndpoint.get collection)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/archived-modules/<pk>/",
        gate: Gate::Entity,
        source: "archive.py:294 (ModuleArchiveUnarchiveEndpoint.get detail)",
    },
];

/// Outcome of a gate check, before denial rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The handler body runs.
    Allow,
    /// The handler body does not run: answer 403 with [`deny_body`].
    Deny,
    /// Anonymous on a D-28 route: never reaches a gate check; DRF
    /// `IsAuthenticated` (or `permission_denied` → `NotAuthenticated` on
    /// the class-gated viewsets) denies first with the 401 [`ANON_BODY`].
    Unauthenticated,
}

/// Which 403 body a denied gate renders: the decorator body for the
/// `@allow_permission` family, the DRF-default body for the
/// permission-class family. [`Gate::Open`] never 403s — its only denial
/// is the 401 [`ANON_BODY`].
pub fn deny_body(gate: &Gate) -> &'static str {
    match gate {
        Gate::Project { .. } | Gate::ProjectCreator { .. } => FORBIDDEN_BODY,
        Gate::Entity | Gate::Lite => VIEWSET_FORBIDDEN_BODY,
        Gate::Open => ANON_BODY,
    }
}

fn spec_for(gate: &Gate) -> AllowSpec {
    let creator_bypass = matches!(gate, Gate::ProjectCreator { .. });
    AllowSpec {
        level: AllowLevel::Project,
        // D-28 views import from `pi_dash.app.permissions` (the `app`
        // copy: non-workspace-members are refused before the creator
        // fast-path; `base.py:20`, `issue.py:20`, `archive.py:31`).
        creator_gate: CreatorGate::App,
        creator_bypass,
    }
}

/// Decide an `@allow_permission`-family or open gate from pre-fetched
/// membership facts.
///
/// `facts` mirrors one `(user, slug, project_id)` row set: the
/// active-row filters (`is_active=True`) and the `workspace__slug=` /
/// `project_id=` scoping are the caller's SQL (the `tenant_context`
/// half); the scope check denies facts fetched for a different
/// workspace. The caller sets `is_creator` from
/// `Module.objects.filter(id=pk, created_by=user).exists()` on the
/// destroy row only. [`Gate::Entity`] / [`Gate::Lite`] use
/// [`decide_class_gate`] instead — passing them here panics so a
/// miswired handler fails loudly in tests rather than shipping the
/// wrong kernel.
pub fn decide_gate(gate: &Gate, scope: &TenantScope, facts: &AllowFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    match gate {
        Gate::Project { .. } | Gate::ProjectCreator { .. } => {
            if decide_allow(&spec_for(gate), scope, facts) {
                GateOutcome::Allow
            } else {
                GateOutcome::Deny
            }
        }
        // Any authenticated caller passes; anonymous already returned.
        Gate::Open => {
            if scope_allows_scope(scope, facts) {
                GateOutcome::Allow
            } else {
                GateOutcome::Deny
            }
        }
        Gate::Entity | Gate::Lite => {
            panic!("class gate needs ProjectFacts: use decide_class_gate")
        }
    }
}

/// Decide a permission-class gate from pre-fetched membership facts.
///
/// D-28 module views never set `project_identifier`, so callers leave
/// `has_project_identifier` false and the identifier branch stays dead —
/// exactly like the Python, where `hasattr(view, "project_identifier")`
/// is false on these views. Passing a [`Gate::Project`],
/// [`Gate::ProjectCreator`], or [`Gate::Open`] gate here panics for the
/// same reason as above.
pub fn decide_class_gate(
    gate: &Gate,
    method: &str,
    scope: &TenantScope,
    facts: &ProjectFacts,
) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    let allowed = match gate {
        Gate::Entity => decide_project_entity(method, scope, facts),
        Gate::Lite => decide_project_lite(scope, facts),
        Gate::Project { .. } | Gate::ProjectCreator { .. } | Gate::Open => {
            panic!("allow/open gate needs AllowFacts: use decide_gate")
        }
    };
    if allowed {
        GateOutcome::Allow
    } else {
        GateOutcome::Deny
    }
}

/// Look up the gate for one route+method; `None` is not a D-28 route.
pub fn gate_for(method: &str, path: &str) -> Option<&'static RouteGate> {
    GATES
        .iter()
        .find(|row| row.method == method && row.path == path)
}

/// Tenant context for one request: the workspace the URL names. Handlers
/// build membership facts only for this slug, so [`decide_gate`] and
/// [`decide_class_gate`] deny cross-workspace facts even when the rows
/// exist.
pub fn tenant_context(slug: &str) -> TenantScope {
    TenantScope::new(WorkspaceId::from(slug))
}

/// Scope check for [`Gate::Open`]: the only scoping an
/// `IsAuthenticated`-only route has is the tenant the URL names.
/// Anonymous callers never reach here ([`decide_gate`] returns
/// [`GateOutcome::Unauthenticated`] first).
fn scope_allows_scope(scope: &TenantScope, facts: &AllowFacts) -> bool {
    scope_allows(scope, &facts.workspace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::ProjectId;

    fn scope() -> TenantScope {
        tenant_context("acme")
    }

    /// Facts for an authenticated caller holding `role` in both the
    /// workspace and the project (the contract-suite world: admin, member
    /// and guest are members of both), for an allow-gate with the given
    /// allowed-role list.
    fn allow_facts_for(role: i32, allowed: &[i32], is_creator: bool) -> AllowFacts {
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: allowed.contains(&role),
            is_creator,
            has_allowed_project_role: allowed.contains(&role),
            is_project_member: true,
            is_workspace_admin: role == ROLE_ADMIN,
        }
    }

    /// Facts for an authenticated caller holding `role` as their project
    /// role (workspace membership mirrors it, as in the contract suite).
    fn class_facts_for(role: i32) -> ProjectFacts {
        ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated: true,
            is_workspace_member: true,
            has_workspace_admin_or_member: role == ROLE_ADMIN || role == ROLE_MEMBER,
            is_workspace_admin: role == ROLE_ADMIN,
            is_project_member: true,
            is_project_admin: role == ROLE_ADMIN,
            has_project_admin_or_member: role == ROLE_ADMIN || role == ROLE_MEMBER,
            has_identifier_membership: false,
            has_project_identifier: false,
        }
    }

    fn anon_allow_facts() -> AllowFacts {
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

    fn anon_class_facts() -> ProjectFacts {
        ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated: false,
            is_workspace_member: false,
            has_workspace_admin_or_member: false,
            is_workspace_admin: false,
            is_project_member: false,
            is_project_admin: false,
            has_project_admin_or_member: false,
            has_identifier_membership: false,
            has_project_identifier: false,
        }
    }

    fn decide_allow_path(method: &str, path: &str, role: Option<(i32, bool)>) -> GateOutcome {
        let row = gate_for(method, path).expect("fixture route must have a gate");
        let facts = match role {
            None => anon_allow_facts(),
            Some((role, is_creator)) => match row.gate {
                Gate::Project { roles } | Gate::ProjectCreator { roles } => {
                    allow_facts_for(role, roles, is_creator)
                }
                Gate::Open => allow_facts_for(role, &[], false),
                Gate::Entity | Gate::Lite => {
                    panic!("class gate row in allow-path test: {method} {path}")
                }
            },
        };
        decide_gate(&row.gate, &scope(), &facts)
    }

    fn decide_class_path(method: &str, path: &str, role: Option<i32>) -> GateOutcome {
        let row = gate_for(method, path).expect("fixture route must have a gate");
        let facts = match role {
            None => anon_class_facts(),
            Some(role) => class_facts_for(role),
        };
        decide_class_gate(&row.gate, method, &scope(), &facts)
    }

    #[test]
    fn table_covers_every_fixture_row() {
        assert_eq!(GATES.len(), 29, "13 routes, one row per method+path");
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

    /// Full FX-MOD-04 allow-family matrix: (method, path, admin, member,
    /// guest) with `true` = handler runs. Destroy takes the creator
    /// flag separately (see `creator_fast_path_and_fall_through`).
    const ALLOW_MATRIX: &[(&str, &str, bool, bool, bool)] = &[
        (
            "POST",
            "workspaces/<slug>/projects/<project_id>/modules/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/modules/",
            true,
            true,
            true,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
            true,
            true,
            false,
        ),
        (
            "PATCH",
            "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
            true,
            true,
            false,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/",
            true,
            true,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/",
            true,
            true,
            false,
        ),
        (
            "POST",
            "workspaces/<slug>/projects/<project_id>/issues/<issue_id>/modules/",
            true,
            true,
            false,
        ),
        (
            "DELETE",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
            true,
            true,
            false,
        ),
        (
            "PATCH",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/user-properties/",
            true,
            true,
            true,
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/user-properties/",
            true,
            true,
            true,
        ),
    ];

    #[test]
    fn allow_matrix_matches_django_allow_deny() {
        for (method, path, admin, member, guest) in ALLOW_MATRIX {
            for (role, role_name, expected) in [
                (ROLE_ADMIN, "ADMIN", *admin),
                (ROLE_MEMBER, "MEMBER", *member),
                (ROLE_GUEST, "GUEST", *guest),
            ] {
                let outcome = decide_allow_path(method, path, Some((role, false)));
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
    fn creator_fast_path_and_fall_through() {
        let (method, path) = (
            "DELETE",
            "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
        );
        // ADMIN creator: fast-path allow.
        assert_eq!(
            decide_allow_path(method, path, Some((ROLE_ADMIN, true))),
            GateOutcome::Allow
        );
        // ADMIN non-creator: role check allows.
        assert_eq!(
            decide_allow_path(method, path, Some((ROLE_ADMIN, false))),
            GateOutcome::Allow
        );
        // MEMBER non-creator: role check denies.
        assert_eq!(
            decide_allow_path(method, path, Some((ROLE_MEMBER, false))),
            GateOutcome::Deny
        );
        // MEMBER creator: the fast-path returns the view before the
        // role check, so the creator is ALLOWED (`base.py:19-88`; the
        // suite pins only the non-creator denial). The FX-MOD-04 line
        // claiming otherwise contradicts the source — see module docs.
        assert_eq!(
            decide_allow_path(method, path, Some((ROLE_MEMBER, true))),
            GateOutcome::Allow
        );
        // GUEST denies either way.
        assert_eq!(
            decide_allow_path(method, path, Some((ROLE_GUEST, false))),
            GateOutcome::Deny
        );
        // Non-workspace-member creator is refused before the fast-path
        // (the `app` copy's membership gate).
        let row = gate_for(method, path).expect("destroy row");
        let mut facts = allow_facts_for(ROLE_ADMIN, ADMIN_ONLY, true);
        facts.is_workspace_member = false;
        assert_eq!(decide_gate(&row.gate, &scope(), &facts), GateOutcome::Deny);
    }

    /// Entity family: safe methods allow any project member (incl.
    /// GUEST); writes need ADMIN/MEMBER.
    const ENTITY_ROWS: &[(&str, &str)] = &[
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/",
        ),
        (
            "POST",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/",
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
        ),
        (
            "PUT",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
        ),
        (
            "PATCH",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
        ),
        (
            "DELETE",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/archived-modules/",
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/archived-modules/<pk>/",
        ),
        (
            "POST",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
        ),
        (
            "DELETE",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
        ),
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/modules/<module_id>/archive/",
        ),
    ];

    #[test]
    fn entity_matrix_matches_django_allow_deny() {
        for (method, path) in ENTITY_ROWS {
            let safe = matches!(*method, "GET" | "HEAD" | "OPTIONS");
            for (role, role_name) in [
                (ROLE_ADMIN, "ADMIN"),
                (ROLE_MEMBER, "MEMBER"),
                (ROLE_GUEST, "GUEST"),
            ] {
                let outcome = decide_class_path(method, path, Some(role));
                let expected = if safe || role != ROLE_GUEST {
                    GateOutcome::Allow
                } else {
                    GateOutcome::Deny
                };
                assert_eq!(outcome, expected, "{method} {path} role {role_name}");
            }
            // Non-member denies on every method (`.exists() == False`).
            let mut outsider = class_facts_for(ROLE_MEMBER);
            outsider.is_project_member = false;
            outsider.has_project_admin_or_member = false;
            outsider.is_project_admin = false;
            let row = gate_for(method, path).expect("entity row");
            assert_eq!(
                decide_class_gate(&row.gate, method, &scope(), &outsider),
                GateOutcome::Deny,
                "non-member {method} {path}"
            );
        }
    }

    /// Lite family: any active project membership allows, every method.
    const LITE_ROWS: &[(&str, &str)] = &[
        (
            "GET",
            "workspaces/<slug>/projects/<project_id>/user-favorite-modules/",
        ),
        (
            "POST",
            "workspaces/<slug>/projects/<project_id>/user-favorite-modules/",
        ),
        (
            "DELETE",
            "workspaces/<slug>/projects/<project_id>/user-favorite-modules/<module_id>/",
        ),
    ];

    #[test]
    fn lite_matrix_matches_django_allow_deny() {
        for (method, path) in LITE_ROWS {
            for (role, role_name) in [
                (ROLE_ADMIN, "ADMIN"),
                (ROLE_MEMBER, "MEMBER"),
                (ROLE_GUEST, "GUEST"),
            ] {
                assert_eq!(
                    decide_class_path(method, path, Some(role)),
                    GateOutcome::Allow,
                    "{method} {path} role {role_name}"
                );
            }
            let mut outsider = class_facts_for(ROLE_ADMIN);
            outsider.is_project_member = false;
            let row = gate_for(method, path).expect("lite row");
            assert_eq!(
                decide_class_gate(&row.gate, method, &scope(), &outsider),
                GateOutcome::Deny,
                "non-member {method} {path}"
            );
        }
    }

    #[test]
    fn open_gates_allow_any_authenticated_caller() {
        // The PUT quirk and the undecorated module-issue detail defaults:
        // even a GUEST with no project membership reaches the body.
        let rows = [
            (
                "PUT",
                "workspaces/<slug>/projects/<project_id>/modules/<pk>/",
            ),
            (
                "GET",
                "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
            ),
            (
                "PUT",
                "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
            ),
            (
                "PATCH",
                "workspaces/<slug>/projects/<project_id>/modules/<module_id>/issues/<issue_id>/",
            ),
        ];
        for (method, path) in rows {
            let row = gate_for(method, path).expect("open row");
            assert_eq!(row.gate, Gate::Open, "{method} {path}");
            for role in [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST] {
                assert_eq!(
                    decide_allow_path(method, path, Some((role, false))),
                    GateOutcome::Allow,
                    "{method} {path} role {role}"
                );
            }
            // Same-workspace membership rows are irrelevant to Open, but
            // cross-workspace facts still deny (tenant context).
            let mut outsider = allow_facts_for(ROLE_GUEST, &[], false);
            outsider.workspace = WorkspaceId::from("other");
            assert_eq!(
                decide_gate(&row.gate, &scope(), &outsider),
                GateOutcome::Deny,
                "cross-workspace {method} {path}"
            );
        }
    }

    #[test]
    fn anonymous_never_reaches_a_gate() {
        // Every D-28 route answers 401 ANON before any gate runs
        // (IsAuthenticated default; permission_denied → NotAuthenticated
        // on the class-gated viewsets).
        for row in GATES {
            let outcome = match row.gate {
                Gate::Project { .. } | Gate::ProjectCreator { .. } | Gate::Open => {
                    decide_gate(&row.gate, &scope(), &anon_allow_facts())
                }
                Gate::Entity | Gate::Lite => {
                    decide_class_gate(&row.gate, row.method, &scope(), &anon_class_facts())
                }
            };
            assert_eq!(
                outcome,
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
        // Decorator gates render the allow-style body, class gates the
        // DRF-default body, open gates only the 401.
        for row in GATES {
            let expected = match row.gate {
                Gate::Project { .. } | Gate::ProjectCreator { .. } => FORBIDDEN_BODY,
                Gate::Entity | Gate::Lite => VIEWSET_FORBIDDEN_BODY,
                Gate::Open => ANON_BODY,
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
        let other_allow = AllowFacts {
            workspace: WorkspaceId::from("other"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            is_creator: true,
            has_allowed_project_role: true,
            is_project_member: true,
            is_workspace_admin: true,
        };
        for gate in [
            Gate::Project {
                roles: ADMIN_MEMBER,
            },
            Gate::Project {
                roles: ADMIN_MEMBER_GUEST,
            },
            Gate::ProjectCreator { roles: ADMIN_ONLY },
            Gate::Open,
        ] {
            assert_eq!(
                decide_gate(&gate, &scope(), &other_allow),
                GateOutcome::Deny,
                "{gate:?}"
            );
        }
        let other_class = ProjectFacts {
            workspace: WorkspaceId::from("other"),
            project_id: ProjectId::from("p-1"),
            authenticated: true,
            is_workspace_member: true,
            has_workspace_admin_or_member: true,
            is_workspace_admin: true,
            is_project_member: true,
            is_project_admin: true,
            has_project_admin_or_member: true,
            has_identifier_membership: true,
            has_project_identifier: false,
        };
        for (gate, method) in [
            (Gate::Entity, "GET"),
            (Gate::Entity, "POST"),
            (Gate::Lite, "DELETE"),
        ] {
            assert_eq!(
                decide_class_gate(&gate, method, &scope(), &other_class),
                GateOutcome::Deny,
                "{gate:?} {method}"
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
        // ... while a workspace admin who IS a project member passes
        // regardless of project role (the override).
        let mut both = ws_admin_no_project;
        both.is_project_member = true;
        assert_eq!(decide_gate(&gate, &scope(), &both), GateOutcome::Allow);
        // The entity kernel has no such override: workspace admin without
        // project membership denies even reads.
        let mut class_outsider = class_facts_for(ROLE_ADMIN);
        class_outsider.is_project_member = false;
        class_outsider.has_project_admin_or_member = false;
        class_outsider.is_project_admin = false;
        assert_eq!(
            decide_class_gate(&Gate::Entity, "GET", &scope(), &class_outsider),
            GateOutcome::Deny
        );
    }
}
