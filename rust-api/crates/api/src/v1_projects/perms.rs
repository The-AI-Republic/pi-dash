//! D-19 permission guards (stage 5, PIDASHCONV-367).
//!
//! Ports the `permission_classes` / `get_permissions` lines of
//! `apps/api/pi_dash/api/views/{project,member,invite,user,state,estimate}.py`
//! for the api layer. Fixture: `rust-api/fixtures/v1_projects/guards/`
//! (`permissions.golden.json`, FX-PERMS; trace:
//! `rust-api/fixtures/v1_projects/TRACE.md`).
//!
//! Shape of the port: the decision kernel lives in the read-only F-06
//! foundation (`pidash_auth::permissions::{project,workspace}`); this module
//! only pins which gate each D-19 route+method carries ([`gate_for`]) and
//! re-exports the single deny body the handlers answer ([`CLASS_DENIAL_BODY`]).
//! Row fetching stays with the handler layer (issues 369/371/372), which must
//! honor the fetch-scoping traps documented on [`decide`].
//!
//! Gate order (preserved, not redesigned): DRF `initial()` runs API-key
//! authentication (`api/views/base.py:101`), then the slug→UUID rewrite
//! (`base.py:52-104`, skipped for anonymous callers so slugs cannot be
//! probed via 404-vs-401), then `check_permissions`, then the handler body.
//! Anonymous callers therefore 401 on every D-19 route and never reach a
//! gate (fixture `authn_before_authz` legend).
//!
//! Response shapes (handlers render these; recorded here so the matrix has
//! one home):
//!
//! * Anonymous on any D-19 route: 401 (missing credentials) — the auth layer
//!   answers before any gate runs. An invalid token answers 403
//!   `{"detail":"Given API token is not valid"}` (pinned by
//!   `contract test_me_bad_token_403`).
//! * Denied member: 403 [`CLASS_DENIAL_BODY`] — the DRF-default
//!   `PermissionDenied` body. None of the 7 classes sets `message`, so every
//!   class denial renders through `APIView.permission_denied` with
//!   `message=None` (fixture `deny_body` legend).
//! * `users/me/` (`UserEndpoint`, `user.py:18`) declares no
//!   `permission_classes`, so it carries [`V1ProjectsGate::AuthOnly`]: any
//!   authenticated caller passes.
//!
//! Throttles (verified, not ported): none of
//! `api/views/{project,member,invite,user,state,estimate}.py` declares
//! `throttle_classes`, `throttle_scope`, or any `throttle` reference —
//! rate limiting on these routes comes only from the shared
//! `BaseAPIView.get_throttles` (`ApiKeyRateThrottle` /
//! `ServiceTokenRateThrottle`, `api/views/base.py:118-131`), which is
//! cross-cutting infrastructure outside this guard port.
//!
//! Name resolution (verified against the imports; the copies are
//! interchangeable for D-19):
//!
//! * `member.py:18`, `invite.py:20` import their names from
//!   `pi_dash.utils.permissions`.
//! * `project.py:48`, `state.py:15`, `estimate.py:11` import from
//!   `pi_dash.app.permissions` (estimate imports `ProjectEntityPermission`
//!   from `pi_dash.app.permissions.project` directly).
//! * `utils/permissions/workspace.py` is byte-identical to
//!   `app/permissions/workspace.py` (diff empty); `utils/project.py` is
//!   identical except it lacks `can_mutate_states` +
//!   `ProjectStateEntityPermission` (app-only); the `base.py` copies differ
//!   only in the creator workspace pre-check (`app/base.py:25-35`), which no
//!   D-19 view invokes (no `creator=True` call sites in these views).
//!
//! Ported quirks (translate, don't redesign):
//!
//! * BUG-6 `WorkspaceOwnerPermission` has no `is_active` filter
//!   (`workspace.py:56-58`) — inactive workspace admins pass. Kernel fact:
//!   [`pidash_auth::permissions::workspace::WorkspaceFacts::is_admin_unfiltered`].
//! * BUG-7 `ProjectMemberPermission` SAFE branch is workspace-scoped, not
//!   project-scoped (`project.py:62-65`: no `project_id` filter) — any
//!   project membership in the workspace reads every project's member list.
//!   Handlers must fetch that one fact without a `project_id` filter; see
//!   [`decide`].
//! * BUG-2 the estimate URL patterns (`api/urls/estimate.py`, 3 entries)
//!   are never registered in `api/urls/__init__.py`, so every estimate route
//!   404s before permissions run. The [`V1ProjectsGate::ProjectEntity`]
//!   mapping is ported anyway for the handler layer.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52` (zero drift
//! Ported-from→HEAD on all D-19 permission and view sources, verified
//! 2026-09-29).

