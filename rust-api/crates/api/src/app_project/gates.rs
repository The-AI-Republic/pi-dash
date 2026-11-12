//! D-25 permission gates (stage 5, PIDASHCONV-569).
//!
//! Ports the L7 guard layer on all 29 D-25 routes (project 20, state 4,
//! estimate 5 — 54 method+path rows: 28 `@allow_permission` actions, 11
//! DRF permission-class actions, 13 `IsAuthenticated`-only fallthroughs,
//! and the 2 `AllowAny` join actions), from:
//!
//! - `apps/api/pi_dash/app/permissions/base.py:13-16` (`ROLE`),
//!   `:19-87` (`allow_permission`)
//! - `apps/api/pi_dash/app/permissions/project.py:56-82`
//!   (`ProjectMemberPermission`), `:85-116` (`ProjectEntityPermission`),
//!   `:146-184` (`can_mutate_states`)
//! - `apps/api/pi_dash/app/permissions/workspace.py:103-110`
//!   (`WorkspaceUserPermission`)
//! - `apps/api/pi_dash/utils/cache.py:15-22` (`generate_cache_key`),
//!   `:54-88` (`invalidate_cache_directly` / `invalidate_cache`)
//!
//! Fixture id: FX-APROJ-07 (`rust-api/fixtures/app_project/`).
//!
//! Shape of the port: the module is pure, like the
//! [`pidash_auth::permissions`] kernel it sits on. Membership facts are
//! caller inputs; row fetching (the `WorkspaceMember`/`ProjectMember`
//! `...exists()` queries with their `workspace__slug=` / `project_id=` /
//! `is_active=True` filters) stays with the handlers, which fetch through
//! the workspace-scoped handle — the `tenant_context` half of the pilot
//! pattern. This module only maps each route to its [`Gate`] and decides
//! through the kernel, so the PROJECT-level workspace-admin override
//! (`app/permissions/base.py`: project member + workspace role exactly
//! ADMIN passes regardless of project role) and the deny-by-default
//! tenant scope come along unchanged.
//!
//! Gate order (preserved, not redesigned):
//!
//! - Decorated routes: Django-session authentication (`BaseViewSet` /
//!   `BaseAPIView`: `BaseSessionAuthentication` + `IsAuthenticated`,
//!   `app/views/base.py:87,190`) runs before the decorator, and the
//!   decorator runs before the handler body. Anonymous callers never
//!   reach a gate ([`GateOutcome::Unauthenticated`]).
//! - Class-guarded routes (deploy boards, bulk estimates, project
//!   roles): DRF checks `permission_classes` in `initial()`, before the
//!   handler method runs. Anonymous callers 401 there too: the views
//!   carry authenticators but no successful authenticator, so DRF raises
//!   `NotAuthenticated` rather than `PermissionDenied`.
//! - `AllowAny` join routes: no auth at all; anonymous callers reach the
//!   body (fixture: anon join GET gets the view's own 404).
//! - State writes add a second, inline gate after the decorator:
//!   `can_mutate_states` denies members (unless the project sets
//!   `members_can_edit_states`) and guests with
//!   [`MEMBERS_BLOCKED_BODY`]. Handler-inline 403s (SECRET-project
//!   retrieve, member role comparisons, join email check, archive/admin
//!   checks) stay in the handler issues — this table pins the
//!   decorator/class half only.
//!
//! Denial bodies (byte-exact, compact DRF rendering):
//!
//! - Decorator denial: `{"error":"You don't have the required permissions."}`
//!   (403); see [`FORBIDDEN_BODY`].
//! - Permission-class denial: the classes define no `message`, so DRF
//!   renders `PermissionDenied.default_detail`:
//!   `{"detail":"You do not have permission to perform this action."}`
//!   (403); see [`crate::permissions::DEFAULT_DENIED_BODY`].
//! - Anonymous: `{"detail":"Authentication credentials were not provided."}`
//!   (401) on every route except the two `AllowAny` join actions; see
//!   [`ANON_BODY`]. (Lowercase `detail`: DRF 3.15.2 `exception_handler`
//!   renders `{'detail': exc.detail}`; the fixture's live captures and
//!   `contract-tests/app_project/test_permissions.py` agree.)
//! - State-mutation denial: [`MEMBERS_BLOCKED_BODY`] (403,
//!   `state/base.py:24`).
//!
//! Cache-invalidation order (preserved, not redesigned): on state
//! create/mark-as-default/destroy the `@invalidate_cache` decorator sits
//! *outside* `@allow_permission`, so the key is deleted **before** the
//! gate runs — a 403 still invalidates. On the bulk-estimate writes the
//! permission class runs in DRF `initial()` *before* the wrapped method,
//! so a 403 does **not** invalidate. State `partial_update` carries no
//! `invalidate_cache` at all (`state/base.py:66-67`, ported as-is). See
//! [`invalidation_for`] and [`InvalidationOrder`].
//!
//! Facts-fetch contracts the handlers must honor (the kernel takes
//! booleans; the SQL stays theirs):
//!
//! - `ProjectMemberPermission` SAFE methods filter `ProjectMember` by
//!   workspace slug only — **no `project_id` filter**
//!   (`project.py:63-65`). Handlers fill `is_project_member` for GET/HEAD
//!   without the project predicate; every other branch filters by
//!   project.
//! - `can_mutate_states` reads one membership row plus the project's
//!   `members_can_edit_states` flag; a missing row denies before the
//!   workspace-admin override is reached.
//!
//! Out of scope (verified, not ported): `creator=`/`model=` are never
//! set in D-25; `ProjectStateEntityPermission` has no D-25 call site
//! (state writes use the decorator plus the inline `can_mutate_states`
//! call instead — the kernel already covers the class for whoever needs
//! it); no D-25 view or base view declares throttles (`grep throttle`
//! over the five view files and `app/views/base.py` is empty). The
//! `project_identifier` branch of `ProjectEntityPermission` is dead in
//! this domain (no D-25 view sets the attribute) but is ported anyway —
//! the fixture records its matrix.
//!
//! Ported quirks (translate, don't redesign): the invalidate-before-gate
//! order above; the missing `project_id` filter in the
//! `ProjectMemberPermission` SAFE branch; the undecorated
//! `partial_update`/`destroy` on `ProjectViewSet` (auth-only at the gate,
//! inline admin checks in the handler issue); the PUT-update and the
//! invite/favorites/deploy-board/state-retrieve fallthroughs that reach
//! `ModelViewSet` defaults past `IsAuthenticated`. The fixture text says
//! "29 decorator call sites"; the tree holds 28 `@allow_permission`
//! lines over 29 URL paths — the table below pins the actual 28.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::project::{
    can_mutate_states as kernel_can_mutate_states, decide_project_entity, decide_project_member,
    ProjectFacts, StateMutationFacts,
};
use pidash_auth::permissions::workspace::{decide_workspace_user, WorkspaceFacts};
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_types::WorkspaceId;

