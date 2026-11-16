//! Read-only v1 scheduler permission guards (stage 8 drift, PIDASHCONV-1061).
//!
//! Ports the `permission_classes` lines of
//! `apps/api/pi_dash/api/views/scheduler.py` (04920ea3) for the api layer:
//!
//! * workspace scheduler list → `WorkspaceEntityPermission`
//!   (`app/permissions/workspace.py:74-85`, SAFE-methods branch)
//! * project scheduler list / detail / runs → `ProjectEntityPermission`
//!   (`app/permissions/project.py:85-116`, SAFE-methods branch; the views
//!   carry no `project_identifier`, so the identifier branch is dead)
//!
//! Shape of the port (the `v1_projects::perms` precedent, PIDASHCONV-367):
//! the decision kernel lives in the read-only F-06 foundation
//! (`pidash_auth::permissions::{project, workspace}`); this module only pins
//! which gate each route carries ([`gate_for`]) and re-exports the single
//! deny body the handlers answer ([`CLASS_DENIAL_BODY`]). Row fetching stays
//! with the handler layer, which must honor the fetch-scoping traps
//! documented on [`decide`].
//!
//! Gate order (preserved, not redesigned): DRF `initial()` runs API-key
//! authentication (`api/views/base.py:101`), then the slug→UUID rewrite
//! (`base.py:52-104`, skipped for anonymous callers so slugs cannot be
//! probed via 404-vs-401), then `check_permissions`, then the handler body.
//! Anonymous callers therefore 401 on every route here and never reach a
//! gate.
//!
//! Response shapes (handlers render these; recorded here so the matrix has
//! one home):
//!
//! * Anonymous on any route: 401 (missing credentials) — the auth layer
//!   answers before any gate runs. An invalid token answers 403
//!   `{"detail":"Given API token is not valid"}`.
//! * Denied member: 403 [`CLASS_DENIAL_BODY`] — the DRF-default
//!   `PermissionDenied` body. Neither class sets `message`, so every class
//!   denial renders through `APIView.permission_denied` with
//!   `message=None`.
//! * Unknown workspace slug: 403, never 404 — the gate's slug-scoped
//!   `exists()` misses before any object lookup runs.
//!
//! Throttles (verified, not ported): `api/views/scheduler.py` declares no
//! `throttle_classes` — rate limiting on these routes comes only from the
//! shared `BaseAPIView.get_throttles` (`ApiKeyRateThrottle` /
//! `ServiceTokenRateThrottle`, `api/views/base.py:118-131`), which is
//! cross-cutting infrastructure outside this guard port.
//!
//! Ported from `04920ea31d0178b61aa87926e54795634c7cb4ce`.

use pidash_auth::permissions::{project, workspace};
use pidash_auth::scope::TenantScope;

/// The DRF-default permission-denied body every scheduler class denial
/// renders.
///
/// Byte-exact: `{"detail":"You do not have permission to perform this action."}`
/// (compact separators, `rest_framework` defaults). Alias of
/// [`crate::permissions::DEFAULT_DENIED_BODY`] so handlers have one home.
pub const CLASS_DENIAL_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;

/// Which permission class guards a scheduler route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1SchedulersGate {
    /// `WorkspaceEntityPermission` (`workspace.py:74-85`): safe methods
    /// need any active workspace membership.
    WorkspaceEntity,
    /// `ProjectEntityPermission` (`project.py:85-116`): safe methods need
    /// active membership on `view.project_id`.
    ProjectEntity,
}

/// One v1 scheduler URL pattern (`api/urls/scheduler.py`, 4 entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1Route {
    /// `workspaces/<slug>/schedulers/` (GET):
    /// `WorkspaceSchedulerListAPIEndpoint`.
    WorkspaceList,
    /// `workspaces/<slug>/projects/<project_id>/schedulers/` (GET):
    /// `ProjectSchedulerListAPIEndpoint`.
    ProjectList,
    /// `.../schedulers/<uuid:scheduler_id>/` (GET):
    /// `ProjectSchedulerDetailAPIEndpoint`.
    ProjectDetail,
    /// `.../schedulers/<uuid:scheduler_id>/runs/` (GET):
    /// `ProjectSchedulerRunsAPIEndpoint`.
    ProjectRuns,
}

/// Map a scheduler route+method to its gate, exactly as the view classes
/// declare. Every route here is GET-only and carries one class for its
/// single registered method.
pub fn gate_for(route: V1Route, _method: &str) -> V1SchedulersGate {
    match route {
        V1Route::WorkspaceList => V1SchedulersGate::WorkspaceEntity,
        V1Route::ProjectList | V1Route::ProjectDetail | V1Route::ProjectRuns => {
            V1SchedulersGate::ProjectEntity
        }
    }
}