use pidash_auth::permissions::{project, workspace};
use pidash_auth::scope::TenantScope;

/// The DRF-default permission-denied body every D-19 class denial renders.
///
/// Byte-exact: `{"detail":"You do not have permission to perform this action."}`
/// (compact separators, `rest_framework` defaults). Alias of
/// [`crate::permissions::DEFAULT_DENIED_BODY`] so handlers have one home.
pub const CLASS_DENIAL_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;

/// Which permission class (or lack of one) guards a D-19 route+method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1ProjectsGate {
    /// `ProjectBasePermission` (`app/permissions/project.py:13-55`).
    ProjectBase,
    /// `ProjectMemberPermission` (`project.py:56-82`).
    ProjectMember,
    /// `ProjectAdminPermission` (`project.py:119-130`).
    ProjectAdmin,
    /// `ProjectEntityPermission` (`project.py:85-116`).
    ProjectEntity,
    /// `ProjectStateEntityPermission` (`project.py:187-206`).
    ProjectStateEntity,
    /// `WorkspaceOwnerPermission` (`workspace.py:51-60`).
    WorkspaceOwner,
    /// `WorkSpaceAdminPermission` (`workspace.py:61-71`).
    WorkspaceAdmin,
    /// No `permission_classes`: base `IsAuthenticated` only
    /// (`UserEndpoint`, `user.py:18`).
    AuthOnly,
}

/// One D-19 URL pattern (methods vary per pattern; see `api/urls/`).
///
/// Aliased patterns (`members/` vs `project-members/`) share their view
/// class, so they share their gate — the aliases are separate variants only
/// so the table pins that both are wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1Route {
    /// `workspaces/<slug>/projects/` (`urls/project.py`, GET/POST):
    /// `ProjectListCreateAPIEndpoint` (`views/project.py:79`).
    ProjectList,
    /// `workspaces/<slug>/projects/<pk>/` (GET/PATCH/DELETE):
    /// `ProjectDetailAPIEndpoint` (`views/project.py:294`).
    ProjectDetail,
    /// `.../projects/<project_id>/archive/` (POST/DELETE):
    /// `ProjectArchiveUnarchiveAPIEndpoint` (`views/project.py:514`).
    ProjectArchive,
    /// `.../projects/<project_id>/summary/` (GET):
    /// `ProjectSummaryAPIEndpoint` (`views/project.py:577`).
    ProjectSummary,
    /// `.../projects/<project_id>/members/` (GET/POST) and the
    /// `project-members/` alias: `ProjectMemberListCreateAPIEndpoint`
    /// (`views/member.py:95`) with `get_permissions` (`member.py:98-101`):
    /// GET carries `ProjectMemberPermission`, every other method carries
    /// `ProjectAdminPermission`.
    ProjectMembers,
    /// `.../members/<pk>/` (GET/PATCH/DELETE) and the `project-members/`
    /// alias: `ProjectMemberDetailAPIEndpoint`
    /// (`views/member.py:160`, subclasses the list/create endpoint, so the
    /// same `get_permissions` branch applies).
    ProjectMemberDetail,
    /// `workspaces/<slug>/members/` (GET): `WorkspaceMemberAPIEndpoint`
    /// (`views/member.py:32`).
    WorkspaceMembers,
    /// `workspaces/<slug>/` router (list/create/retrieve/update/destroy):
    /// workspace-invite viewset (`views/invite.py:32`).
    Invites,
    /// `users/me/` (GET): `UserEndpoint` (`views/user.py:18`), no
    /// `permission_classes`.
    UserMe,
    /// `.../projects/<project_id>/states/` (GET/POST):
    /// `StateListCreateAPIEndpoint` (`views/state.py:44`).
    StateList,
    /// `.../states/<state_id>/` (GET/PATCH/DELETE): `StateDetailAPIEndpoint`
    /// (`views/state.py:167`).
    StateDetail,
    /// `.../projects/<project_id>/estimates/` (GET/POST/PATCH/DELETE):
    /// `ProjectEstimateAPIEndpoint` (`views/estimate.py:31`).
    /// UNREGISTERED (BUG-2) — the gate is ported anyway.
    Estimate,
    /// `.../estimates/<estimate_id>/estimate-points/` (GET/POST):
    /// `EstimatePointListCreateAPIEndpoint` (`views/estimate.py:140`).
    /// UNREGISTERED (BUG-2).
    EstimatePoints,
    /// `.../estimate-points/<estimate_point_id>/` (PATCH/DELETE):
    /// `EstimatePointDetailAPIEndpoint` (`views/estimate.py:237`).
    /// UNREGISTERED (BUG-2).
    EstimatePointDetail,
}