/// Exact bytes of the `@allow_permission` 403
/// (`app/permissions/base.py`). Alias of
/// [`crate::permissions::PERMISSION_DENIED_BODY`] so handlers have one home.
pub const FORBIDDEN_BODY: &str = crate::permissions::PERMISSION_DENIED_BODY;
/// Exact bytes of the DRF-default permission-class 403: what the
/// `ProjectMemberPermission` / `ProjectEntityPermission` /
/// `WorkspaceUserPermission` denials render through
/// `APIView.permission_denied` with `message=None`. Alias of
/// [`crate::permissions::DEFAULT_DENIED_BODY`] so handlers have one home.
pub const CLASS_DENIED_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;
/// Exact bytes of the DRF `IsAuthenticated` / `NotAuthenticated` 401:
/// what anonymous callers get on every D-25 route except the two
/// `AllowAny` join actions, before any gate runs.
pub const ANON_BODY: &str = r#"{"detail":"Authentication credentials were not provided."}"#;
/// Exact bytes of the `can_mutate_states` 403
/// (`_MEMBERS_BLOCKED_RESPONSE`, `state/base.py:24`): the inline denial
/// on the four state writes, after the decorator gate passes.
pub const MEMBERS_BLOCKED_BODY: &str =
    r#"{"error":"Members are not permitted to edit workflow states for this project."}"#;