/// Decide a gate from caller-fetched membership facts.
///
/// Each boolean mirrors one `...objects.filter(...).exists()` in the guard
/// class; the caller's SQL carries the filters shown. `true` passes the
/// request into the handler body, `false` answers 403
/// [`CLASS_DENIAL_BODY`]. Anonymous callers never reach here (the auth layer
/// 401s first); the kernel's `authenticated` flags are set, not checked, by
/// handlers — they exist so a handler cannot accidentally authorize a
/// caller it failed to authenticate.
///
/// Fetch-scoping traps the handler layer must honor:
///
/// * `WorkspaceEntityPermission` SAFE reads **any-role** active workspace
///   membership: `WorkspaceMember` filtered by
///   `(workspace__slug, member, is_active)` — roles are irrelevant on GET
///   (`workspace.py:79-83`).
/// * `ProjectEntityPermission` SAFE reads active project membership on the
///   **rewritten** `view.project_id`: `ProjectMember` filtered by
///   `(workspace__slug, member, project_id, is_active)` (`project.py:100-106`).
///   The scheduler views set no `project_identifier`, so
///   `has_project_identifier` is always false here and the identifier
///   branch (`project.py:91-98`) is dead.
pub fn decide(
    gate: V1SchedulersGate,
    method: &str,
    scope: &TenantScope,
    project_facts: &project::ProjectFacts,
    workspace_facts: &workspace::WorkspaceFacts,
) -> bool {
    match gate {
        V1SchedulersGate::WorkspaceEntity => {
            workspace::decide_workspace_entity(method, scope, workspace_facts)
        }
        V1SchedulersGate::ProjectEntity => {
            project::decide_project_entity(method, scope, project_facts)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::{ProjectId, WorkspaceId};

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn project_facts() -> project::ProjectFacts {
        project::ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated: true,
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

    fn workspace_facts() -> workspace::WorkspaceFacts {
        workspace::WorkspaceFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            has_admin_or_member_role: false,
            has_admin_role: false,
            is_member: false,
            is_admin_unfiltered: false,
        }
    }

    #[test]
    fn route_gate_table() {
        use V1Route as R;
        use V1SchedulersGate as G;
        for (route, gate) in [
            (R::WorkspaceList, G::WorkspaceEntity),
            (R::ProjectList, G::ProjectEntity),
            (R::ProjectDetail, G::ProjectEntity),
            (R::ProjectRuns, G::ProjectEntity),
        ] {
            assert_eq!(gate_for(route, "GET"), gate, "{route:?}");
        }
    }

    #[test]
    fn workspace_entity_matrix() {
        let scope = scope();
        let pf = project_facts();
        // Non-member denied; any-role active member passes.
        let wf = workspace_facts();
        assert!(!decide(
            V1SchedulersGate::WorkspaceEntity,
            "GET",
            &scope,
            &pf,
            &wf
        ));
        let wf = workspace::WorkspaceFacts {
            is_member: true,
            ..workspace_facts()
        };
        assert!(decide(
            V1SchedulersGate::WorkspaceEntity,
            "GET",
            &scope,
            &pf,
            &wf
        ));
    }

    #[test]
    fn project_entity_matrix() {
        let scope = scope();
        let wf = workspace_facts();
        // Workspace membership alone does not pass a project gate.
        let mut pf = project_facts();
        pf.is_workspace_member = true;
        assert!(!decide(
            V1SchedulersGate::ProjectEntity,
            "GET",
            &scope,
            &pf,
            &wf
        ));
        pf.is_project_member = true;
        assert!(decide(
            V1SchedulersGate::ProjectEntity,
            "GET",
            &scope,
            &pf,
            &wf
        ));
    }

    #[test]
    fn cross_workspace_scope_denies() {
        let other = TenantScope::new(WorkspaceId::from("other"));
        let pf = project::ProjectFacts {
            is_project_member: true,
            ..project_facts()
        };
        let wf = workspace::WorkspaceFacts {
            is_member: true,
            ..workspace_facts()
        };
        assert!(!decide(
            V1SchedulersGate::WorkspaceEntity,
            "GET",
            &other,
            &pf,
            &wf
        ));
        assert!(!decide(
            V1SchedulersGate::ProjectEntity,
            "GET",
            &other,
            &pf,
            &wf
        ));
    }
}