/// Map a D-19 route+method to its gate, exactly as the view classes declare.
///
/// `method` is the uppercase HTTP method (`GET`, `POST`, `PATCH`, `DELETE`,
/// …). Only the `ProjectMembers` / `ProjectMemberDetail` routes branch on
/// it (the `get_permissions` override, `views/member.py:98-101`); every
/// other route carries one class for all its registered methods.
pub fn gate_for(route: V1Route, method: &str) -> V1ProjectsGate {
    match route {
        V1Route::ProjectList | V1Route::ProjectDetail | V1Route::ProjectArchive => {
            V1ProjectsGate::ProjectBase
        }
        V1Route::ProjectSummary | V1Route::WorkspaceMembers => V1ProjectsGate::WorkspaceAdmin,
        V1Route::ProjectMembers | V1Route::ProjectMemberDetail => {
            // `get_permissions` checks `self.request.method == "GET"`, not
            // `SAFE_METHODS`: HEAD/OPTIONS on these routes fall to
            // `ProjectAdminPermission`, exactly as written.
            if method == "GET" {
                V1ProjectsGate::ProjectMember
            } else {
                V1ProjectsGate::ProjectAdmin
            }
        }
        V1Route::Invites => V1ProjectsGate::WorkspaceOwner,
        V1Route::UserMe => V1ProjectsGate::AuthOnly,
        V1Route::StateList | V1Route::StateDetail => V1ProjectsGate::ProjectStateEntity,
        V1Route::Estimate | V1Route::EstimatePoints | V1Route::EstimatePointDetail => {
            V1ProjectsGate::ProjectEntity
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
/// Fetch-scoping traps the handler layer must honor (the decision functions
/// themselves are scope-correct only when the facts are fetched as below):
///
/// * `ProjectMemberPermission` SAFE reads **workspace-scoped**
///   `is_project_member`: `ProjectMember` filtered by
///   `(workspace__slug, member, is_active)` with **no** `project_id`
///   (BUG-7, `project.py:62-65`). Every other project fact on every other
///   gate adds `project_id=view.project_id`.
/// * `ProjectEntityPermission` SAFE with `has_project_identifier` reads
///   `has_identifier_membership` from the identifier's project, not
///   `view.project_id` (`project.py:91-98`).
/// * `WorkspaceOwnerPermission` reads `is_admin_unfiltered` with **no**
///   `is_active` filter (BUG-6, `workspace.py:56-58`).
/// * `can_mutate_states` reads a single membership row
///   (`role`, `project__members_can_edit_states`) plus one workspace-admin
///   `exists()`; a missing row denies before the admin override
///   (`project.py:169-184`).
#[allow(clippy::too_many_arguments)]
pub fn decide(
    gate: V1ProjectsGate,
    method: &str,
    scope: &TenantScope,
    project_facts: &project::ProjectFacts,
    workspace_facts: &workspace::WorkspaceFacts,
    mutation: &project::StateMutationFacts,
) -> bool {
    match gate {
        V1ProjectsGate::ProjectBase => project::decide_project_base(method, scope, project_facts),
        V1ProjectsGate::ProjectMember => {
            project::decide_project_member(method, scope, project_facts)
        }
        V1ProjectsGate::ProjectAdmin => project::decide_project_admin(scope, project_facts),
        V1ProjectsGate::ProjectEntity => {
            project::decide_project_entity(method, scope, project_facts)
        }
        V1ProjectsGate::ProjectStateEntity => {
            project::decide_project_state_entity(method, scope, project_facts, mutation)
        }
        V1ProjectsGate::WorkspaceOwner => workspace::decide_workspace_owner(scope, workspace_facts),
        V1ProjectsGate::WorkspaceAdmin => workspace::decide_workspace_admin(scope, workspace_facts),
        // No class: the base `IsAuthenticated` already passed, so the
        // authenticated caller reaches the handler body.
        V1ProjectsGate::AuthOnly => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
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

    fn mutation(role: Option<i32>) -> project::StateMutationFacts {
        project::StateMutationFacts {
            authenticated: true,
            project_role: role,
            members_can_edit_states: false,
            is_workspace_admin: false,
        }
    }

    /// Workspace ADMIN with no project row (active).
    fn ws_admin(pf: &mut project::ProjectFacts, wf: &mut workspace::WorkspaceFacts) {
        pf.is_workspace_member = true;
        pf.has_workspace_admin_or_member = true;
        pf.is_workspace_admin = true;
        wf.is_member = true;
        wf.has_admin_or_member_role = true;
        wf.has_admin_role = true;
        wf.is_admin_unfiltered = true;
    }

    /// Workspace MEMBER (15) with no project row (active).
    fn ws_member(pf: &mut project::ProjectFacts, wf: &mut workspace::WorkspaceFacts) {
        pf.is_workspace_member = true;
        pf.has_workspace_admin_or_member = true;
        wf.is_member = true;
        wf.has_admin_or_member_role = true;
    }

    /// Workspace GUEST (5) with no project row (active).
    fn ws_guest(pf: &mut project::ProjectFacts, wf: &mut workspace::WorkspaceFacts) {
        pf.is_workspace_member = true;
        wf.is_member = true;
    }

    /// Active project ADMIN row (plus workspace membership).
    fn project_admin(pf: &mut project::ProjectFacts) {
        pf.is_workspace_member = true;
        pf.has_workspace_admin_or_member = true;
        pf.is_project_member = true;
        pf.is_project_admin = true;
        pf.has_project_admin_or_member = true;
    }

    /// Active project MEMBER (15) row (plus workspace membership).
    fn project_member(pf: &mut project::ProjectFacts) {
        pf.is_workspace_member = true;
        pf.has_workspace_admin_or_member = true;
        pf.is_project_member = true;
        pf.has_project_admin_or_member = true;
    }

    /// Active project GUEST (5) row (plus workspace membership).
    fn project_guest(pf: &mut project::ProjectFacts) {
        pf.is_workspace_member = true;
        pf.has_workspace_admin_or_member = true;
        pf.is_project_member = true;
    }

    fn decide_project(
        gate: V1ProjectsGate,
        method: &str,
        pf: &project::ProjectFacts,
        role: Option<i32>,
        flag: bool,
        ws_admin: bool,
    ) -> bool {
        let wf = workspace_facts();
        let mut m = mutation(role);
        m.members_can_edit_states = flag;
        m.is_workspace_admin = ws_admin;
        decide(gate, method, &scope(), pf, &wf, &m)
    }

    #[test]
    fn class_denial_body_is_byte_identical_drf_default() {
        // DRF `PermissionDenied.default_detail`, compact separators.
        assert_eq!(
            CLASS_DENIAL_BODY,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(CLASS_DENIAL_BODY, crate::permissions::DEFAULT_DENIED_BODY);
    }

    #[test]
    fn gate_table_pins_every_route_and_method() {
        use V1ProjectsGate as G;
        use V1Route as R;
        let cases = [
            (R::ProjectList, "GET", G::ProjectBase),
            (R::ProjectList, "POST", G::ProjectBase),
            (R::ProjectDetail, "GET", G::ProjectBase),
            (R::ProjectDetail, "PATCH", G::ProjectBase),
            (R::ProjectDetail, "DELETE", G::ProjectBase),
            (R::ProjectArchive, "POST", G::ProjectBase),
            (R::ProjectArchive, "DELETE", G::ProjectBase),
            (R::ProjectSummary, "GET", G::WorkspaceAdmin),
            (R::ProjectMembers, "GET", G::ProjectMember),
            // `get_permissions` branches on `method == "GET"`: POST (and
            // anything else) falls to the admin class.
            (R::ProjectMembers, "POST", G::ProjectAdmin),
            (R::ProjectMembers, "HEAD", G::ProjectAdmin),
            (R::ProjectMemberDetail, "GET", G::ProjectMember),
            (R::ProjectMemberDetail, "PATCH", G::ProjectAdmin),
            (R::ProjectMemberDetail, "DELETE", G::ProjectAdmin),
            (R::WorkspaceMembers, "GET", G::WorkspaceAdmin),
            (R::Invites, "GET", G::WorkspaceOwner),
            (R::Invites, "POST", G::WorkspaceOwner),
            (R::Invites, "PATCH", G::WorkspaceOwner),
            (R::Invites, "DELETE", G::WorkspaceOwner),
            (R::UserMe, "GET", G::AuthOnly),
            (R::StateList, "GET", G::ProjectStateEntity),
            (R::StateList, "POST", G::ProjectStateEntity),
            (R::StateDetail, "GET", G::ProjectStateEntity),
            (R::StateDetail, "PATCH", G::ProjectStateEntity),
            (R::StateDetail, "DELETE", G::ProjectStateEntity),
            (R::Estimate, "GET", G::ProjectEntity),
            (R::Estimate, "POST", G::ProjectEntity),
            (R::Estimate, "PATCH", G::ProjectEntity),
            (R::Estimate, "DELETE", G::ProjectEntity),
            (R::EstimatePoints, "GET", G::ProjectEntity),
            (R::EstimatePoints, "POST", G::ProjectEntity),
            (R::EstimatePointDetail, "PATCH", G::ProjectEntity),
            (R::EstimatePointDetail, "DELETE", G::ProjectEntity),
        ];
        for (route, method, gate) in cases {
            assert_eq!(gate_for(route, method), gate, "{route:?} {method}");
        }
    }

    #[test]
    fn project_base_matrix() {
        // Outsider: deny 403 all methods.
        let pf = project_facts();
        for method in ["GET", "POST", "PATCH", "DELETE"] {
            assert!(!decide_project(
                V1ProjectsGate::ProjectBase,
                method,
                &pf,
                None,
                false,
                false
            ));
        }
        // Workspace GUEST, no project row: GET allow; POST/PATCH deny.
        let (mut pf, mut wf) = (project_facts(), workspace_facts());
        ws_guest(&mut pf, &mut wf);
        assert!(decide_project(
            V1ProjectsGate::ProjectBase,
            "GET",
            &pf,
            None,
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectBase,
            "POST",
            &pf,
            None,
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectBase,
            "PATCH",
            &pf,
            None,
            false,
            false
        ));
        // Project MEMBER + ws MEMBER: GET allow; POST allow (ws 15);
        // PATCH deny (not admin, not ws-admin).
        let mut pf = project_facts();
        project_member(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectBase,
            "GET",
            &pf,
            Some(ROLE_MEMBER),
            false,
            false
        ));
        assert!(decide_project(
            V1ProjectsGate::ProjectBase,
            "POST",
            &pf,
            Some(ROLE_MEMBER),
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectBase,
            "PATCH",
            &pf,
            Some(ROLE_MEMBER),
            false,
            false
        ));
        // Project GUEST + ws ADMIN: PATCH allow (override branch).
        let mut pf = project_facts();
        project_guest(&mut pf);
        pf.is_workspace_admin = true;
        assert!(decide_project(
            V1ProjectsGate::ProjectBase,
            "PATCH",
            &pf,
            Some(ROLE_GUEST),
            false,
            true
        ));
        // Project GUEST + ws MEMBER: GET allow; POST allow (the POST
        // branch keys ONLY on the workspace role, never the project role —
        // `project.py:25-31`); PATCH deny (not project-admin, not ws-admin).
        // NOTE: the FX-PERMS `project_guest_ws_member` cell says "POST deny",
        // which contradicts the code and the fixture's own
        // `project_member_ws_member` cell ("POST allow (ws 15)"); the code
        // governs (translation, not redesign) — flagged in the PR for the
        // domain gate (PIDASHCONV-373).
        let mut pf = project_facts();
        project_guest(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectBase,
            "GET",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        assert!(decide_project(
            V1ProjectsGate::ProjectBase,
            "POST",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectBase,
            "PATCH",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        // WS ADMIN, no project row: GET/POST allow; PATCH deny (override
        // needs a project row).
        let mut pf = project_facts();
        let mut wf = workspace_facts();
        ws_admin(&mut pf, &mut wf);
        assert!(decide_project(
            V1ProjectsGate::ProjectBase,
            "GET",
            &pf,
            None,
            false,
            true
        ));
        assert!(decide_project(
            V1ProjectsGate::ProjectBase,
            "POST",
            &pf,
            None,
            false,
            true
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectBase,
            "PATCH",
            &pf,
            None,
            false,
            true
        ));
        // Project ADMIN: allow all.
        let mut pf = project_facts();
        project_admin(&mut pf);
        for method in ["GET", "POST", "PATCH", "DELETE"] {
            assert!(decide_project(
                V1ProjectsGate::ProjectBase,
                method,
                &pf,
                Some(ROLE_ADMIN),
                false,
                false
            ));
        }
    }

    #[test]
    fn project_member_matrix_and_workspace_scoped_safe() {
        // Outsider: deny 403.
        let pf = project_facts();
        assert!(!decide_project(
            V1ProjectsGate::ProjectMember,
            "GET",
            &pf,
            None,
            false,
            false
        ));
        // Workspace MEMBER with NO project row: GET deny (the SAFE fact is
        // workspace-scoped, but a caller with no row anywhere still denies).
        let (mut pf, mut wf) = (project_facts(), workspace_facts());
        ws_member(&mut pf, &mut wf);
        assert!(!decide_project(
            V1ProjectsGate::ProjectMember,
            "GET",
            &pf,
            None,
            false,
            false
        ));
        // BUG-7: workspace MEMBER holding a row on ANOTHER project reads
        // this project's member list — the SAFE fetch carries no
        // `project_id`, so `is_project_member` is true here.
        pf.is_project_member = true;
        assert!(decide_project(
            V1ProjectsGate::ProjectMember,
            "GET",
            &pf,
            None,
            false,
            false
        ));
        // Project GUEST: GET allow (any active row passes the SAFE branch).
        let mut pf = project_facts();
        project_guest(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectMember,
            "GET",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        // POST needs workspace role 20/15: member passes, guest denies.
        let mut pf = project_facts();
        project_member(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectMember,
            "POST",
            &pf,
            Some(ROLE_MEMBER),
            false,
            false
        ));
        let mut pf = project_facts();
        project_guest(&mut pf);
        pf.has_workspace_admin_or_member = false;
        assert!(!decide_project(
            V1ProjectsGate::ProjectMember,
            "POST",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        // Tail branch (ported though unreachable from D-19 routes):
        // non-safe, non-POST needs project role 20/15.
        let mut pf = project_facts();
        project_member(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectMember,
            "PUT",
            &pf,
            Some(ROLE_MEMBER),
            false,
            false
        ));
        let mut pf = project_facts();
        project_guest(&mut pf);
        assert!(!decide_project(
            V1ProjectsGate::ProjectMember,
            "PUT",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
    }

    #[test]
    fn project_entity_matrix() {
        // Outsider: deny 403.
        let pf = project_facts();
        assert!(!decide_project(
            V1ProjectsGate::ProjectEntity,
            "GET",
            &pf,
            None,
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectEntity,
            "PATCH",
            &pf,
            None,
            false,
            false
        ));
        // Workspace MEMBER with no project row: deny all.
        let (mut pf, mut wf) = (project_facts(), workspace_facts());
        ws_member(&mut pf, &mut wf);
        assert!(!decide_project(
            V1ProjectsGate::ProjectEntity,
            "GET",
            &pf,
            None,
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectEntity,
            "PATCH",
            &pf,
            None,
            false,
            false
        ));
        // Project GUEST: GET allow; PATCH deny.
        let mut pf = project_facts();
        project_guest(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectEntity,
            "GET",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectEntity,
            "PATCH",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        // Project MEMBER: allow all.
        let mut pf = project_facts();
        project_member(&mut pf);
        for method in ["GET", "POST", "PATCH", "DELETE"] {
            assert!(decide_project(
                V1ProjectsGate::ProjectEntity,
                method,
                &pf,
                Some(ROLE_MEMBER),
                false,
                false
            ));
        }
        // Identifier branch: SAFE with `project_identifier` set checks the
        // identifier's membership, not `view.project_id`.
        let mut pf = project_facts();
        pf.has_project_identifier = true;
        pf.has_identifier_membership = true;
        assert!(decide_project(
            V1ProjectsGate::ProjectEntity,
            "GET",
            &pf,
            None,
            false,
            false
        ));
        // Writes ignore the identifier branch.
        assert!(!decide_project(
            V1ProjectsGate::ProjectEntity,
            "PATCH",
            &pf,
            None,
            false,
            false
        ));
    }

    #[test]
    fn project_admin_matrix() {
        // Only an active project-ADMIN row allows, any method.
        let mut pf = project_facts();
        project_admin(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectAdmin,
            "POST",
            &pf,
            Some(ROLE_ADMIN),
            false,
            false
        ));
        assert!(decide_project(
            V1ProjectsGate::ProjectAdmin,
            "DELETE",
            &pf,
            Some(ROLE_ADMIN),
            false,
            false
        ));
        // Project MEMBER and GUEST rows deny.
        let mut pf = project_facts();
        project_member(&mut pf);
        assert!(!decide_project(
            V1ProjectsGate::ProjectAdmin,
            "PATCH",
            &pf,
            Some(ROLE_MEMBER),
            false,
            false
        ));
        let mut pf = project_facts();
        project_guest(&mut pf);
        assert!(!decide_project(
            V1ProjectsGate::ProjectAdmin,
            "DELETE",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        // Workspace ADMIN without a project-ADMIN row denies.
        let (mut pf, mut wf) = (project_facts(), workspace_facts());
        ws_admin(&mut pf, &mut wf);
        assert!(!decide_project(
            V1ProjectsGate::ProjectAdmin,
            "PATCH",
            &pf,
            None,
            false,
            true
        ));
        // Outsider denies.
        let pf = project_facts();
        assert!(!decide_project(
            V1ProjectsGate::ProjectAdmin,
            "GET",
            &pf,
            None,
            false,
            false
        ));
    }

    #[test]
    fn state_entity_matrix() {
        // Outsider: deny 403.
        let pf = project_facts();
        assert!(!decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "GET",
            &pf,
            None,
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "POST",
            &pf,
            None,
            false,
            false
        ));
        // Project ADMIN: allow all.
        let mut pf = project_facts();
        project_admin(&mut pf);
        for method in ["GET", "POST", "PATCH", "DELETE"] {
            assert!(decide_project(
                V1ProjectsGate::ProjectStateEntity,
                method,
                &pf,
                Some(ROLE_ADMIN),
                false,
                false
            ));
        }
        // Project MEMBER with the flag on: allow all.
        let mut pf = project_facts();
        project_member(&mut pf);
        for method in ["GET", "POST", "PATCH", "DELETE"] {
            assert!(decide_project(
                V1ProjectsGate::ProjectStateEntity,
                method,
                &pf,
                Some(ROLE_MEMBER),
                true,
                false
            ));
        }
        // Project MEMBER with the flag off: GET allow; writes deny.
        let mut pf = project_facts();
        project_member(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "GET",
            &pf,
            Some(ROLE_MEMBER),
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "POST",
            &pf,
            Some(ROLE_MEMBER),
            false,
            false
        ));
        // Project GUEST + ws ADMIN: PATCH allow (override); GET allow.
        let mut pf = project_facts();
        project_guest(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "PATCH",
            &pf,
            Some(ROLE_GUEST),
            false,
            true
        ));
        // Project GUEST + ws MEMBER: GET allow; writes deny.
        let mut pf = project_facts();
        project_guest(&mut pf);
        assert!(decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "GET",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "POST",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "DELETE",
            &pf,
            Some(ROLE_GUEST),
            false,
            false
        ));
        // WS ADMIN with NO project row: GET deny (no membership row) and
        // POST deny (override never reached).
        let (mut pf, mut wf) = (project_facts(), workspace_facts());
        ws_admin(&mut pf, &mut wf);
        assert!(!decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "GET",
            &pf,
            None,
            false,
            true
        ));
        assert!(!decide_project(
            V1ProjectsGate::ProjectStateEntity,
            "POST",
            &pf,
            None,
            false,
            true
        ));
    }

    fn decide_workspace(gate: V1ProjectsGate, wf: &workspace::WorkspaceFacts) -> bool {
        let pf = project_facts();
        let m = mutation(None);
        decide(gate, "GET", &scope(), &pf, wf, &m)
    }

    #[test]
    fn workspace_owner_matrix_and_inactive_quirk() {
        // Outsider: deny.
        assert!(!decide_workspace(
            V1ProjectsGate::WorkspaceOwner,
            &workspace_facts()
        ));
        // Workspace ADMIN: allow all methods (gate ignores the method).
        let mut wf = workspace_facts();
        wf.is_admin_unfiltered = true;
        for method in ["GET", "POST", "PATCH", "DELETE"] {
            let pf = project_facts();
            let m = mutation(None);
            assert!(decide(
                V1ProjectsGate::WorkspaceOwner,
                method,
                &scope(),
                &pf,
                &wf,
                &m
            ));
        }
        // Workspace MEMBER (15): deny everything, including GET.
        let mut wf = workspace_facts();
        wf.is_member = true;
        wf.has_admin_or_member_role = true;
        assert!(!decide_workspace(V1ProjectsGate::WorkspaceOwner, &wf));
        // BUG-6: INACTIVE workspace admin still passes — the check carries
        // no `is_active` filter, so only the unfiltered fact matters.
        let mut wf = workspace_facts();
        wf.is_admin_unfiltered = true;
        assert!(decide_workspace(V1ProjectsGate::WorkspaceOwner, &wf));
    }

    #[test]
    fn workspace_admin_matrix() {
        // ADMIN and MEMBER allow; GUEST denies.
        let mut wf = workspace_facts();
        wf.is_member = true;
        wf.has_admin_or_member_role = true;
        wf.has_admin_role = true;
        assert!(decide_workspace(V1ProjectsGate::WorkspaceAdmin, &wf));
        let mut wf = workspace_facts();
        wf.is_member = true;
        wf.has_admin_or_member_role = true;
        assert!(decide_workspace(V1ProjectsGate::WorkspaceAdmin, &wf));
        let mut wf = workspace_facts();
        wf.is_member = true;
        assert!(!decide_workspace(V1ProjectsGate::WorkspaceAdmin, &wf));
        // Outsider denies.
        assert!(!decide_workspace(
            V1ProjectsGate::WorkspaceAdmin,
            &workspace_facts()
        ));
    }

    #[test]
    fn anonymous_and_cross_scope_deny_everywhere() {
        // Anonymous (authenticated=false) denies on every gate, even with
        // full membership facts.
        let mut pf = project_facts();
        project_admin(&mut pf);
        pf.authenticated = false;
        let mut wf = workspace_facts();
        wf.is_admin_unfiltered = true;
        wf.authenticated = false;
        let m = mutation(Some(ROLE_ADMIN));
        let gates = [
            V1ProjectsGate::ProjectBase,
            V1ProjectsGate::ProjectMember,
            V1ProjectsGate::ProjectAdmin,
            V1ProjectsGate::ProjectEntity,
            V1ProjectsGate::ProjectStateEntity,
            V1ProjectsGate::WorkspaceOwner,
            V1ProjectsGate::WorkspaceAdmin,
        ];
        for gate in gates {
            assert!(!decide(gate, "GET", &scope(), &pf, &wf, &m), "{gate:?}");
        }
        // Facts fetched for another workspace deny (fail closed) — with
        // fully authenticated facts, so only the scope denies.
        let other = TenantScope::new(WorkspaceId::from("other"));
        let mut pf = project_facts();
        project_admin(&mut pf);
        let mut wf = workspace_facts();
        wf.is_admin_unfiltered = true;
        let m = mutation(Some(ROLE_ADMIN));
        assert!(!decide(
            V1ProjectsGate::ProjectBase,
            "GET",
            &other,
            &pf,
            &wf,
            &m
        ));
        assert!(!decide(
            V1ProjectsGate::WorkspaceAdmin,
            "GET",
            &other,
            &pf,
            &wf,
            &m
        ));
        // AuthOnly passes: the base `IsAuthenticated` already admitted the
        // caller, so there is no class left to deny.
        let pf = project_facts();
        let wf = workspace_facts();
        assert!(decide(
            V1ProjectsGate::AuthOnly,
            "GET",
            &scope(),
            &pf,
            &wf,
            &m
        ));
    }
}