/// How one D-25 route+method authorizes, before any handler logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `permission_classes = [AllowAny]` (`ProjectJoinEndpoint`,
    /// `invite.py:184`): no auth, no membership check. Anonymous
    /// callers reach the body; the gate never denies.
    AllowAny,
    /// No decorator and the `IsAuthenticated` default: any signed-in
    /// user reaches the handler body (undecorated project
    /// `partial_update`/`destroy`, PUT-update, invite/favorites
    /// fallthroughs, project-views post, member-me get, state
    /// retrieve). Anonymous denies first with [`ANON_BODY`].
    Authenticated,
    /// `@allow_permission(..., level="WORKSPACE")`: active workspace
    /// membership with a listed role. Denies with [`FORBIDDEN_BODY`].
    Workspace { roles: &'static [i32] },
    /// `@allow_permission(...)` at the default `"PROJECT"` level:
    /// active project membership with a listed role, or the
    /// workspace-admin override (`app/permissions/base.py`). Denies
    /// with [`FORBIDDEN_BODY`].
    Project { roles: &'static [i32] },
    /// `permission_classes = [ProjectMemberPermission]`
    /// (`DeployBoardViewSet`, `base.py:541`): method-dispatched
    /// project/workspace checks. Authenticated denials render
    /// [`CLASS_DENIED_BODY`].
    ProjectMember,
    /// `permission_classes = [ProjectEntityPermission]`
    /// (`BulkEstimatePointEndpoint`, `estimate/base.py:50`): safe
    /// methods need project membership, writes need project
    /// Admin/Member. Authenticated denials render
    /// [`CLASS_DENIED_BODY`].
    ProjectEntity,
    /// `permission_classes = [WorkspaceUserPermission]`
    /// (`UserProjectRolesEndpoint`, `member.py:343`): any active
    /// workspace membership, every method. Authenticated denials
    /// render [`CLASS_DENIED_BODY`].
    WorkspaceUser,
}

/// One row of the FX-APROJ-07 matrix: a route+method and its gate.
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

/// All 29 D-25 routes, one entry per method, in URL-file order
/// (`app/urls/project.py`, `state.py`, `estimate.py`).
pub static GATES: &[RouteGate] = &[
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:144 (ProjectViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "base.py:257 (ProjectViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/details/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:100 (ProjectViewSet.list_detail)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<pk>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "base.py:225 (ProjectViewSet.retrieve)",
    },
    RouteGate {
        method: "PUT",
        path: "workspaces/<slug>/projects/<pk>/",
        gate: Gate::Authenticated,
        source: "base.py (no def update; PUT falls to UpdateModelMixin.update)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<pk>/",
        gate: Gate::Authenticated,
        source: "base.py:314 (ProjectViewSet.partial_update; no decorator, inline admin checks in handlers)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<pk>/",
        gate: Gate::Authenticated,
        source: "base.py:382 (ProjectViewSet.destroy; no decorator, inline admin checks in handlers)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/project-identifiers/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "base.py:450 (ProjectIdentifierEndpoint.get)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/project-identifiers/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "base.py:461 (ProjectIdentifierEndpoint.delete)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/invitations/",
        gate: Gate::Authenticated,
        source: "invite.py (no def list; GET falls to ModelViewSet default list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/invitations/",
        gate: Gate::Project { roles: ADMIN },
        source: "invite.py:53 (ProjectInvitationsViewset.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/invitations/<pk>/",
        gate: Gate::Authenticated,
        source: "invite.py (no def retrieve; falls to ModelViewSet default)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/invitations/<pk>/",
        gate: Gate::Authenticated,
        source: "invite.py (no def destroy; falls to ModelViewSet default)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/workspaces/<slug>/projects/invitations/",
        gate: Gate::Authenticated,
        source: "invite.py (UserProjectInvitationsViewset: no def list; ModelViewSet default)",
    },
    RouteGate {
        method: "POST",
        path: "users/me/workspaces/<slug>/projects/invitations/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "invite.py:128 (UserProjectInvitationsViewset.create)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/workspaces/<slug>/project-roles/",
        gate: Gate::WorkspaceUser,
        source: "member.py:343 (UserProjectRolesEndpoint.get; permission_classes=[WorkspaceUserPermission])",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/join/<pk>/",
        gate: Gate::AllowAny,
        source: "invite.py:184,251 (ProjectJoinEndpoint.get; permission_classes=[AllowAny])",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/join/<pk>/",
        gate: Gate::AllowAny,
        source: "invite.py:184,186 (ProjectJoinEndpoint.post; permission_classes=[AllowAny])",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/members/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:156 (ProjectMemberViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/members/",
        gate: Gate::Project { roles: ADMIN },
        source: "member.py:46 (ProjectMemberViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/members/<pk>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:171 (ProjectMemberViewSet.retrieve)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/members/<pk>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:205 (ProjectMemberViewSet.partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/members/<pk>/",
        gate: Gate::Project { roles: ADMIN },
        source: "member.py:267 (ProjectMemberViewSet.destroy)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/members/leave/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:300 (ProjectMemberViewSet.leave)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/project-views/",
        gate: Gate::Authenticated,
        source: "base.py:480 (ProjectUserViewsEndpoint.post; no decorator)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/project-members/me/",
        gate: Gate::Authenticated,
        source: "member.py:330 (ProjectMemberUserEndpoint.get; no decorator)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-favorite-projects/",
        gate: Gate::Authenticated,
        source: "base.py (ProjectFavoritesViewSet: no def list; ModelViewSet default)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/user-favorite-projects/",
        gate: Gate::Authenticated,
        source: "base.py:519 (ProjectFavoritesViewSet.create; no decorator)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/user-favorite-projects/<project_id>/",
        gate: Gate::Authenticated,
        source: "base.py:528 (ProjectFavoritesViewSet.destroy; no decorator)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/project-deploy-boards/",
        gate: Gate::ProjectMember,
        source: "base.py:541,545 (DeployBoardViewSet.list; permission_classes=[ProjectMemberPermission])",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/project-deploy-boards/",
        gate: Gate::ProjectMember,
        source: "base.py:541,553 (DeployBoardViewSet.create; permission_classes=[ProjectMemberPermission])",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/project-deploy-boards/<pk>/",
        gate: Gate::ProjectMember,
        source: "base.py:541 (DeployBoardViewSet: no def retrieve; ModelViewSet default)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/project-deploy-boards/<pk>/",
        gate: Gate::ProjectMember,
        source: "base.py:541 (DeployBoardViewSet: no def partial_update; update default)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/project-deploy-boards/<pk>/",
        gate: Gate::ProjectMember,
        source: "base.py:541 (DeployBoardViewSet: no def destroy; ModelViewSet default)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/archive/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:433 (ProjectArchiveUnarchiveEndpoint.post)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/archive/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "base.py:441 (ProjectArchiveUnarchiveEndpoint.delete)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/preferences/member/<member_id>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:379 (ProjectMemberPreferenceEndpoint.get)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/preferences/member/<member_id>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:367 (ProjectMemberPreferenceEndpoint.patch)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/states/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "state/base.py:84 (StateViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/states/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "state/base.py:49 (StateViewSet.create; + invalidate-before-gate + can_mutate_states)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/states/<pk>/",
        gate: Gate::Authenticated,
        source: "state/base.py (no def retrieve; falls to ModelViewSet default)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/states/<pk>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "state/base.py:66 (StateViewSet.partial_update; + can_mutate_states, no invalidate_cache)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/states/<pk>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "state/base.py:122 (StateViewSet.destroy; + invalidate-before-gate + can_mutate_states)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/intake-state/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "state/base.py:148 (IntakeStateEndpoint.get)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/states/<pk>/mark-default/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "state/base.py:112 (StateViewSet.mark_as_default; + invalidate-before-gate + can_mutate_states)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/project-estimates/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "estimate/base.py:35 (ProjectEstimatePointEndpoint.get)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/estimates/",
        gate: Gate::ProjectEntity,
        source: "estimate/base.py:50,54 (BulkEstimatePointEndpoint.list; permission_classes=[ProjectEntityPermission])",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/estimates/",
        gate: Gate::ProjectEntity,
        source: "estimate/base.py:50,64 (BulkEstimatePointEndpoint.create; + invalidate-after-permission)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/",
        gate: Gate::ProjectEntity,
        source: "estimate/base.py:50,103 (BulkEstimatePointEndpoint.retrieve)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/",
        gate: Gate::ProjectEntity,
        source: "estimate/base.py:50,109 (BulkEstimatePointEndpoint.partial_update; + invalidate-after-permission)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/",
        gate: Gate::ProjectEntity,
        source: "estimate/base.py:50,147 (BulkEstimatePointEndpoint.destroy; + invalidate-after-permission)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "estimate/base.py:154 (EstimatePointEndpoint.create)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/<estimate_point_id>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "estimate/base.py:170 (EstimatePointEndpoint.partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/<estimate_point_id>/",
        gate: Gate::Project {
            roles: ADMIN_MEMBER,
        },
        source: "estimate/base.py:185 (EstimatePointEndpoint.destroy)",
    },
];

/// Outcome of a gate check, before denial rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The handler body runs.
    Allow,
    /// The handler body does not run: answer 403 with [`deny_body`].
    Deny,
    /// Anonymous on a guarded route: never reaches a gate;
    /// Django-session authN + `IsAuthenticated` denies first with the
    /// 401 [`ANON_BODY`]. `AllowAny` rows never yield this.
    Unauthenticated,
}

/// The 403 body when an authenticated caller is denied by `gate`;
/// `None` when the gate never 403s (`AllowAny` never denies at all;
/// `Authenticated` rows only ever deny anonymous callers, with the 401
/// [`ANON_BODY`]). Decorator gates render [`FORBIDDEN_BODY`];
/// permission-class gates render [`CLASS_DENIED_BODY`].
pub fn deny_body(gate: &Gate) -> Option<&'static str> {
    match gate {
        Gate::AllowAny | Gate::Authenticated => None,
        Gate::Workspace { .. } | Gate::Project { .. } => Some(FORBIDDEN_BODY),
        Gate::ProjectMember | Gate::ProjectEntity | Gate::WorkspaceUser => Some(CLASS_DENIED_BODY),
    }
}

fn spec_for(gate: &Gate) -> AllowSpec {
    let level = match gate {
        Gate::Workspace { .. } => AllowLevel::Workspace,
        // `AllowAny`, `Authenticated`, and the class gates never reach
        // the kernel through `decide_gate`; the level below is
        // unreached for them.
        _ => AllowLevel::Project,
    };
    AllowSpec {
        level,
        creator_gate: CreatorGate::App,
        // No D-25 decorator sets `creator=`/`model=`.
        creator_bypass: false,
    }
}

/// Decide one decorator/default gate from pre-fetched membership facts.
///
/// `facts` mirrors one `(user, slug, project_id)` row set: the
/// active-row filters (`is_active=True`) and the `workspace__slug=` /
/// `project_id=` scoping are the caller's SQL (the `tenant_context`
/// half); the scope check denies facts fetched for a different
/// workspace. Class gates ([`Gate::ProjectMember`],
/// [`Gate::ProjectEntity`], [`Gate::WorkspaceUser`]) decide through
/// [`decide_project_member_gate`], [`decide_project_entity_gate`], and
/// [`decide_workspace_user_gate`] instead — they need different facts,
/// so passing one here is a caller bug and fails closed ([`deny_body`]
/// still maps them for the rendering half). The dispatch is the
/// handler's; the table test pins every row's variant.
pub fn decide_gate(gate: &Gate, scope: &TenantScope, facts: &AllowFacts) -> GateOutcome {
    match gate {
        Gate::AllowAny => GateOutcome::Allow,
        Gate::Authenticated => {
            if facts.authenticated {
                GateOutcome::Allow
            } else {
                GateOutcome::Unauthenticated
            }
        }
        Gate::ProjectMember | Gate::ProjectEntity | Gate::WorkspaceUser => {
            if facts.authenticated {
                GateOutcome::Deny
            } else {
                GateOutcome::Unauthenticated
            }
        }
        Gate::Workspace { .. } | Gate::Project { .. } => {
            if !facts.authenticated {
                return GateOutcome::Unauthenticated;
            }
            if decide_allow(&spec_for(gate), scope, facts) {
                GateOutcome::Allow
            } else {
                GateOutcome::Deny
            }
        }
    }
}

/// Decide `ProjectMemberPermission` (`project.py:56-82`, deploy boards)
/// for one method: anonymous 401s; safe methods need any active
/// project row in the workspace (**no `project_id` filter** — handlers
/// fill `is_project_member` for GET/HEAD from the workspace-scoped
/// query); POST needs workspace Admin/Member; anything else needs
/// project Admin/Member. Authenticated denials render
/// [`CLASS_DENIED_BODY`].
pub fn decide_project_member_gate(
    method: &str,
    scope: &TenantScope,
    facts: &ProjectFacts,
) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if decide_project_member(method, scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::Deny
    }
}

/// Decide `ProjectEntityPermission` (`project.py:85-116`, bulk
/// estimates) for one method: anonymous 401s; with a
/// `project_identifier` on the view, safe methods check that
/// identifier's membership (dead in D-25 — no view sets the
/// attribute — but ported per the fixture); otherwise safe methods
/// need project membership and writes need project Admin/Member.
/// Authenticated denials render [`CLASS_DENIED_BODY`].
pub fn decide_project_entity_gate(
    method: &str,
    scope: &TenantScope,
    facts: &ProjectFacts,
) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if decide_project_entity(method, scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::Deny
    }
}

/// Decide `WorkspaceUserPermission` (`workspace.py:103-110`, the
/// project-roles endpoint): anonymous 401s; any active workspace
/// membership passes every method. Authenticated denials render
/// [`CLASS_DENIED_BODY`].
pub fn decide_workspace_user_gate(scope: &TenantScope, facts: &WorkspaceFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if decide_workspace_user(scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::Deny
    }
}

/// Decide the inline `can_mutate_states` check (`project.py:146-184`)
/// on the four state writes (create, partial_update, mark_as_default,
/// destroy): project admins always; members only when the project sets
/// `members_can_edit_states`; workspace admins who hold a membership
/// row. Runs **after** the decorator gate, so anonymous callers never
/// reach it — a `false` here (anonymous included, mirroring the
/// kernel) denies with [`MEMBERS_BLOCKED_BODY`].
pub fn decide_state_mutation(facts: &StateMutationFacts) -> GateOutcome {
    if kernel_can_mutate_states(facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::Deny
    }
}

/// Look up the gate for one route+method; `None` is not a D-25 route.
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

/// `invalidate_cache` path template on the state writes
/// (`state/base.py:48,111,121`).
pub const STATES_CACHE_PATH: &str = "workspaces/:slug/states/";
/// `invalidate_cache` path template on the bulk-estimate writes
/// (`estimate/base.py:63,108,146`).
pub const ESTIMATES_CACHE_PATH: &str = "/api/workspaces/:slug/estimates/";

/// `generate_cache_key` (`utils/cache.py:15-22`): `None` (and the
/// unreachable empty string — Python tests truthiness) yields the bare
/// path, otherwise `"{path}:{auth}"`.
pub fn generate_cache_key(custom_path: &str, auth_header: Option<&str>) -> String {
    match auth_header {
        Some(auth) if !auth.is_empty() => format!("{custom_path}:{auth}"),
        _ => custom_path.to_owned(),
    }
}

/// `url_params` substitution (`utils/cache.py:55-60`): the `:key`
/// placeholders are replaced from the resolver kwargs. Both D-25
/// templates carry only `:slug`, so the general loop collapses to this.
pub fn resolve_cache_key(path_template: &str, slug: &str) -> String {
    path_template.replace(":slug", slug)
}

/// Invalidation key for the state writes: the substituted
/// [`STATES_CACHE_PATH`] with no user suffix (`user=False`).
pub fn states_cache_key(slug: &str) -> String {
    resolve_cache_key(STATES_CACHE_PATH, slug)
}

/// Invalidation key for the bulk-estimate writes: the substituted
/// [`ESTIMATES_CACHE_PATH`] with no user suffix (`user=False`).
pub fn estimates_cache_key(slug: &str) -> String {
    resolve_cache_key(ESTIMATES_CACHE_PATH, slug)
}

/// A state/estimate write that may invalidate a cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAction {
    StateCreate,
    StatePartialUpdate,
    StateMarkDefault,
    StateDestroy,
    EstimateCreate,
    EstimatePartialUpdate,
    EstimateDestroy,
}

/// Where the invalidation runs relative to the authorization check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationOrder {
    /// `@invalidate_cache` sits outside `@allow_permission` (the state
    /// writes): the key is deleted **before** the gate runs, so a 403
    /// still invalidates.
    BeforeGate,
    /// The permission class runs in DRF `initial()` before the wrapped
    /// method (the bulk-estimate writes): a 403 does **not**
    /// invalidate.
    AfterPermissionCheck,
}

/// One invalidation point: which key template a write deletes, and
/// when. Single-key `DEL` in every D-25 case (`multiple=False`).
pub struct CacheInvalidation {
    pub path_template: &'static str,
    pub order: InvalidationOrder,
}

/// The invalidation point for one write, or `None` when the action
/// carries no `invalidate_cache` — exactly one such action exists:
/// state `partial_update` (`state/base.py:66-67`, ported as-is).
pub fn invalidation_for(action: WriteAction) -> Option<CacheInvalidation> {
    match action {
        WriteAction::StateCreate | WriteAction::StateMarkDefault | WriteAction::StateDestroy => {
            Some(CacheInvalidation {
                path_template: STATES_CACHE_PATH,
                order: InvalidationOrder::BeforeGate,
            })
        }
        WriteAction::StatePartialUpdate => None,
        WriteAction::EstimateCreate
        | WriteAction::EstimatePartialUpdate
        | WriteAction::EstimateDestroy => Some(CacheInvalidation {
            path_template: ESTIMATES_CACHE_PATH,
            order: InvalidationOrder::AfterPermissionCheck,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::ProjectId;

    const PROJECTS: &str = "workspaces/<slug>/projects/";
    const DETAILS: &str = "workspaces/<slug>/projects/details/";
    const PROJECT: &str = "workspaces/<slug>/projects/<pk>/";
    const IDENTIFIERS: &str = "workspaces/<slug>/project-identifiers/";
    const INVITES: &str = "workspaces/<slug>/projects/<project_id>/invitations/";
    const INVITE: &str = "workspaces/<slug>/projects/<project_id>/invitations/<pk>/";
    const USER_INVITES: &str = "users/me/workspaces/<slug>/projects/invitations/";
    const ROLES: &str = "users/me/workspaces/<slug>/project-roles/";
    const JOIN: &str = "workspaces/<slug>/projects/<project_id>/join/<pk>/";
    const MEMBERS: &str = "workspaces/<slug>/projects/<project_id>/members/";
    const MEMBER: &str = "workspaces/<slug>/projects/<project_id>/members/<pk>/";
    const LEAVE: &str = "workspaces/<slug>/projects/<project_id>/members/leave/";
    const VIEWS: &str = "workspaces/<slug>/projects/<project_id>/project-views/";
    const ME: &str = "workspaces/<slug>/projects/<project_id>/project-members/me/";
    const FAVS: &str = "workspaces/<slug>/user-favorite-projects/";
    const FAV: &str = "workspaces/<slug>/user-favorite-projects/<project_id>/";
    const BOARDS: &str = "workspaces/<slug>/projects/<project_id>/project-deploy-boards/";
    const BOARD: &str = "workspaces/<slug>/projects/<project_id>/project-deploy-boards/<pk>/";
    const ARCHIVE: &str = "workspaces/<slug>/projects/<project_id>/archive/";
    const PREFS: &str = "workspaces/<slug>/projects/<project_id>/preferences/member/<member_id>/";
    const STATES: &str = "workspaces/<slug>/projects/<project_id>/states/";
    const STATE: &str = "workspaces/<slug>/projects/<project_id>/states/<pk>/";
    const INTAKE_STATE: &str = "workspaces/<slug>/projects/<project_id>/intake-state/";
    const MARK_DEFAULT: &str = "workspaces/<slug>/projects/<project_id>/states/<pk>/mark-default/";
    const PROJ_ESTIMATES: &str = "workspaces/<slug>/projects/<project_id>/project-estimates/";
    const ESTIMATES: &str = "workspaces/<slug>/projects/<project_id>/estimates/";
    const ESTIMATE: &str = "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/";
    const POINTS: &str =
        "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/";
    const POINT: &str = "workspaces/<slug>/projects/<project_id>/estimates/<estimate_id>/estimate-points/<estimate_point_id>/";

    fn scope() -> TenantScope {
        tenant_context("acme")
    }

    /// Facts for an authenticated caller holding `role` in both the
    /// workspace and the project (the contract-suite world), for a
    /// decorator gate with the given allowed-role list.
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

    fn anon_allow() -> AllowFacts {
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

    /// Class-gate facts for an authenticated caller holding `role` in
    /// both the workspace and the project. `ProjectMemberPermission`
    /// SAFE methods read `is_project_member` from the workspace-scoped
    /// (project-unfiltered) query; every other branch filters by
    /// project — the caller fills both from the same active row here.
    fn class_facts_for(role: i32) -> ProjectFacts {
        let senior = role == ROLE_ADMIN || role == ROLE_MEMBER;
        ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated: true,
            is_workspace_member: true,
            has_workspace_admin_or_member: senior,
            is_workspace_admin: role == ROLE_ADMIN,
            is_project_member: true,
            is_project_admin: role == ROLE_ADMIN,
            has_project_admin_or_member: senior,
            has_identifier_membership: false,
            has_project_identifier: false,
        }
    }

    fn ws_facts_for(_role: i32) -> WorkspaceFacts {
        WorkspaceFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            has_admin_or_member_role: true,
            has_admin_role: true,
            is_member: true,
            is_admin_unfiltered: true,
        }
    }

    fn decide_path(method: &str, path: &str, role: Option<i32>) -> GateOutcome {
        let row = gate_for(method, path).expect("fixture route must have a gate");
        match (row.gate, role) {
            (Gate::AllowAny, _) => decide_gate(&row.gate, &scope(), &anon_allow()),
            (_, None) => match row.gate {
                Gate::AllowAny => GateOutcome::Allow,
                Gate::Authenticated
                | Gate::Workspace { .. }
                | Gate::Project { .. }
                | Gate::ProjectMember
                | Gate::ProjectEntity
                | Gate::WorkspaceUser => GateOutcome::Unauthenticated,
            },
            (Gate::Authenticated, Some(_)) => {
                decide_gate(&row.gate, &scope(), &facts_for(ROLE_GUEST, &[]))
            }
            (Gate::Workspace { roles }, Some(role)) | (Gate::Project { roles }, Some(role)) => {
                decide_gate(&row.gate, &scope(), &facts_for(role, roles))
            }
            (Gate::ProjectMember, Some(role)) => {
                decide_project_member_gate(method, &scope(), &class_facts_for(role))
            }
            (Gate::ProjectEntity, Some(role)) => {
                decide_project_entity_gate(method, &scope(), &class_facts_for(role))
            }
            (Gate::WorkspaceUser, Some(role)) => {
                decide_workspace_user_gate(&scope(), &ws_facts_for(role))
            }
        }
    }

    #[test]
    fn table_covers_every_gated_site() {
        assert_eq!(GATES.len(), 54, "29 routes, one row per method+path");
        assert!(gate_for("GET", "nope/").is_none());
        for row in GATES {
            assert!(
                gate_for(row.method, row.path).is_some(),
                "row must round-trip: {} {}",
                row.method,
                row.path
            );
        }
        let count = |pred: fn(&Gate) -> bool| GATES.iter().filter(|row| pred(&row.gate)).count();
        // 28 `@allow_permission` lines over 29 URL paths.
        assert_eq!(
            count(|gate| matches!(gate, Gate::Workspace { .. } | Gate::Project { .. })),
            28,
            "one row per decorator site"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::AllowAny)),
            2,
            "join GET + POST"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::Authenticated)),
            13,
            "auth-only fallthroughs"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::ProjectMember)),
            5,
            "deploy-board actions"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::ProjectEntity)),
            5,
            "bulk-estimate actions"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::WorkspaceUser)),
            1,
            "roles GET"
        );
        // No creator bypass anywhere in D-25.
        for row in GATES {
            if let Gate::Workspace { .. } | Gate::Project { .. } = row.gate {
                assert!(
                    !spec_for(&row.gate).creator_bypass,
                    "{} {}",
                    row.method,
                    row.path
                );
            }
        }
    }

    /// Full FX-APROJ-07 matrix: (method, path, admin, member, guest)
    /// with `true` = handler runs. Roles are held in both the
    /// workspace and the project; the workspace-admin override and the
    /// no-project-membership personas have their own tests.
    const MATRIX: &[(&str, &str, bool, bool, bool)] = &[
        ("GET", PROJECTS, true, true, true),
        ("POST", PROJECTS, true, true, false),
        ("GET", DETAILS, true, true, true),
        ("GET", PROJECT, true, true, true),
        ("PUT", PROJECT, true, true, true),
        ("PATCH", PROJECT, true, true, true),
        ("DELETE", PROJECT, true, true, true),
        ("GET", IDENTIFIERS, true, true, false),
        ("DELETE", IDENTIFIERS, true, true, false),
        ("GET", INVITES, true, true, true),
        ("POST", INVITES, true, false, false),
        ("GET", INVITE, true, true, true),
        ("DELETE", INVITE, true, true, true),
        ("GET", USER_INVITES, true, true, true),
        ("POST", USER_INVITES, true, true, false),
        ("GET", ROLES, true, true, true),
        ("GET", JOIN, true, true, true),
        ("POST", JOIN, true, true, true),
        ("GET", MEMBERS, true, true, true),
        ("POST", MEMBERS, true, false, false),
        ("GET", MEMBER, true, true, true),
        ("PATCH", MEMBER, true, true, true),
        ("DELETE", MEMBER, true, false, false),
        ("POST", LEAVE, true, true, true),
        ("POST", VIEWS, true, true, true),
        ("GET", ME, true, true, true),
        ("GET", FAVS, true, true, true),
        ("POST", FAVS, true, true, true),
        ("DELETE", FAV, true, true, true),
        ("GET", BOARDS, true, true, true),
        ("POST", BOARDS, true, true, false),
        ("GET", BOARD, true, true, true),
        ("PATCH", BOARD, true, true, false),
        ("DELETE", BOARD, true, true, false),
        ("POST", ARCHIVE, true, true, false),
        ("DELETE", ARCHIVE, true, true, false),
        ("GET", PREFS, true, true, true),
        ("PATCH", PREFS, true, true, true),
        ("GET", STATES, true, true, true),
        ("POST", STATES, true, true, false),
        ("GET", STATE, true, true, true),
        ("PATCH", STATE, true, true, false),
        ("DELETE", STATE, true, true, false),
        ("GET", INTAKE_STATE, true, true, true),
        ("POST", MARK_DEFAULT, true, true, false),
        ("GET", PROJ_ESTIMATES, true, true, false),
        ("GET", ESTIMATES, true, true, true),
        ("POST", ESTIMATES, true, true, false),
        ("GET", ESTIMATE, true, true, true),
        ("PATCH", ESTIMATE, true, true, false),
        ("DELETE", ESTIMATE, true, true, false),
        ("POST", POINTS, true, true, false),
        ("PATCH", POINT, true, true, false),
        ("DELETE", POINT, true, true, false),
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
    fn fixture_personas_match_live_captures() {
        // `ws_member_no_project` (workspace MEMBER, no project row):
        // WORKSPACE gates pass, PROJECT gates deny — the override
        // needs a project membership (`base.py:64-77`).
        let no_project = AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        };
        // `ws_admin_project_guest` (workspace ADMIN, project GUEST):
        // passes every decorator gate — the role check where GUEST is
        // listed, the workspace-admin override everywhere else.
        let admin_guest = AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: true,
            is_workspace_admin: true,
        };
        let mut ws_rows = 0;
        let mut project_rows = 0;
        for row in GATES {
            match row.gate {
                Gate::Workspace { .. } => {
                    ws_rows += 1;
                    assert_eq!(
                        decide_gate(&row.gate, &scope(), &no_project),
                        GateOutcome::Allow,
                        "ws MEMBER passes {} {}",
                        row.method,
                        row.path
                    );
                    assert_eq!(
                        decide_gate(&row.gate, &scope(), &admin_guest),
                        GateOutcome::Allow,
                        "ws ADMIN passes {} {}",
                        row.method,
                        row.path
                    );
                }
                Gate::Project { .. } => {
                    project_rows += 1;
                    assert_eq!(
                        decide_gate(&row.gate, &scope(), &no_project),
                        GateOutcome::Deny,
                        "no project row denies {} {}",
                        row.method,
                        row.path
                    );
                    assert_eq!(
                        decide_gate(&row.gate, &scope(), &admin_guest),
                        GateOutcome::Allow,
                        "ws-admin override passes {} {}",
                        row.method,
                        row.path
                    );
                }
                _ => {}
            }
        }
        assert_eq!(ws_rows, 7, "AMG_WS x3 + AM_WS x4");
        assert_eq!(project_rows, 21, "A_P x3 + AM_P x10 + AMG_P x8");
    }

    #[test]
    fn outsider_denies_every_membership_gate() {
        // Authenticated but member of nothing: auth-only and AllowAny
        // rows still run (no membership check); every decorator and
        // class gate denies.
        let outsider = AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: false,
            has_allowed_workspace_role: false,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        };
        let outsider_project = ProjectFacts {
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
        };
        let outsider_ws = WorkspaceFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            has_admin_or_member_role: false,
            has_admin_role: false,
            is_member: false,
            is_admin_unfiltered: false,
        };
        let mut membership_gates = 0;
        for row in GATES {
            let outcome = match row.gate {
                Gate::AllowAny | Gate::Authenticated => decide_gate(&row.gate, &scope(), &outsider),
                Gate::Workspace { .. } | Gate::Project { .. } => {
                    membership_gates += 1;
                    decide_gate(&row.gate, &scope(), &outsider)
                }
                Gate::ProjectMember => {
                    membership_gates += 1;
                    decide_project_member_gate(row.method, &scope(), &outsider_project)
                }
                Gate::ProjectEntity => {
                    membership_gates += 1;
                    decide_project_entity_gate(row.method, &scope(), &outsider_project)
                }
                Gate::WorkspaceUser => {
                    membership_gates += 1;
                    decide_workspace_user_gate(&scope(), &outsider_ws)
                }
            };
            let expected = match row.gate {
                Gate::AllowAny | Gate::Authenticated => GateOutcome::Allow,
                _ => GateOutcome::Deny,
            };
            assert_eq!(outcome, expected, "{} {}", row.method, row.path);
        }
        assert_eq!(membership_gates, 39, "28 decorator + 11 class rows");
    }

    #[test]
    fn anonymous_401s_except_join() {
        // Every route answers 401 before any gate runs — except the
        // two `AllowAny` join actions, where anonymous callers reach
        // the body (fixture: the view's own 404).
        for row in GATES {
            let expected = match row.gate {
                Gate::AllowAny => GateOutcome::Allow,
                _ => GateOutcome::Unauthenticated,
            };
            assert_eq!(
                decide_path(row.method, row.path, None),
                expected,
                "anon {} {}",
                row.method,
                row.path
            );
        }
        // The class fns map anonymous to 401 on their own as well.
        let anon_project = ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated: false,
            is_workspace_member: true,
            has_workspace_admin_or_member: true,
            is_workspace_admin: true,
            is_project_member: true,
            is_project_admin: true,
            has_project_admin_or_member: true,
            has_identifier_membership: true,
            has_project_identifier: true,
        };
        assert_eq!(
            decide_project_member_gate("GET", &scope(), &anon_project),
            GateOutcome::Unauthenticated
        );
        assert_eq!(
            decide_project_entity_gate("GET", &scope(), &anon_project),
            GateOutcome::Unauthenticated
        );
        let anon_ws = WorkspaceFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: false,
            has_admin_or_member_role: true,
            has_admin_role: true,
            is_member: true,
            is_admin_unfiltered: true,
        };
        assert_eq!(
            decide_workspace_user_gate(&scope(), &anon_ws),
            GateOutcome::Unauthenticated
        );
        assert_eq!(
            decide_state_mutation(&StateMutationFacts {
                authenticated: false,
                project_role: Some(ROLE_ADMIN),
                members_can_edit_states: true,
                is_workspace_admin: true,
            }),
            GateOutcome::Deny,
            "unreached in practice (the decorator 401s first); denies like the kernel"
        );
    }

    #[test]
    fn denial_bodies_are_byte_identical() {
        assert_eq!(
            FORBIDDEN_BODY,
            r#"{"error":"You don't have the required permissions."}"#
        );
        assert_eq!(FORBIDDEN_BODY, crate::permissions::PERMISSION_DENIED_BODY);
        assert_eq!(
            CLASS_DENIED_BODY,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(CLASS_DENIED_BODY, crate::permissions::DEFAULT_DENIED_BODY);
        assert_eq!(
            ANON_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            MEMBERS_BLOCKED_BODY,
            r#"{"error":"Members are not permitted to edit workflow states for this project."}"#
        );
        for row in GATES {
            let expected = match row.gate {
                Gate::AllowAny | Gate::Authenticated => None,
                Gate::Workspace { .. } | Gate::Project { .. } => Some(FORBIDDEN_BODY),
                Gate::ProjectMember | Gate::ProjectEntity | Gate::WorkspaceUser => {
                    Some(CLASS_DENIED_BODY)
                }
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
    fn cross_workspace_facts_deny_every_membership_gate() {
        // Tenant-isolation probe, one per membership row: an ADMIN
        // holding valid rows in another workspace denies on every
        // decorator and class gate. Auth-only and AllowAny rows pass
        // any authenticated caller at the gate (isolation there is the
        // handlers' queryset scoping, not a role check).
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
        let other_project = ProjectFacts {
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
            has_project_identifier: true,
        };
        let other_ws = WorkspaceFacts {
            workspace: WorkspaceId::from("other"),
            authenticated: true,
            has_admin_or_member_role: true,
            has_admin_role: true,
            is_member: true,
            is_admin_unfiltered: true,
        };
        let mut membership_gates = 0;
        for row in GATES {
            let outcome = match row.gate {
                Gate::AllowAny | Gate::Authenticated => {
                    decide_gate(&row.gate, &scope(), &other_allow)
                }
                Gate::Workspace { .. } | Gate::Project { .. } => {
                    membership_gates += 1;
                    decide_gate(&row.gate, &scope(), &other_allow)
                }
                Gate::ProjectMember => {
                    membership_gates += 1;
                    decide_project_member_gate(row.method, &scope(), &other_project)
                }
                Gate::ProjectEntity => {
                    membership_gates += 1;
                    decide_project_entity_gate(row.method, &scope(), &other_project)
                }
                Gate::WorkspaceUser => {
                    membership_gates += 1;
                    decide_workspace_user_gate(&scope(), &other_ws)
                }
            };
            let expected = match row.gate {
                Gate::AllowAny | Gate::Authenticated => GateOutcome::Allow,
                _ => GateOutcome::Deny,
            };
            assert_eq!(outcome, expected, "isolation {} {}", row.method, row.path);
        }
        assert_eq!(
            membership_gates, 39,
            "one isolation probe per membership row"
        );
    }

    #[test]
    fn class_gates_through_decide_gate_fail_closed() {
        // Mis-dispatched class gates deny rather than silently
        // passing; handlers must use the class decision fns.
        for gate in [
            Gate::ProjectMember,
            Gate::ProjectEntity,
            Gate::WorkspaceUser,
        ] {
            assert_eq!(
                decide_gate(&gate, &scope(), &facts_for(ROLE_ADMIN, ADMIN)),
                GateOutcome::Deny
            );
        }
    }

    #[test]
    fn state_mutation_matches_can_mutate_states_vectors() {
        fn facts(role: Option<i32>, flag: bool, ws_admin: bool) -> StateMutationFacts {
            StateMutationFacts {
                authenticated: true,
                project_role: role,
                members_can_edit_states: flag,
                is_workspace_admin: ws_admin,
            }
        }
        // members_can_edit_states=True: admin + member pass.
        assert_eq!(
            decide_state_mutation(&facts(Some(ROLE_ADMIN), true, false)),
            GateOutcome::Allow
        );
        assert_eq!(
            decide_state_mutation(&facts(Some(ROLE_MEMBER), true, false)),
            GateOutcome::Allow
        );
        assert_eq!(
            decide_state_mutation(&facts(Some(ROLE_GUEST), true, false)),
            GateOutcome::Deny
        );
        assert_eq!(
            decide_state_mutation(&facts(None, true, false)),
            GateOutcome::Deny
        );
        // Flag off: only admins pass.
        assert_eq!(
            decide_state_mutation(&facts(Some(ROLE_ADMIN), false, false)),
            GateOutcome::Allow
        );
        assert_eq!(
            decide_state_mutation(&facts(Some(ROLE_MEMBER), false, false)),
            GateOutcome::Deny
        );
        // Workspace-admin override still needs a membership row.
        assert_eq!(
            decide_state_mutation(&facts(Some(ROLE_GUEST), false, true)),
            GateOutcome::Allow,
            "ws-admin project guest passes with the flag off"
        );
        assert_eq!(
            decide_state_mutation(&facts(None, false, true)),
            GateOutcome::Deny,
            "inactive (row-less) membership denies before the override"
        );
    }

    #[test]
    fn entity_identifier_branch_is_ported_but_dead_in_d25() {
        // No D-25 view sets `project_identifier`, so handlers always
        // pass `has_project_identifier=false`; the branch below pins
        // the kernel path the fixture records anyway.
        let mut facts = class_facts_for(ROLE_GUEST);
        facts.has_project_identifier = true;
        facts.has_identifier_membership = true;
        facts.is_project_member = false;
        assert_eq!(
            decide_project_entity_gate("GET", &scope(), &facts),
            GateOutcome::Allow,
            "identifier membership passes safe methods"
        );
        assert_eq!(
            decide_project_entity_gate("PATCH", &scope(), &facts),
            GateOutcome::Deny,
            "writes ignore the identifier branch"
        );
    }

    #[test]
    fn member_safe_branch_needs_no_project_filter() {
        // `ProjectMemberPermission` SAFE methods read any active
        // project row in the workspace; the `is_project_member` field
        // below stands for the project-unfiltered fetch the deploy
        // handlers run for GET/HEAD.
        let mut facts = class_facts_for(ROLE_GUEST);
        assert!(facts.is_project_member);
        facts.has_project_admin_or_member = false;
        assert_eq!(
            decide_project_member_gate("GET", &scope(), &facts),
            GateOutcome::Allow
        );
        assert_eq!(
            decide_project_member_gate("DELETE", &scope(), &facts),
            GateOutcome::Deny
        );
    }

    #[test]
    fn cache_keys_match_fixture_goldens() {
        assert_eq!(
            states_cache_key("fx07w-e8e47221"),
            "workspaces/fx07w-e8e47221/states/"
        );
        assert_eq!(
            estimates_cache_key("fx07w-e8e47221"),
            "/api/workspaces/fx07w-e8e47221/estimates/"
        );
        assert_eq!(generate_cache_key("/api/x/", None), "/api/x/");
        assert_eq!(
            generate_cache_key("/api/x/", Some("user-1")),
            "/api/x/:user-1"
        );
        assert_eq!(generate_cache_key("/api/x/", Some("")), "/api/x/");
        assert_eq!(
            generate_cache_key(
                "/api/full/path/?a=1",
                Some("bfa7056d-321c-4620-83f7-aa8030bbc02f")
            ),
            "/api/full/path/?a=1:bfa7056d-321c-4620-83f7-aa8030bbc02f"
        );
        // Anonymous callers get no user suffix even with user=True
        // (`cache.py:63`): the caller passes `None` for them.
        assert_eq!(generate_cache_key("/api/anon/", None), "/api/anon/");
        assert_eq!(
            resolve_cache_key(STATES_CACHE_PATH, "acme"),
            "workspaces/acme/states/"
        );
        assert_eq!(
            resolve_cache_key(ESTIMATES_CACHE_PATH, "acme"),
            "/api/workspaces/acme/estimates/"
        );
    }

    #[test]
    fn invalidation_points_match_decorator_sites() {
        for action in [
            WriteAction::StateCreate,
            WriteAction::StateMarkDefault,
            WriteAction::StateDestroy,
        ] {
            let point = invalidation_for(action).expect("state write invalidates");
            assert_eq!(point.path_template, STATES_CACHE_PATH);
            assert_eq!(point.order, InvalidationOrder::BeforeGate);
        }
        assert!(
            invalidation_for(WriteAction::StatePartialUpdate).is_none(),
            "state partial_update carries no invalidate_cache"
        );
        for action in [
            WriteAction::EstimateCreate,
            WriteAction::EstimatePartialUpdate,
            WriteAction::EstimateDestroy,
        ] {
            let point = invalidation_for(action).expect("estimate write invalidates");
            assert_eq!(point.path_template, ESTIMATES_CACHE_PATH);
            assert_eq!(point.order, InvalidationOrder::AfterPermissionCheck);
        }
    }
}
