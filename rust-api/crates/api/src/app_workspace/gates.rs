//! D-24 permission gates (stage 5, PIDASHCONV-613).
//!
//! Ports the L7 guard layer on all 64 D-24 routes (workspace 45, user 16,
//! api 2, timezone 1 — 108 method+path rows: 31 plain `@allow_permission`
//! rows, 5 creator rows, 27 permission-class rows, 38 `IsAuthenticated`-only
//! fallthroughs, 4 `AllowAny` rows, 2 composed class+decorator rows, and
//! the collection-list KeyError row), from:
//!
//! - `apps/api/pi_dash/app/permissions/base.py:13-16` (`ROLE`),
//!   `:19-87` (`allow_permission`)
//! - `apps/api/pi_dash/app/permissions/workspace.py:12-110` (all six
//!   classes; `WorkspaceUserPermission` has no D-24 `app/views` call
//!   site — ported as absent)
//! - `apps/api/pi_dash/authentication/rate_limit.py:17-47` (throttle
//!   specs; the D-16 kernels in `services/auth_session/guards.rs` are
//!   wired, not re-ported)
//! - `apps/api/pi_dash/utils/cache.py:16-88` (`generate_cache_key`,
//!   `cache_response`, `invalidate_cache_directly` / `invalidate_cache`)
//! - header decorators: `cache_control(private, max_age=12)` +
//!   `vary_on_cookie` (`app/views/user/base.py:75-76,81-82,424-425`),
//!   `gzip_page` (`app/views/workspace/draft.py:97`), `cache_page(2h)`
//!   (`app/views/timezone/base.py:28`)
//!
//! Fixture id: F-W24-13
//! (`rust-api/fixtures/app_workspace/guards/matrix.golden.json` +
//! `TRACE.md`).
//!
//! Shape of the port: the module is pure, like the
//! [`pidash_auth::permissions`] kernel it sits on. Membership facts are
//! caller inputs; row fetching (the `WorkspaceMember ...exists()`
//! queries with their `workspace__slug=` / `is_active=True` filters, and
//! the creator `model.objects.filter(id=pk, created_by=user).exists()`
//! queries) stays with the handlers, which fetch through the
//! workspace-scoped handle — the `tenant_context` half of the pilot
//! pattern. This module only maps each route to its [`Gate`] and decides
//! through the kernel, so the `app`-copy creator membership gate
//! (`app/permissions/base.py:24-38`: non-members refused before the
//! creator bypass) and the deny-by-default tenant scope come along
//! unchanged.
//!
//! Gate order (preserved, not redesigned):
//!
//! - Decorated routes: Django-session authentication (`BaseViewSet` /
//!   `BaseAPIView`: `BaseSessionAuthentication` + `IsAuthenticated`,
//!   `app/views/base.py:84-194`) runs before the decorator, and the
//!   decorator runs before the handler body. Anonymous callers never
//!   reach a gate ([`GateOutcome::Unauthenticated`]).
//! - Class-guarded routes: DRF checks `permission_classes` in
//!   `initial()`, before the handler method runs. Anonymous callers 401
//!   there too: the views carry authenticators but no successful
//!   authenticator, so DRF raises `NotAuthenticated` rather than
//!   `PermissionDenied`.
//! - `AllowAny` routes (both join actions, user session, timezones): no
//!   auth at all; anonymous callers reach the body.
//! - `WorkSpaceViewSet` composes BOTH (`W02` GET, `W03` PATCH/DELETE):
//!   `WorkSpaceBasePermission` runs in `initial()` first, then the
//!   `@allow_permission` decorator. A guest PATCH is refused by the
//!   class ([`CLASS_DENIED_BODY`]) before the decorator runs, while a
//!   member PATCH passes the class and is refused by the decorator
//!   ([`FORBIDDEN_BODY`]). See [`Gate::BaseThenAllow`] and
//!   [`Gate::CollectionList`].
//! - Handler-inline guards (member self/demote/sole-admin 400s,
//!   invite role ceiling, profile role branches — fixture
//!   `inline_guards`) stay in the handler issues: they run inside the
//!   body, after every gate. This table pins the decorator/class half
//!   only.
//!
//! Denial bodies (byte-exact, compact DRF rendering):
//!
//! - Decorator denial: `{"error":"You don't have the required permissions."}`
//!   (403); see [`FORBIDDEN_BODY`].
//! - Permission-class denial: the classes define no `message`, so DRF
//!   renders `PermissionDenied.default_detail`:
//!   `{"detail":"You do not have permission to perform this action."}`
//!   (403); see [`CLASS_DENIED_BODY`].
//! - Anonymous: `{"detail":"Authentication credentials were not provided."}`
//!   (401) on every route except the four `AllowAny` actions; see
//!   [`ANON_BODY`].
//! - Collection list: `{"error":"The required key does not exist."}`
//!   (400, `app/views/base.py:138-143`); see [`MISSING_KEY_BODY`].
//! - Throttled: `{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}`
//!   (429); see [`throttle_denied_json`].
//!
//! Because one composed gate ([`Gate::BaseThenAllow`]) denies with two
//! different 403 bodies depending on which step fails, denial rendering
//! resolves from the [`GateOutcome`], not the [`Gate`]: see
//! [`outcome_body`].
//!
//! Cache-invalidation order (preserved, not redesigned): on member
//! leave the `@invalidate_cache` decorators sit *outside*
//! `@allow_permission`, so the keys are deleted **before** the gate runs
//! — a 403 still invalidates. On join-request approve the Owner class
//! runs in DRF `initial()` *before* the wrapped method, so a 403 does
//! **not** invalidate (same for the bulk-invite create behind the
//! `IsAuthenticated` default). The join post is `AllowAny`: it
//! invalidates unconditionally. See [`invalidations_for`] and
//! [`InvalidationOrder`].
//!
//! Facts-fetch contracts the handlers must honor (the kernel takes
//! booleans; the SQL stays theirs):
//!
//! - Creator rows name their model ([`CreatorModel`]): sticky rows read
//!   `Sticky`, draft destroy reads `DraftIssue`, draft retrieve and
//!   partial_update read **`Issue`** with the draft pk (the dead branch
//!   — ported as-is, handlers must query `Issue`, not `DraftIssue`).
//! - `WorkspaceOwnerPermission` has no `is_active` filter
//!   (`workspace.py:56-58`): handlers fill
//!   `is_admin_unfiltered` from the unfiltered Admin row.
//! - Throttle order is DRF `initial()` order — auth, then permissions,
//!   then throttles: anonymous callers 401 on the generate-code route
//!   before any throttle runs, while `AllowAny` timezone requests reach
//!   the throttle (authenticated callers bypass the anon scope).
//!
//! Out of scope (verified, not ported): `WorkspaceUserPermission` (no
//! D-24 `app/views` call site — used only by `project/member.py:343` +
//! `api/views/sticky.py:28`); DRF `DEFAULT_THROTTLE_CLASSES` (project-wide
//! anon default, cross-cutting like every other domain); DRF's own 405
//! (`Method Not Allowed`) and OPTIONS handling on unlisted methods.
//! Full-dispatch `as_view()` extras (favorite/home/account/token
//! method+path pairs whose handler signature mismatches the URL kwargs)
//! table their gate half here; the post-gate `TypeError` → 500 half
//! belongs to the handlers (each row says so).
//!
//! Ported quirks (translate, don't redesign): the collection-list 400;
//! the draft creator-vs-`Issue` dead branch; sticky creator-only with
//! `roles=[]`; the creator membership gate firing first; the Owner
//! `is_active` omission; the missing-PUT-decorator looseness (member
//! PUT allows, member PATCH denies); the slash-less leave-invalidate
//! path (a key that never matches — ported byte-for-byte); sticky
//! retrieve as the auth-only `ModelViewSet` default.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::permissions::workspace::{
    decide_workspace_admin, decide_workspace_base, decide_workspace_entity, decide_workspace_owner,
    decide_workspace_viewer, WorkspaceFacts,
};
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;
use pidash_types::WorkspaceId;

use pidash_services::auth_session::guards as throttle_kernel;

use crate::assistant::throttles::ThrottleSpec;

/// Exact bytes of the `@allow_permission` 403
/// (`app/permissions/base.py`). Alias of
/// [`crate::permissions::PERMISSION_DENIED_BODY`] so handlers have one home.
pub const FORBIDDEN_BODY: &str = crate::permissions::PERMISSION_DENIED_BODY;
/// Exact bytes of the DRF-default permission-class 403: what the
/// `app/permissions/workspace.py` denials render through
/// `APIView.permission_denied` with `message=None`. Alias of
/// [`crate::permissions::DEFAULT_DENIED_BODY`] so handlers have one home.
pub const CLASS_DENIED_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;
/// Exact bytes of the DRF `IsAuthenticated` / `NotAuthenticated` 401:
/// what anonymous callers get on every D-24 route except the four
/// `AllowAny` actions, before any gate runs.
pub const ANON_BODY: &str = r#"{"detail":"Authentication credentials were not provided."}"#;
/// Exact bytes of the `KeyError` 400 (`app/views/base.py:138-143`):
/// what every authenticated `GET workspaces/` gets, because the list
/// decorator reads the missing `slug` kwarg.
pub const MISSING_KEY_BODY: &str = r#"{"error":"The required key does not exist."}"#;

/// Which table a creator row's `is_creator` fact comes from:
/// `model.objects.filter(id=kwargs["pk"], created_by=user).exists()`
/// (`app/permissions/base.py:36`). The kernel takes the boolean; the
/// handlers' SQL must query the tagged table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatorModel {
    /// `Sticky` (`sticky.py:54,58`): `Sticky.created_by`, auto-set by
    /// `BaseModel.save` (`db/models/base.py:23-38`).
    Sticky,
    /// **`Issue`** (`draft.py:159,186`): the draft pk is looked up in
    /// the `Issue` table, so the branch ~never matches a draft row and
    /// the role check decides. Ported as-is — handlers must query
    /// `Issue`, not `DraftIssue`.
    Issue,
    /// `DraftIssue` (`draft.py:199`): the only consistent creator row.
    DraftIssue,
}

/// How one D-24 route+method authorizes, before any handler logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `permission_classes = [AllowAny]` (both join actions,
    /// `invite.py:151`; user session, `user/base.py:360`; timezones,
    /// `timezone/base.py:24`): no auth, no membership check. Anonymous
    /// callers reach the body; the gate never denies.
    AllowAny,
    /// No decorator and the `IsAuthenticated` default: any signed-in
    /// user reaches the handler body (undecorated user/account/token
    /// endpoints, sticky retrieve, member-me/views, last-visited,
    /// stats/profile/graphs/dashboard). Anonymous denies first with
    /// [`ANON_BODY`].
    Authenticated,
    /// `@allow_permission(..., level="WORKSPACE")` without
    /// `creator`: active workspace membership with a listed role.
    /// Denies with [`FORBIDDEN_BODY`].
    Workspace { roles: &'static [i32] },
    /// `@allow_permission(..., level="WORKSPACE", creator=True,
    /// model=...)`: the `app`-copy creator branch
    /// (`app/permissions/base.py:24-38`) — workspace membership is
    /// checked first, then `model.created_by == user` passes any role;
    /// only non-creators fall to the role check. Denies with
    /// [`FORBIDDEN_BODY`].
    WorkspaceCreator {
        roles: &'static [i32],
        model: CreatorModel,
    },
    /// `permission_classes = [WorkSpaceBasePermission]`
    /// (`workspace.py:19-48`): alone on workspace create (POST→True),
    /// retrieve (SAFE→True, even for non-members — the queryset 404s
    /// them), and PUT update (PUT→Admin/Member). Anonymous denies
    /// first; authenticated denials render [`CLASS_DENIED_BODY`].
    ClassBase,
    /// `permission_classes = [WorkSpaceAdminPermission]`
    /// (`workspace.py:61-71`): active Admin/Member, every method
    /// (invitations, workspace themes). Authenticated denials render
    /// [`CLASS_DENIED_BODY`].
    ClassAdmin,
    /// `permission_classes = [WorkspaceOwnerPermission]`
    /// (`workspace.py:51-58`): role Admin with **no `is_active`
    /// filter** — deactivated admins still pass (ported as-is).
    /// Join-request list/approve/deny. Authenticated denials render
    /// [`CLASS_DENIED_BODY`].
    ClassOwner,
    /// `permission_classes = [WorkspaceEntityPermission]`
    /// (`workspace.py:74-90`): safe methods need any active membership,
    /// writes need Admin/Member (project-members, user-activity, export,
    /// states, estimates). Authenticated denials render
    /// [`CLASS_DENIED_BODY`].
    ClassEntity,
    /// `permission_classes = [WorkspaceViewerPermission]`
    /// (`workspace.py:93-100`): any active membership, every method
    /// (user-issues, labels, user-properties, modules, cycles).
    /// Authenticated denials render [`CLASS_DENIED_BODY`].
    ClassViewer,
    /// PATCH/DELETE on `workspaces/<slug>/`: `WorkSpaceBasePermission`
    /// runs in DRF `initial()` first (PUT/PATCH→Admin/Member,
    /// DELETE→Admin), then the ADMIN `@allow_permission` decorator.
    /// The class step denies with [`CLASS_DENIED_BODY`], the decorator
    /// step with [`FORBIDDEN_BODY`]; see [`decide_base_then_allow`].
    BaseThenAllow { roles: &'static [i32] },
    /// GET `workspaces/`: `WorkSpaceBasePermission` (SAFE→True for any
    /// authenticated caller) runs first, then the decorator reads the
    /// missing `slug` kwarg and `handle_exception` answers the 400
    /// [`MISSING_KEY_BODY`]. Anonymous 401s at the class step.
    CollectionList,
}

/// One row of the F-W24-13 matrix: a route+method and its gate.
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
const CREATOR_ONLY: &[i32] = &[];

/// All 64 D-24 routes, one entry per dispatched method, in URL-file
/// order (`app/urls/workspace.py`, `user.py`, `api.py`, `timezone.py`).
/// Paths omit the `/api` mount prefix, like the sibling gate tables.
pub static GATES: &[RouteGate] = &[
    // --- workspace-slug-check (W01) ---
    RouteGate {
        method: "GET",
        path: "workspace-slug-check/",
        gate: Gate::Authenticated,
        source: "workspace/base.py:243-244 (WorkSpaceAvailabilityCheckEndpoint.get; IsAuthenticated default)",
    },
    // --- workspaces/ (W02) ---
    RouteGate {
        method: "GET",
        path: "workspaces/",
        gate: Gate::CollectionList,
        source: "workspace/base.py:168-170 (WorkSpaceViewSet.list; class SAFE passes, decorator KeyError -> 400 [BUG])",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/",
        gate: Gate::ClassBase,
        source: "workspace/base.py:83 (WorkSpaceViewSet.create; no decorator, class POST -> True)",
    },
    // --- workspaces/<slug>/ (W03) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/",
        gate: Gate::ClassBase,
        source: "workspace/base.py (no def retrieve; class SAFE -> True, queryset 404s non-members)",
    },
    RouteGate {
        method: "PUT",
        path: "workspaces/<slug>/",
        gate: Gate::ClassBase,
        source: "workspace/base.py (no def update; PUT falls to UpdateModelMixin.update, class PUT -> Admin/Member [QUIRK: looser than PATCH])",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/",
        gate: Gate::BaseThenAllow { roles: ADMIN },
        source: "workspace/base.py:172-174 (partial_update; class PUT/PATCH -> Admin/Member, then ADMIN decorator)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/",
        gate: Gate::BaseThenAllow { roles: ADMIN },
        source: "workspace/base.py:183-184 (destroy; class DELETE -> Admin, then ADMIN decorator)",
    },
    // --- invitations (W04-W05) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/invitations/",
        gate: Gate::ClassAdmin,
        source: "invite.py (WorkspaceInvitationsViewset: no def list; WorkSpaceAdminPermission)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/invitations/",
        gate: Gate::ClassAdmin,
        source: "invite.py:53 (WorkspaceInvitationsViewset.create; WorkSpaceAdminPermission)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/invitations/<pk>/",
        gate: Gate::ClassAdmin,
        source: "invite.py (no def retrieve; WorkSpaceAdminPermission)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/invitations/<pk>/",
        gate: Gate::ClassAdmin,
        source: "invite.py (no def partial_update; WorkSpaceAdminPermission)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/invitations/<pk>/",
        gate: Gate::ClassAdmin,
        source: "invite.py:144 (WorkspaceInvitationsViewset.destroy; WorkSpaceAdminPermission)",
    },
    // --- user invitations (W06) ---
    RouteGate {
        method: "GET",
        path: "users/me/workspaces/invitations/",
        gate: Gate::Authenticated,
        source: "invite.py (UserWorkspaceInvitationsViewSet: no def list; IsAuthenticated default)",
    },
    RouteGate {
        method: "POST",
        path: "users/me/workspaces/invitations/",
        gate: Gate::Authenticated,
        source: "invite.py:255 (UserWorkspaceInvitationsViewSet.create; IsAuthenticated default)",
    },
    // --- join (W07) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/invitations/<pk>/join/",
        gate: Gate::AllowAny,
        source: "invite.py:238 (WorkspaceJoinEndpoint.get; permission_classes=[AllowAny])",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/invitations/<pk>/join/",
        gate: Gate::AllowAny,
        source: "invite.py:163 (WorkspaceJoinEndpoint.post; permission_classes=[AllowAny])",
    },
    // --- join requests (W08-W11) ---
    RouteGate {
        method: "GET",
        path: "users/me/workspaces/join-requests/",
        gate: Gate::Authenticated,
        source: "join_request.py (UserWorkspaceJoinRequestViewSet: no def list; IsAuthenticated default)",
    },
    RouteGate {
        method: "POST",
        path: "users/me/workspaces/join-requests/",
        gate: Gate::Authenticated,
        source: "join_request.py:48 (UserWorkspaceJoinRequestViewSet.create; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/join-requests/",
        gate: Gate::ClassOwner,
        source: "join_request.py (WorkspaceJoinRequestViewSet: no def list; WorkspaceOwnerPermission)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/join-requests/<pk>/approve/",
        gate: Gate::ClassOwner,
        source: "join_request.py:187 (WorkspaceJoinRequestViewSet.approve; WorkspaceOwnerPermission)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/join-requests/<pk>/deny/",
        gate: Gate::ClassOwner,
        source: "join_request.py:241 (WorkspaceJoinRequestViewSet.deny; WorkspaceOwnerPermission)",
    },
    // --- members (W12-W15) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/members/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:45-46 (WorkSpaceMemberViewSet.list)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/project-members/",
        gate: Gate::ClassEntity,
        source: "member.py:243 (WorkspaceProjectMemberEndpoint.get; WorkspaceEntityPermission)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/members/<pk>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:57-58 (WorkSpaceMemberViewSet.retrieve)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/members/<pk>/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "member.py:76-77 (WorkSpaceMemberViewSet.partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/members/<pk>/",
        gate: Gate::Workspace { roles: ADMIN },
        source: "member.py:98-99 (WorkSpaceMemberViewSet.destroy)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/members/leave/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "member.py:160-161 (WorkSpaceMemberViewSet.leave; invalidate_cache runs before the gate)",
    },
    // --- last-visited, member-me, views (W16-W18) ---
    RouteGate {
        method: "GET",
        path: "users/last-visited-workspace/",
        gate: Gate::Authenticated,
        source: "workspace/user.py:70 (UserLastProjectWithWorkspaceEndpoint.get; then AttributeError 500 [BUG], handler-owned)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/workspace-members/me/",
        gate: Gate::Authenticated,
        source: "member.py:220 (WorkspaceMemberUserEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/workspace-views/",
        gate: Gate::Authenticated,
        source: "member.py:209 (WorkspaceMemberUserViewsEndpoint.post; IsAuthenticated default)",
    },
    // --- themes (W19-W20) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/workspace-themes/",
        gate: Gate::ClassAdmin,
        source: "workspace/base.py (WorkspaceThemeViewSet: no def list; WorkSpaceAdminPermission)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/workspace-themes/",
        gate: Gate::ClassAdmin,
        source: "workspace/base.py:359 (WorkspaceThemeViewSet.create; WorkSpaceAdminPermission)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/workspace-themes/<pk>/",
        gate: Gate::ClassAdmin,
        source: "workspace/base.py (WorkspaceThemeViewSet: no def retrieve; WorkSpaceAdminPermission)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/workspace-themes/<pk>/",
        gate: Gate::ClassAdmin,
        source: "workspace/base.py (WorkspaceThemeViewSet: no def partial_update; WorkSpaceAdminPermission)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/workspace-themes/<pk>/",
        gate: Gate::ClassAdmin,
        source: "workspace/base.py (WorkspaceThemeViewSet: no def destroy; WorkSpaceAdminPermission)",
    },
    // --- user stats/activity/profile/issues (W21-W25) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-stats/<user_id>/",
        gate: Gate::Authenticated,
        source: "workspace/user.py:398 (WorkspaceUserProfileStatsEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-activity/<user_id>/",
        gate: Gate::ClassEntity,
        source: "workspace/user.py:374 (WorkspaceUserActivityEndpoint.get; WorkspaceEntityPermission)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/user-activity/<user_id>/export/",
        gate: Gate::ClassEntity,
        source: "workspace/base.py:379 (ExportWorkspaceUserActivityEndpoint.post; WorkspaceEntityPermission)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-profile/<user_id>/",
        gate: Gate::Authenticated,
        source: "workspace/user.py:282 (WorkspaceUserProfileEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-issues/<user_id>/",
        gate: Gate::ClassViewer,
        source: "workspace/user.py:136 (WorkspaceUserProfileIssuesEndpoint.get; WorkspaceViewerPermission)",
    },
    // --- labels/states/estimates/modules/cycles + user-properties (W26-W31) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/labels/",
        gate: Gate::ClassViewer,
        source: "label.py:22 (WorkspaceLabelsEndpoint.get; WorkspaceViewerPermission + cache_response 2h)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-properties/",
        gate: Gate::ClassViewer,
        source: "workspace/user.py:270 (WorkspaceUserPropertiesEndpoint.get; WorkspaceViewerPermission)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/user-properties/",
        gate: Gate::ClassViewer,
        source: "workspace/user.py:256 (WorkspaceUserPropertiesEndpoint.patch; WorkspaceViewerPermission)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/states/",
        gate: Gate::ClassEntity,
        source: "state.py:21 (WorkspaceStatesEndpoint.get; WorkspaceEntityPermission)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/estimates/",
        gate: Gate::ClassEntity,
        source: "estimate.py:22 (WorkspaceEstimatesEndpoint.get; WorkspaceEntityPermission + cache_response 2h)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/modules/",
        gate: Gate::ClassViewer,
        source: "module.py:22 (WorkspaceModulesEndpoint.get; WorkspaceViewerPermission)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/cycles/",
        gate: Gate::ClassViewer,
        source: "cycle.py:22 (WorkspaceCyclesEndpoint.get; WorkspaceViewerPermission)",
    },
    // --- favorites (W32-W34; as_view() full dispatch) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-favorites/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:23-24 (WorkspaceFavoriteEndpoint.get)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/user-favorites/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:37-38 (WorkspaceFavoriteEndpoint.post)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/user-favorites/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:69-70 (patch dispatches on the collection path; post-gate TypeError, no favorite_id — handler-owned)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/user-favorites/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:78-79 (delete dispatches on the collection path; post-gate TypeError, no favorite_id — handler-owned)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-favorites/<favorite_id>/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:23-24 (get dispatches on the detail path; post-gate TypeError, unexpected favorite_id — handler-owned)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/user-favorites/<favorite_id>/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:37-38 (post dispatches on the detail path; post-gate TypeError, unexpected favorite_id — handler-owned)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/user-favorites/<favorite_id>/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:69-70 (WorkspaceFavoriteEndpoint.patch)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/user-favorites/<favorite_id>/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:78-79 (WorkspaceFavoriteEndpoint.delete)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/user-favorites/<favorite_id>/group/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "favorite.py:86-87 (WorkspaceFavoriteGroupEndpoint.get)",
    },
    // --- drafts (W35-W37) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/draft-issues/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "draft.py:98-99 (WorkspaceDraftIssueViewSet.list; gzip_page outside the decorator)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/draft-issues/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "draft.py:111-112 (WorkspaceDraftIssueViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/draft-issues/<pk>/",
        gate: Gate::WorkspaceCreator {
            roles: ADMIN,
            model: CreatorModel::Issue,
        },
        source: "draft.py:186-187 (retrieve; ADMIN + creator model=Issue — dead branch, ported as-is)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/draft-issues/<pk>/",
        gate: Gate::WorkspaceCreator {
            roles: ADMIN_MEMBER,
            model: CreatorModel::Issue,
        },
        source: "draft.py:156-162 (partial_update; ADMIN/MEMBER + creator model=Issue — dead branch, ported as-is)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/draft-issues/<pk>/",
        gate: Gate::WorkspaceCreator {
            roles: ADMIN,
            model: CreatorModel::DraftIssue,
        },
        source: "draft.py:199-200 (destroy; ADMIN + creator model=DraftIssue)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/draft-to-issue/<draft_id>/",
        gate: Gate::Workspace { roles: ADMIN_MEMBER },
        source: "draft.py:205-206 (WorkspaceDraftIssueViewSet.create_draft_to_issue)",
    },
    // --- quick links (W38-W39) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/quick-links/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "quick_link.py:60-61 (QuickLinkViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/quick-links/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "quick_link.py:23-24 (QuickLinkViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/quick-links/<pk>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "quick_link.py:45-46 (QuickLinkViewSet.retrieve)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/quick-links/<pk>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "quick_link.py:33-34 (QuickLinkViewSet.partial_update)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/quick-links/<pk>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "quick_link.py:54-55 (QuickLinkViewSet.destroy)",
    },
    // --- home preferences (W40-W41; as_view() full dispatch) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/home-preferences/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "home.py:23-24 (WorkspaceHomePreferenceViewSet.get)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/home-preferences/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "home.py:67-68 (patch dispatches on the collection path; post-gate TypeError, no key — handler-owned)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/home-preferences/<key>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "home.py:23-24 (get dispatches on the key path; post-gate TypeError, unexpected key — handler-owned)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/home-preferences/<key>/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "home.py:67-68 (WorkspaceHomePreferenceViewSet.patch)",
    },
    // --- recent visits (W42) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/recent-visits/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "recent_visit.py:24-25 (UserRecentVisitViewSet.list)",
    },
    // --- stickies (W43-W44) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/stickies/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "sticky.py:40-41 (WorkspaceStickyViewSet.list)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/stickies/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "sticky.py:31-32 (WorkspaceStickyViewSet.create)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/stickies/<pk>/",
        gate: Gate::Authenticated,
        source: "sticky.py (no def retrieve; ModelViewSet default, auth-only)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/stickies/<pk>/",
        gate: Gate::WorkspaceCreator {
            roles: CREATOR_ONLY,
            model: CreatorModel::Sticky,
        },
        source: "sticky.py:54-56 (partial_update; roles=[] + creator model=Sticky — creator-only)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/stickies/<pk>/",
        gate: Gate::WorkspaceCreator {
            roles: CREATOR_ONLY,
            model: CreatorModel::Sticky,
        },
        source: "sticky.py:58-60 (destroy; roles=[] + creator model=Sticky — creator-only)",
    },
    // --- sidebar preferences (W45) ---
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/sidebar-preferences/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "user_preference.py:25-26 (WorkspaceUserPreferenceViewSet.get)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/sidebar-preferences/",
        gate: Gate::Workspace {
            roles: ADMIN_MEMBER_GUEST,
        },
        source: "user_preference.py:81-82 (WorkspaceUserPreferenceViewSet.patch)",
    },
    // --- user identity (U01-U05, U09-U11) ---
    RouteGate {
        method: "GET",
        path: "users/me/",
        gate: Gate::Authenticated,
        source: "user/base.py:77 (UserEndpoint.retrieve; cache_control + vary_on_cookie)",
    },
    RouteGate {
        method: "PATCH",
        path: "users/me/",
        gate: Gate::Authenticated,
        source: "user/base.py:92 (UserEndpoint.partial_update; IsAuthenticated default)",
    },
    RouteGate {
        method: "DELETE",
        path: "users/me/",
        gate: Gate::Authenticated,
        source: "user/base.py:252 (UserEndpoint.deactivate; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "users/session/",
        gate: Gate::AllowAny,
        source: "user/base.py:362 (UserSessionEndpoint.get; permission_classes=[AllowAny])",
    },
    RouteGate {
        method: "GET",
        path: "users/me/settings/",
        gate: Gate::Authenticated,
        source: "user/base.py:83 (UserEndpoint.retrieve_user_settings; cache_control + vary_on_cookie)",
    },
    RouteGate {
        method: "POST",
        path: "users/me/email/generate-code/",
        gate: Gate::Authenticated,
        source: "user/base.py:137 (UserEndpoint.generate_email_verification_code; + EmailVerificationThrottle via get_throttles)",
    },
    RouteGate {
        method: "PATCH",
        path: "users/me/email/",
        gate: Gate::Authenticated,
        source: "user/base.py:176 (UserEndpoint.update_email; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/instance-admin/",
        gate: Gate::Authenticated,
        source: "user/base.py:87 (UserEndpoint.retrieve_instance_admin; IsAuthenticated default)",
    },
    RouteGate {
        method: "PATCH",
        path: "users/me/onboard/",
        gate: Gate::Authenticated,
        source: "user/base.py:374 (UpdateUserOnBoardedEndpoint.patch; IsAuthenticated default)",
    },
    RouteGate {
        method: "PATCH",
        path: "users/me/tour-completed/",
        gate: Gate::Authenticated,
        source: "user/base.py:385 (UpdateUserTourCompletedEndpoint.patch; IsAuthenticated default)",
    },
    // --- profile + accounts (U06-U08; as_view() full dispatch) ---
    RouteGate {
        method: "GET",
        path: "users/me/profile/",
        gate: Gate::Authenticated,
        source: "user/base.py:426 (ProfileEndpoint.get; cache_control + vary_on_cookie)",
    },
    RouteGate {
        method: "PATCH",
        path: "users/me/profile/",
        gate: Gate::Authenticated,
        source: "user/base.py:431 (ProfileEndpoint.patch; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/accounts/",
        gate: Gate::Authenticated,
        source: "user/base.py:407 (AccountEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "DELETE",
        path: "users/me/accounts/",
        gate: Gate::Authenticated,
        source: "user/base.py:417 (delete dispatches on the collection path; post-gate TypeError, no pk — handler-owned)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/accounts/<pk>/",
        gate: Gate::Authenticated,
        source: "user/base.py:407 (AccountEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "DELETE",
        path: "users/me/accounts/<pk>/",
        gate: Gate::Authenticated,
        source: "user/base.py:417 (AccountEndpoint.delete; IsAuthenticated default)",
    },
    // --- activities, workspaces, graphs, dashboard (U12-U16) ---
    RouteGate {
        method: "GET",
        path: "users/me/activities/",
        gate: Gate::Authenticated,
        source: "user/base.py:393 (UserActivityEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/workspaces/",
        gate: Gate::Authenticated,
        source: "workspace/base.py:209 (UserWorkSpacesEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/workspaces/<slug>/activity-graph/",
        gate: Gate::Authenticated,
        source: "workspace/user.py:525 (UserActivityGraphEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/workspaces/<slug>/issues-completed-graph/",
        gate: Gate::Authenticated,
        source: "workspace/user.py:542 (UserIssueCompletedGraphEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "GET",
        path: "users/me/workspaces/<slug>/dashboard/",
        gate: Gate::Authenticated,
        source: "workspace/base.py:263 (UserWorkspaceDashboardEndpoint.get; IsAuthenticated default)",
    },
    // --- api tokens (A01-A02; as_view() full dispatch) ---
    RouteGate {
        method: "GET",
        path: "users/api-tokens/",
        gate: Gate::Authenticated,
        source: "api.py:41 (ApiTokenEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "POST",
        path: "users/api-tokens/",
        gate: Gate::Authenticated,
        source: "api.py:21 (ApiTokenEndpoint.post; IsAuthenticated default)",
    },
    RouteGate {
        method: "PATCH",
        path: "users/api-tokens/",
        gate: Gate::Authenticated,
        source: "api.py:56 (patch dispatches on the collection path; post-gate TypeError, no pk — handler-owned)",
    },
    RouteGate {
        method: "DELETE",
        path: "users/api-tokens/",
        gate: Gate::Authenticated,
        source: "api.py:51 (delete dispatches on the collection path; post-gate TypeError, no pk — handler-owned)",
    },
    RouteGate {
        method: "GET",
        path: "users/api-tokens/<pk>/",
        gate: Gate::Authenticated,
        source: "api.py:41 (ApiTokenEndpoint.get; IsAuthenticated default)",
    },
    RouteGate {
        method: "PATCH",
        path: "users/api-tokens/<pk>/",
        gate: Gate::Authenticated,
        source: "api.py:56 (ApiTokenEndpoint.patch; IsAuthenticated default)",
    },
    RouteGate {
        method: "DELETE",
        path: "users/api-tokens/<pk>/",
        gate: Gate::Authenticated,
        source: "api.py:51 (ApiTokenEndpoint.delete; IsAuthenticated default)",
    },
    // --- timezones (T01) ---
    RouteGate {
        method: "GET",
        path: "timezones/",
        gate: Gate::AllowAny,
        source: "timezone/base.py:27-28 (TimezoneEndpoint.get; AllowAny + AuthenticationThrottle + cache_page 2h)",
    },
];

/// Outcome of a gate check, before denial rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// The handler body runs.
    Allow,
    /// The handler body does not run: answer 403 with
    /// [`FORBIDDEN_BODY`] (the `@allow_permission` decorator refused).
    Deny,
    /// The handler body does not run: answer 403 with
    /// [`CLASS_DENIED_BODY`] (a `permission_classes` entry refused).
    DenyClass,
    /// The collection list only: answer 400 with [`MISSING_KEY_BODY`]
    /// (the decorator read the missing `slug` kwarg).
    MissingSlug,
    /// Anonymous on a guarded route: never reaches a gate;
    /// Django-session authN + `IsAuthenticated` denies first with the
    /// 401 [`ANON_BODY`]. `AllowAny` rows never yield this.
    Unauthenticated,
}

/// The denial body for one outcome, independent of which gate produced
/// it: `None` when the handler runs, otherwise the exact bytes to
/// answer. Rendering resolves here (not per-[`Gate`]) because
/// [`Gate::BaseThenAllow`] denies with two different 403 bodies
/// depending on which step fails.
pub fn outcome_body(outcome: GateOutcome) -> Option<&'static str> {
    match outcome {
        GateOutcome::Allow => None,
        GateOutcome::Deny => Some(FORBIDDEN_BODY),
        GateOutcome::DenyClass => Some(CLASS_DENIED_BODY),
        GateOutcome::MissingSlug => Some(MISSING_KEY_BODY),
        GateOutcome::Unauthenticated => Some(ANON_BODY),
    }
}

fn spec_for(gate: &Gate) -> AllowSpec {
    AllowSpec {
        // Every D-24 decorator passes `level="WORKSPACE"`.
        level: AllowLevel::Workspace,
        // `app/permissions/base.py`: the membership gate fires before
        // the creator bypass.
        creator_gate: CreatorGate::App,
        creator_bypass: matches!(gate, Gate::WorkspaceCreator { .. }),
    }
}

/// Decide one decorator/default gate from pre-fetched membership facts.
///
/// `facts` mirrors one `(user, slug)` row set: the active-row filters
/// (`is_active=True`) and the `workspace__slug=` scoping are the
/// caller's SQL (the `tenant_context` half); the scope check denies
/// facts fetched for a different workspace. On creator rows
/// `is_creator` is the tagged [`CreatorModel`] lookup
/// (`model.objects.filter(id=pk, created_by=user).exists()`).
///
/// Class gates ([`Gate::ClassBase`], [`Gate::ClassAdmin`],
/// [`Gate::ClassOwner`], [`Gate::ClassEntity`], [`Gate::ClassViewer`])
/// decide through [`decide_class_base`], [`decide_class_admin`],
/// [`decide_class_owner`], [`decide_class_entity`], and
/// [`decide_class_viewer`] instead — they need different facts, so
/// passing one here is a caller bug and fails closed. Composed rows
/// ([`Gate::BaseThenAllow`]) decide through
/// [`decide_base_then_allow`]. The dispatch is the handler's; the table
/// test pins every row's variant.
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
        Gate::Workspace { .. } | Gate::WorkspaceCreator { .. } => {
            if !facts.authenticated {
                return GateOutcome::Unauthenticated;
            }
            if decide_allow(&spec_for(gate), scope, facts) {
                GateOutcome::Allow
            } else {
                GateOutcome::Deny
            }
        }
        // The class step passes every authenticated caller on the
        // collection list (SAFE), and the decorator then reads the
        // missing `slug` kwarg unconditionally: no membership row is
        // ever consulted, so the facts beyond `authenticated` are
        // irrelevant and this branch is exact, not fail-closed.
        Gate::CollectionList => {
            if facts.authenticated {
                GateOutcome::MissingSlug
            } else {
                GateOutcome::Unauthenticated
            }
        }
        Gate::ClassBase
        | Gate::ClassAdmin
        | Gate::ClassOwner
        | Gate::ClassEntity
        | Gate::ClassViewer
        | Gate::BaseThenAllow { .. } => {
            if facts.authenticated {
                GateOutcome::Deny
            } else {
                GateOutcome::Unauthenticated
            }
        }
    }
}

/// Decide `WorkSpaceBasePermission` (`workspace.py:19-48`) for one
/// method: anonymous 401s; POST and safe methods pass any login;
/// PUT/PATCH need Admin/Member; DELETE needs Admin. Authenticated
/// denials render [`CLASS_DENIED_BODY`].
pub fn decide_class_base(method: &str, scope: &TenantScope, facts: &WorkspaceFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if decide_workspace_base(method, scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::DenyClass
    }
}

/// Decide `WorkSpaceAdminPermission` (`workspace.py:61-71`): anonymous
/// 401s; active Admin/Member passes every method. Authenticated
/// denials render [`CLASS_DENIED_BODY`].
pub fn decide_class_admin(scope: &TenantScope, facts: &WorkspaceFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if decide_workspace_admin(scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::DenyClass
    }
}

/// Decide `WorkspaceOwnerPermission` (`workspace.py:51-58`): anonymous
/// 401s; role Admin passes with **no `is_active` filter** (handlers
/// fill `is_admin_unfiltered` from the unfiltered row).
/// Authenticated denials render [`CLASS_DENIED_BODY`].
pub fn decide_class_owner(scope: &TenantScope, facts: &WorkspaceFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if decide_workspace_owner(scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::DenyClass
    }
}

/// Decide `WorkspaceEntityPermission` (`workspace.py:74-90`) for one
/// method: anonymous 401s; safe methods need any active membership,
/// writes need Admin/Member. Authenticated denials render
/// [`CLASS_DENIED_BODY`].
pub fn decide_class_entity(
    method: &str,
    scope: &TenantScope,
    facts: &WorkspaceFacts,
) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if decide_workspace_entity(method, scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::DenyClass
    }
}

/// Decide `WorkspaceViewerPermission` (`workspace.py:93-100`):
/// anonymous 401s; any active membership passes every method.
/// Authenticated denials render [`CLASS_DENIED_BODY`].
pub fn decide_class_viewer(scope: &TenantScope, facts: &WorkspaceFacts) -> GateOutcome {
    if !facts.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if decide_workspace_viewer(scope, facts) {
        GateOutcome::Allow
    } else {
        GateOutcome::DenyClass
    }
}

/// Decide the composed PATCH/DELETE gates on `workspaces/<slug>/`
/// ([`Gate::BaseThenAllow`]): `WorkSpaceBasePermission` first (DRF
/// `initial()`), then the ADMIN `@allow_permission` decorator.
/// Anonymous 401s; a class refusal denies with [`CLASS_DENIED_BODY`],
/// a decorator refusal with [`FORBIDDEN_BODY`]. `method` is `"PATCH"`
/// or `"DELETE"` (the class branch differs); `allow` carries the
/// decorator facts (`has_allowed_workspace_role` over the ADMIN list),
/// `class` the class facts — both describe the same caller.
pub fn decide_base_then_allow(
    method: &str,
    scope: &TenantScope,
    allow: &AllowFacts,
    class: &WorkspaceFacts,
) -> GateOutcome {
    if !allow.authenticated {
        return GateOutcome::Unauthenticated;
    }
    if !decide_workspace_base(method, scope, class) {
        return GateOutcome::DenyClass;
    }
    let spec = AllowSpec {
        level: AllowLevel::Workspace,
        creator_gate: CreatorGate::App,
        // Neither composed action sets `creator=`/`model=`.
        creator_bypass: false,
    };
    if decide_allow(&spec, scope, allow) {
        GateOutcome::Allow
    } else {
        GateOutcome::Deny
    }
}

/// Look up the gate for one route+method; `None` is not a D-24 route.
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

// ---------------------------------------------------------------------------
// Throttles (wired, not re-ported: kernels live in D-16
// `services/auth_session/guards.rs`)
// ---------------------------------------------------------------------------

/// `EmailVerificationThrottle`: 3/hour, `UserRateThrottle`
/// (`authentication/rate_limit.py:31-37`). Wired on the generate-code
/// action only, via `UserEndpoint.get_throttles`
/// (`app/views/user/base.py:67-73`).
pub const EMAIL_VERIFICATION_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: throttle_kernel::EMAIL_VERIFICATION_THROTTLE_SCOPE,
    requests: 3,
    window_secs: 3600,
};

/// `AuthenticationThrottle`: 30/minute, `AnonRateThrottle`
/// (`authentication/rate_limit.py:17-19`). Wired on the timezone
/// endpoint via `throttle_classes` (`app/views/timezone/base.py:26`).
pub const AUTHENTICATION_THROTTLE: ThrottleSpec = ThrottleSpec {
    scope: throttle_kernel::AUTHENTICATION_THROTTLE_SCOPE,
    requests: 30,
    window_secs: 60,
};

/// Which identity a wired throttle keys on (`throttling.py`
/// `get_cache_key`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleIdent {
    /// `UserRateThrottle`: the user pk when authenticated, else the IP
    /// (kernel `pidash_services::auth_session::guards::user_cache_key`).
    UserPkOrIp,
    /// `AnonRateThrottle`: authenticated callers bypass (no key — the
    /// kernel `anon_cache_key` returns `None`), anonymous callers key
    /// by IP.
    IpAnonOnly,
}

/// One wired throttle: its spec, its identity rule, and its site.
pub struct ThrottleWiring {
    pub spec: ThrottleSpec,
    pub ident: ThrottleIdent,
    /// Python source of the wiring (not the kernel).
    pub source: &'static str,
}

/// The throttle wired on one route+method, or `None`: exactly two D-24
/// sites wire throttles (verified by grep over the four D-24 view
/// trees). Throttles run in DRF `initial()` order — after auth and
/// permissions — so anonymous callers 401 on the generate-code route
/// before any throttle runs.
pub fn throttle_for(method: &str, path: &str) -> Option<ThrottleWiring> {
    match (method, path) {
        ("POST", "users/me/email/generate-code/") => Some(ThrottleWiring {
            spec: EMAIL_VERIFICATION_THROTTLE,
            ident: ThrottleIdent::UserPkOrIp,
            source:
                "user/base.py:67-73 (get_throttles iff action == generate_email_verification_code)",
        }),
        ("GET", "timezones/") => Some(ThrottleWiring {
            spec: AUTHENTICATION_THROTTLE,
            ident: ThrottleIdent::IpAnonOnly,
            source: "timezone/base.py:26 (throttle_classes=[AuthenticationThrottle])",
        }),
        _ => None,
    }
}

/// Live 429 body: `auth_exception_handler` rewrites DRF's default
/// throttled response to the 5900 error dict (`exception.py:26-31`).
/// Alias of the D-16 kernel rendering so handlers have one home.
pub fn throttle_denied_json() -> String {
    throttle_kernel::throttle_denied_json()
}

// ---------------------------------------------------------------------------
// Cache (`pi_dash/utils/cache.py:16-88`)
// ---------------------------------------------------------------------------

/// `cache_response(60 * 60 * 2)` timeout in seconds
/// (`workspace/label.py:21`, `workspace/estimate.py:21`).
pub const CACHE_RESPONSE_TIMEOUT_SECS: u64 = 7200;
/// `cache_page(60 * 60 * 2)` timeout in seconds
/// (`app/views/timezone/base.py:28`). Spec only: key derivation is
/// Django's `cache_page` machinery (path-based, anonymous-safe).
pub const CACHE_PAGE_TIMEOUT_SECS: u64 = 7200;

/// `generate_cache_key` (`utils/cache.py:16-22`): `None` (and the
/// unreachable empty string — Python tests truthiness) yields the bare
/// path, otherwise `"{path}:{auth}"`.
pub fn generate_cache_key(custom_path: &str, auth_header: Option<&str>) -> String {
    match auth_header {
        Some(auth) if !auth.is_empty() => format!("{custom_path}:{auth}"),
        _ => custom_path.to_owned(),
    }
}

/// `cache_response` key derivation (`utils/cache.py:32-34`): the full
/// request path plus `str(user.id)` for signed-in callers (both D-24
/// sites keep the `user=True` default); anonymous callers get the bare
/// path. A hit returns the cached data+status without running the view
/// (`:37-38`); handlers implement the lookup, this is the key half.
pub fn cache_response_key(full_path: &str, user_id: Option<&str>) -> String {
    generate_cache_key(full_path, user_id)
}

/// `cache_response` store rule (`utils/cache.py:40-45`): only 200s,
/// and never when `DEBUG` is on.
pub fn should_cache_response(status: u16, debug: bool) -> bool {
    status == 200 && !debug
}

/// Whether one route+method carries `@cache_response(2h)`: exactly the
/// labels and estimates GETs (`label.py:21`, `estimate.py:21`).
pub fn cache_response_site(method: &str, path: &str) -> bool {
    matches!(
        (method, path),
        ("GET", "workspaces/<slug>/labels/") | ("GET", "workspaces/<slug>/estimates/")
    )
}

/// `invalidate_cache` path on the join post, bulk-invite create, and
/// join-request approve (`invite.py:154,253`, `join_request.py:179`).
pub const INVALIDATE_WORKSPACES: &str = "/api/workspaces/";
/// `invalidate_cache` path on the same three sites (`invite.py:155,254`,
/// `join_request.py:180`).
pub const INVALIDATE_ME_WORKSPACES: &str = "/api/users/me/workspaces/";
/// `invalidate_cache` path on the join post and member leave
/// (`invite.py:162`, `member.py:158`).
pub const INVALIDATE_ME_SETTINGS: &str = "/api/users/me/settings/";
/// `invalidate_cache` `:slug` template on the join post, member leave,
/// and join-request approve (`invite.py:157`, `member.py:153`,
/// `join_request.py:181-185`).
pub const INVALIDATE_MEMBERS_TEMPLATE: &str = "/api/workspaces/:slug/members/";
/// Member-leave invalidate path (`member.py:159`): missing the leading
/// slash, so the key never matches the cached `/api/...` key. Ported
/// byte-for-byte [BUG].
pub const INVALIDATE_ME_WORKSPACES_NOSLASH: &str = "api/users/me/workspaces/";

/// Where an invalidation runs relative to the authorization check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationOrder {
    /// `@invalidate_cache` sits outside `@allow_permission` (member
    /// leave): the key is deleted **before** the gate runs, so a 403
    /// still invalidates.
    BeforeGate,
    /// The permission check runs in DRF `initial()` before the wrapped
    /// method (join-request approve behind `WorkspaceOwnerPermission`,
    /// bulk-invite create behind the `IsAuthenticated` default): a
    /// 403/401 does **not** invalidate.
    AfterPermissionCheck,
    /// No gate at all (join post behind `AllowAny`): invalidates
    /// unconditionally, before the body.
    NoGate,
}

/// One invalidation point: which key template a write deletes, with
/// which flags, and when.
pub struct CacheInvalidation {
    pub path_template: &'static str,
    pub url_params: bool,
    pub user: bool,
    pub multiple: bool,
    pub order: InvalidationOrder,
}

/// A D-24 write that invalidates cache keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidateAction {
    /// Join post (`invite.py:154-162`): four keys.
    JoinPost,
    /// Bulk-invite create (`invite.py:253-254`): two keys, plus the
    /// per-invite direct call (see [`bulk_create_direct_key`]).
    JoinBulkCreate,
    /// Member leave (`member.py:152-160`): three keys, one slash-less.
    Leave,
    /// Join-request approve (`join_request.py:179-186`): three keys.
    ApproveJoinRequest,
}

/// The `@invalidate_cache` points for one write, top-to-bottom
/// decorator order (call-time order).
pub fn invalidations_for(action: InvalidateAction) -> &'static [CacheInvalidation] {
    match action {
        InvalidateAction::JoinPost => &[
            CacheInvalidation {
                path_template: INVALIDATE_WORKSPACES,
                url_params: false,
                user: false,
                multiple: false,
                order: InvalidationOrder::NoGate,
            },
            CacheInvalidation {
                path_template: INVALIDATE_ME_WORKSPACES,
                url_params: false,
                user: true,
                multiple: true,
                order: InvalidationOrder::NoGate,
            },
            CacheInvalidation {
                path_template: INVALIDATE_MEMBERS_TEMPLATE,
                url_params: true,
                user: false,
                multiple: true,
                order: InvalidationOrder::NoGate,
            },
            CacheInvalidation {
                path_template: INVALIDATE_ME_SETTINGS,
                url_params: false,
                user: true,
                multiple: true,
                order: InvalidationOrder::NoGate,
            },
        ],
        InvalidateAction::JoinBulkCreate => &[
            CacheInvalidation {
                path_template: INVALIDATE_WORKSPACES,
                url_params: false,
                user: false,
                multiple: false,
                order: InvalidationOrder::AfterPermissionCheck,
            },
            CacheInvalidation {
                path_template: INVALIDATE_ME_WORKSPACES,
                url_params: false,
                user: true,
                multiple: true,
                order: InvalidationOrder::AfterPermissionCheck,
            },
        ],
        InvalidateAction::Leave => &[
            CacheInvalidation {
                path_template: INVALIDATE_MEMBERS_TEMPLATE,
                url_params: true,
                user: false,
                multiple: true,
                order: InvalidationOrder::BeforeGate,
            },
            CacheInvalidation {
                path_template: INVALIDATE_ME_SETTINGS,
                url_params: false,
                user: true,
                multiple: false,
                order: InvalidationOrder::BeforeGate,
            },
            CacheInvalidation {
                path_template: INVALIDATE_ME_WORKSPACES_NOSLASH,
                url_params: false,
                user: false,
                multiple: true,
                order: InvalidationOrder::BeforeGate,
            },
        ],
        InvalidateAction::ApproveJoinRequest => &[
            CacheInvalidation {
                path_template: INVALIDATE_WORKSPACES,
                url_params: false,
                user: false,
                multiple: false,
                order: InvalidationOrder::AfterPermissionCheck,
            },
            CacheInvalidation {
                path_template: INVALIDATE_ME_WORKSPACES,
                url_params: false,
                user: true,
                multiple: true,
                order: InvalidationOrder::AfterPermissionCheck,
            },
            CacheInvalidation {
                path_template: INVALIDATE_MEMBERS_TEMPLATE,
                url_params: true,
                user: false,
                multiple: true,
                order: InvalidationOrder::AfterPermissionCheck,
            },
        ],
    }
}

/// `url_params` substitution (`utils/cache.py:55-60`): the `:key`
/// placeholders are replaced from the resolver kwargs. Every D-24
/// template carries only `:slug`, so the general loop collapses to
/// this.
pub fn resolve_invalidation_path(path_template: &str, slug: &str) -> String {
    path_template.replace(":slug", slug)
}

/// Final cache key plus delete mode for one invalidation point
/// (`utils/cache.py:54-69`): `url_params` substitutes `:slug` from the
/// URL kwargs; `user` appends `:{user_id}` for signed-in callers (the
/// decorator path always passes the request); `multiple` deletes by
/// glob (`*{key}*`) instead of a single `DEL`. Returns
/// `(key, multiple)`.
pub fn invalidation_key(
    inv: &CacheInvalidation,
    slug: &str,
    user_id: Option<&str>,
) -> (String, bool) {
    let custom_path = if inv.url_params {
        resolve_invalidation_path(inv.path_template, slug)
    } else {
        inv.path_template.to_owned()
    };
    let auth = if inv.user { user_id } else { None };
    (generate_cache_key(&custom_path, auth), inv.multiple)
}

/// Per-invite direct invalidation inside the bulk-invite create body
/// (`invite.py:263-268`): `f"/api/workspaces/{slug}/members/"` with
/// `user=False`, `multiple=True`, for each accepted invitation's
/// workspace slug. Runs inside the body (after the auth check), once
/// per invite.
pub fn bulk_create_direct_key(slug: &str) -> String {
    resolve_invalidation_path(INVALIDATE_MEMBERS_TEMPLATE, slug)
}

// ---------------------------------------------------------------------------
// Response headers (decorator sites; handlers set the bytes)
// ---------------------------------------------------------------------------

/// `cache_control(private=True, max_age=12)` bytes, verified against
/// live Django 4.2 (`Cache-Control: private, max-age=12`).
pub const CACHE_CONTROL_PRIVATE_12: &str = "private, max-age=12";
/// `vary_on_cookie` bytes (`Vary: Cookie`).
pub const VARY_COOKIE: &str = "Cookie";
/// `gzip_page` vary addition (`Vary: Accept-Encoding`, alongside
/// `Content-Encoding: gzip` when the client accepts it).
pub const VARY_ACCEPT_ENCODING: &str = "Accept-Encoding";

/// Decorator-driven headers for one route+method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteHeaders {
    pub cache_control: Option<&'static str>,
    pub vary: Option<&'static str>,
    /// `gzip_page`: compress when the client sends
    /// `Accept-Encoding: gzip`. Sits outside `@allow_permission`, so it
    /// wraps the 403 too.
    pub gzip: bool,
}

/// Decorator-driven headers for one route+method: `cache_control` +
/// `vary_on_cookie` on me/settings/profile GETs
/// (`user/base.py:75-76,81-82,424-425`), `gzip_page` on the draft list
/// (`draft.py:97`, the only gzip in D-24). Anything else carries no
/// decorator headers.
pub fn headers_for(method: &str, path: &str) -> RouteHeaders {
    const PRIVATE: RouteHeaders = RouteHeaders {
        cache_control: Some(CACHE_CONTROL_PRIVATE_12),
        vary: Some(VARY_COOKIE),
        gzip: false,
    };
    const GZIPPED: RouteHeaders = RouteHeaders {
        cache_control: None,
        vary: Some(VARY_ACCEPT_ENCODING),
        gzip: true,
    };
    const PLAIN: RouteHeaders = RouteHeaders {
        cache_control: None,
        vary: None,
        gzip: false,
    };
    match (method, path) {
        ("GET", "users/me/") | ("GET", "users/me/settings/") | ("GET", "users/me/profile/") => {
            PRIVATE
        }
        ("GET", "workspaces/<slug>/draft-issues/") => GZIPPED,
        _ => PLAIN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Matrix legend: A = Allow, D = Deny (allow-body 403), C =
    // DenyClass (class-body 403), M = MissingSlug (400).
    const A: GateOutcome = GateOutcome::Allow;
    const D: GateOutcome = GateOutcome::Deny;
    const C: GateOutcome = GateOutcome::DenyClass;
    const M: GateOutcome = GateOutcome::MissingSlug;

    fn scope() -> TenantScope {
        tenant_context("acme")
    }

    #[derive(Debug, Clone, Copy)]
    enum Caller {
        Admin,
        Member,
        Guest,
        Outsider,
    }

    const CALLERS: [Caller; 4] = [
        Caller::Admin,
        Caller::Member,
        Caller::Guest,
        Caller::Outsider,
    ];

    /// Decorator facts for one caller against a row's allowed-role
    /// list. Project fields stay false: the WORKSPACE level never
    /// consults them, so a level flip would deny every protected row
    /// and fail the matrix.
    fn allow_facts(caller: Caller, workspace: &str, allowed: &[i32], creator: bool) -> AllowFacts {
        let role = match caller {
            Caller::Admin => Some(ROLE_ADMIN),
            Caller::Member => Some(ROLE_MEMBER),
            Caller::Guest => Some(ROLE_GUEST),
            Caller::Outsider => None,
        };
        AllowFacts {
            workspace: WorkspaceId::from(workspace),
            authenticated: true,
            is_workspace_member: role.is_some(),
            has_allowed_workspace_role: role.is_some_and(|role| allowed.contains(&role)),
            is_creator: creator,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: role == Some(ROLE_ADMIN),
        }
    }

    /// Class facts for one caller: an active member holding the role,
    /// or an authenticated outsider holding nothing.
    fn class_facts(caller: Caller, workspace: &str) -> WorkspaceFacts {
        let (admin_or_member, admin, member) = match caller {
            Caller::Admin => (true, true, true),
            Caller::Member => (true, false, true),
            Caller::Guest => (false, false, true),
            Caller::Outsider => (false, false, false),
        };
        WorkspaceFacts {
            workspace: WorkspaceId::from(workspace),
            authenticated: true,
            has_admin_or_member_role: admin_or_member,
            has_admin_role: admin,
            is_member: member,
            is_admin_unfiltered: admin,
        }
    }

    fn anon_allow(workspace: &str) -> AllowFacts {
        AllowFacts {
            workspace: WorkspaceId::from(workspace),
            authenticated: false,
            is_workspace_member: false,
            has_allowed_workspace_role: false,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        }
    }

    fn anon_class(workspace: &str) -> WorkspaceFacts {
        WorkspaceFacts {
            workspace: WorkspaceId::from(workspace),
            authenticated: false,
            has_admin_or_member_role: false,
            has_admin_role: false,
            is_member: false,
            is_admin_unfiltered: false,
        }
    }

    /// Decide one table row for one caller (`None` = anonymous),
    /// dispatching to the row variant's real decide function — the
    /// dispatch the handlers perform.
    fn decide_path(
        method: &str,
        path: &str,
        caller: Option<Caller>,
        scope: &TenantScope,
    ) -> GateOutcome {
        let row = gate_for(method, path).expect("matrix route must have a gate");
        let ws = "acme";
        match row.gate {
            Gate::AllowAny | Gate::Authenticated | Gate::CollectionList => {
                let facts = match caller {
                    Some(caller) => allow_facts(caller, ws, &[], false),
                    None => anon_allow(ws),
                };
                decide_gate(&row.gate, scope, &facts)
            }
            Gate::Workspace { roles } | Gate::WorkspaceCreator { roles, .. } => {
                let facts = match caller {
                    // Matrix callers are non-creators; the creator
                    // bypass has its own tests.
                    Some(caller) => allow_facts(caller, ws, roles, false),
                    None => anon_allow(ws),
                };
                decide_gate(&row.gate, scope, &facts)
            }
            Gate::ClassBase => {
                let facts = match caller {
                    Some(caller) => class_facts(caller, ws),
                    None => anon_class(ws),
                };
                decide_class_base(method, scope, &facts)
            }
            Gate::ClassAdmin => {
                let facts = match caller {
                    Some(caller) => class_facts(caller, ws),
                    None => anon_class(ws),
                };
                decide_class_admin(scope, &facts)
            }
            Gate::ClassOwner => {
                let facts = match caller {
                    Some(caller) => class_facts(caller, ws),
                    None => anon_class(ws),
                };
                decide_class_owner(scope, &facts)
            }
            Gate::ClassEntity => {
                let facts = match caller {
                    Some(caller) => class_facts(caller, ws),
                    None => anon_class(ws),
                };
                decide_class_entity(method, scope, &facts)
            }
            Gate::ClassViewer => {
                let facts = match caller {
                    Some(caller) => class_facts(caller, ws),
                    None => anon_class(ws),
                };
                decide_class_viewer(scope, &facts)
            }
            Gate::BaseThenAllow { roles } => match caller {
                Some(caller) => decide_base_then_allow(
                    method,
                    scope,
                    &allow_facts(caller, ws, roles, false),
                    &class_facts(caller, ws),
                ),
                None => decide_base_then_allow(method, scope, &anon_allow(ws), &anon_class(ws)),
            },
        }
    }

    #[test]
    fn table_covers_every_gated_site() {
        assert_eq!(GATES.len(), 108, "64 routes, one row per method+path");
        assert!(gate_for("GET", "nope/").is_none());
        assert!(gate_for("POST", "workspaces/<slug>/members/").is_none());
        for row in GATES {
            assert!(
                gate_for(row.method, row.path).is_some(),
                "row must round-trip: {} {}",
                row.method,
                row.path
            );
            assert!(
                !row.source.is_empty(),
                "row must cite its Python source: {} {}",
                row.method,
                row.path
            );
        }
        // Variant distribution: removing or re-typing any gate fails here.
        let count = |pred: fn(&Gate) -> bool| GATES.iter().filter(|row| pred(&row.gate)).count();
        assert_eq!(
            count(|gate| matches!(gate, Gate::AllowAny)),
            4,
            "join GET+POST, session GET, timezones GET"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::Authenticated)),
            38,
            "IsAuthenticated-only fallthroughs"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::Workspace { .. })),
            31,
            "plain WORKSPACE decorator rows"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::WorkspaceCreator { .. })),
            5,
            "draft retrieve/patch/destroy + sticky patch/destroy"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::ClassBase)),
            3,
            "workspace create/retrieve/PUT"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::ClassAdmin)),
            10,
            "invitations + themes"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::ClassOwner)),
            3,
            "join-request list/approve/deny"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::ClassEntity)),
            5,
            "project-members, user-activity, export, states, estimates"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::ClassViewer)),
            6,
            "user-issues, labels, user-properties x2, modules, cycles"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::BaseThenAllow { .. })),
            2,
            "workspace PATCH + DELETE"
        );
        assert_eq!(
            count(|gate| matches!(gate, Gate::CollectionList)),
            1,
            "GET workspaces/"
        );
        // Role-list distribution over the plain WORKSPACE rows.
        let workspace_roles: Vec<&[i32]> = GATES
            .iter()
            .filter_map(|row| match &row.gate {
                Gate::Workspace { roles } => Some(*roles),
                _ => None,
            })
            .collect();
        assert_eq!(
            workspace_roles
                .iter()
                .filter(|roles| **roles == ADMIN_MEMBER_GUEST)
                .count(),
            19,
            "ADMIN/MEMBER/GUEST list-style rows"
        );
        assert_eq!(
            workspace_roles
                .iter()
                .filter(|roles| **roles == ADMIN_MEMBER)
                .count(),
            10,
            "ADMIN/MEMBER favorites/drafts rows"
        );
        assert_eq!(
            workspace_roles
                .iter()
                .filter(|roles| **roles == ADMIN)
                .count(),
            2,
            "ADMIN member-write rows"
        );
        // Creator-model tags: sticky x2, Issue x2 (the quirk), DraftIssue x1.
        let creators: Vec<(&[i32], CreatorModel)> = GATES
            .iter()
            .filter_map(|row| match &row.gate {
                Gate::WorkspaceCreator { roles, model } => Some((*roles, *model)),
                _ => None,
            })
            .collect();
        assert_eq!(
            creators
                .iter()
                .filter(|(_, model)| *model == CreatorModel::Sticky)
                .count(),
            2
        );
        assert_eq!(
            creators
                .iter()
                .filter(|(_, model)| *model == CreatorModel::Issue)
                .count(),
            2,
            "draft retrieve + partial_update query Issue, not DraftIssue"
        );
        assert_eq!(
            creators
                .iter()
                .filter(|(_, model)| *model == CreatorModel::DraftIssue)
                .count(),
            1
        );
        for (roles, model) in &creators {
            if *model == CreatorModel::Sticky {
                assert!(
                    roles.is_empty(),
                    "sticky creator rows carry allowed_roles=[]"
                );
            } else {
                assert!(
                    !roles.is_empty(),
                    "draft creator rows keep their role check"
                );
            }
        }
        // `WorkspaceUserPermission` is ported as absent: no D-24
        // `app/views` call site, so no row may claim it — the enum has
        // no such variant, and the counts above pin every row.
    }

    /// Full F-W24-13 matrix: (method, path, admin, member, guest,
    /// outsider). Creator rows assume non-creators (the bypass has its
    /// own tests). Every denied-permission probe the fixture names is
    /// one `D`/`C` cell here.
    const MATRIX: &[(
        &str,
        &str,
        GateOutcome,
        GateOutcome,
        GateOutcome,
        GateOutcome,
    )] = &[
        ("GET", "workspace-slug-check/", A, A, A, A),
        ("GET", "workspaces/", M, M, M, M),
        ("POST", "workspaces/", A, A, A, A),
        ("GET", "workspaces/<slug>/", A, A, A, A),
        ("PUT", "workspaces/<slug>/", A, A, C, C),
        ("PATCH", "workspaces/<slug>/", A, D, C, C),
        ("DELETE", "workspaces/<slug>/", A, C, C, C),
        ("GET", "workspaces/<slug>/invitations/", A, A, C, C),
        ("POST", "workspaces/<slug>/invitations/", A, A, C, C),
        ("GET", "workspaces/<slug>/invitations/<pk>/", A, A, C, C),
        ("PATCH", "workspaces/<slug>/invitations/<pk>/", A, A, C, C),
        ("DELETE", "workspaces/<slug>/invitations/<pk>/", A, A, C, C),
        ("GET", "users/me/workspaces/invitations/", A, A, A, A),
        ("POST", "users/me/workspaces/invitations/", A, A, A, A),
        (
            "GET",
            "workspaces/<slug>/invitations/<pk>/join/",
            A,
            A,
            A,
            A,
        ),
        (
            "POST",
            "workspaces/<slug>/invitations/<pk>/join/",
            A,
            A,
            A,
            A,
        ),
        ("GET", "users/me/workspaces/join-requests/", A, A, A, A),
        ("POST", "users/me/workspaces/join-requests/", A, A, A, A),
        ("GET", "workspaces/<slug>/join-requests/", A, C, C, C),
        (
            "POST",
            "workspaces/<slug>/join-requests/<pk>/approve/",
            A,
            C,
            C,
            C,
        ),
        (
            "POST",
            "workspaces/<slug>/join-requests/<pk>/deny/",
            A,
            C,
            C,
            C,
        ),
        ("GET", "workspaces/<slug>/members/", A, A, A, D),
        ("GET", "workspaces/<slug>/project-members/", A, A, A, C),
        ("GET", "workspaces/<slug>/members/<pk>/", A, A, A, D),
        ("PATCH", "workspaces/<slug>/members/<pk>/", A, D, D, D),
        ("DELETE", "workspaces/<slug>/members/<pk>/", A, D, D, D),
        ("POST", "workspaces/<slug>/members/leave/", A, A, A, D),
        ("GET", "users/last-visited-workspace/", A, A, A, A),
        ("GET", "workspaces/<slug>/workspace-members/me/", A, A, A, A),
        ("POST", "workspaces/<slug>/workspace-views/", A, A, A, A),
        ("GET", "workspaces/<slug>/workspace-themes/", A, A, C, C),
        ("POST", "workspaces/<slug>/workspace-themes/", A, A, C, C),
        (
            "GET",
            "workspaces/<slug>/workspace-themes/<pk>/",
            A,
            A,
            C,
            C,
        ),
        (
            "PATCH",
            "workspaces/<slug>/workspace-themes/<pk>/",
            A,
            A,
            C,
            C,
        ),
        (
            "DELETE",
            "workspaces/<slug>/workspace-themes/<pk>/",
            A,
            A,
            C,
            C,
        ),
        ("GET", "workspaces/<slug>/user-stats/<user_id>/", A, A, A, A),
        (
            "GET",
            "workspaces/<slug>/user-activity/<user_id>/",
            A,
            A,
            A,
            C,
        ),
        (
            "POST",
            "workspaces/<slug>/user-activity/<user_id>/export/",
            A,
            A,
            C,
            C,
        ),
        (
            "GET",
            "workspaces/<slug>/user-profile/<user_id>/",
            A,
            A,
            A,
            A,
        ),
        (
            "GET",
            "workspaces/<slug>/user-issues/<user_id>/",
            A,
            A,
            A,
            C,
        ),
        ("GET", "workspaces/<slug>/labels/", A, A, A, C),
        ("GET", "workspaces/<slug>/user-properties/", A, A, A, C),
        ("PATCH", "workspaces/<slug>/user-properties/", A, A, A, C),
        ("GET", "workspaces/<slug>/states/", A, A, A, C),
        ("GET", "workspaces/<slug>/estimates/", A, A, A, C),
        ("GET", "workspaces/<slug>/modules/", A, A, A, C),
        ("GET", "workspaces/<slug>/cycles/", A, A, A, C),
        ("GET", "workspaces/<slug>/user-favorites/", A, A, D, D),
        ("POST", "workspaces/<slug>/user-favorites/", A, A, D, D),
        ("PATCH", "workspaces/<slug>/user-favorites/", A, A, D, D),
        ("DELETE", "workspaces/<slug>/user-favorites/", A, A, D, D),
        (
            "GET",
            "workspaces/<slug>/user-favorites/<favorite_id>/",
            A,
            A,
            D,
            D,
        ),
        (
            "POST",
            "workspaces/<slug>/user-favorites/<favorite_id>/",
            A,
            A,
            D,
            D,
        ),
        (
            "PATCH",
            "workspaces/<slug>/user-favorites/<favorite_id>/",
            A,
            A,
            D,
            D,
        ),
        (
            "DELETE",
            "workspaces/<slug>/user-favorites/<favorite_id>/",
            A,
            A,
            D,
            D,
        ),
        (
            "GET",
            "workspaces/<slug>/user-favorites/<favorite_id>/group/",
            A,
            A,
            D,
            D,
        ),
        ("GET", "workspaces/<slug>/draft-issues/", A, A, A, D),
        ("POST", "workspaces/<slug>/draft-issues/", A, A, A, D),
        ("GET", "workspaces/<slug>/draft-issues/<pk>/", A, D, D, D),
        ("PATCH", "workspaces/<slug>/draft-issues/<pk>/", A, A, D, D),
        ("DELETE", "workspaces/<slug>/draft-issues/<pk>/", A, D, D, D),
        (
            "POST",
            "workspaces/<slug>/draft-to-issue/<draft_id>/",
            A,
            A,
            D,
            D,
        ),
        ("GET", "workspaces/<slug>/quick-links/", A, A, A, D),
        ("POST", "workspaces/<slug>/quick-links/", A, A, A, D),
        ("GET", "workspaces/<slug>/quick-links/<pk>/", A, A, A, D),
        ("PATCH", "workspaces/<slug>/quick-links/<pk>/", A, A, A, D),
        ("DELETE", "workspaces/<slug>/quick-links/<pk>/", A, A, A, D),
        ("GET", "workspaces/<slug>/home-preferences/", A, A, A, D),
        ("PATCH", "workspaces/<slug>/home-preferences/", A, A, A, D),
        (
            "GET",
            "workspaces/<slug>/home-preferences/<key>/",
            A,
            A,
            A,
            D,
        ),
        (
            "PATCH",
            "workspaces/<slug>/home-preferences/<key>/",
            A,
            A,
            A,
            D,
        ),
        ("GET", "workspaces/<slug>/recent-visits/", A, A, A, D),
        ("GET", "workspaces/<slug>/stickies/", A, A, A, D),
        ("POST", "workspaces/<slug>/stickies/", A, A, A, D),
        ("GET", "workspaces/<slug>/stickies/<pk>/", A, A, A, A),
        ("PATCH", "workspaces/<slug>/stickies/<pk>/", D, D, D, D),
        ("DELETE", "workspaces/<slug>/stickies/<pk>/", D, D, D, D),
        ("GET", "workspaces/<slug>/sidebar-preferences/", A, A, A, D),
        (
            "PATCH",
            "workspaces/<slug>/sidebar-preferences/",
            A,
            A,
            A,
            D,
        ),
        ("GET", "users/me/", A, A, A, A),
        ("PATCH", "users/me/", A, A, A, A),
        ("DELETE", "users/me/", A, A, A, A),
        ("GET", "users/session/", A, A, A, A),
        ("GET", "users/me/settings/", A, A, A, A),
        ("POST", "users/me/email/generate-code/", A, A, A, A),
        ("PATCH", "users/me/email/", A, A, A, A),
        ("GET", "users/me/instance-admin/", A, A, A, A),
        ("PATCH", "users/me/onboard/", A, A, A, A),
        ("PATCH", "users/me/tour-completed/", A, A, A, A),
        ("GET", "users/me/profile/", A, A, A, A),
        ("PATCH", "users/me/profile/", A, A, A, A),
        ("GET", "users/me/accounts/", A, A, A, A),
        ("DELETE", "users/me/accounts/", A, A, A, A),
        ("GET", "users/me/accounts/<pk>/", A, A, A, A),
        ("DELETE", "users/me/accounts/<pk>/", A, A, A, A),
        ("GET", "users/me/activities/", A, A, A, A),
        ("GET", "users/me/workspaces/", A, A, A, A),
        (
            "GET",
            "users/me/workspaces/<slug>/activity-graph/",
            A,
            A,
            A,
            A,
        ),
        (
            "GET",
            "users/me/workspaces/<slug>/issues-completed-graph/",
            A,
            A,
            A,
            A,
        ),
        ("GET", "users/me/workspaces/<slug>/dashboard/", A, A, A, A),
        ("GET", "users/api-tokens/", A, A, A, A),
        ("POST", "users/api-tokens/", A, A, A, A),
        ("PATCH", "users/api-tokens/", A, A, A, A),
        ("DELETE", "users/api-tokens/", A, A, A, A),
        ("GET", "users/api-tokens/<pk>/", A, A, A, A),
        ("PATCH", "users/api-tokens/<pk>/", A, A, A, A),
        ("DELETE", "users/api-tokens/<pk>/", A, A, A, A),
        ("GET", "timezones/", A, A, A, A),
    ];

    #[test]
    fn matrix_matches_django_allow_deny() {
        assert_eq!(MATRIX.len(), GATES.len());
        for row in GATES {
            assert!(
                MATRIX
                    .iter()
                    .any(|(method, path, _, _, _, _)| *method == row.method && *path == row.path),
                "table row missing from matrix: {} {}",
                row.method,
                row.path
            );
        }
        for (method, path, admin, member, guest, outsider) in MATRIX {
            for (caller, expected) in [
                (Caller::Admin, *admin),
                (Caller::Member, *member),
                (Caller::Guest, *guest),
                (Caller::Outsider, *outsider),
            ] {
                assert_eq!(
                    decide_path(method, path, Some(caller), &scope()),
                    expected,
                    "{method} {path} {caller:?}"
                );
            }
        }
    }

    #[test]
    fn anonymous_never_reaches_a_guarded_gate() {
        // Every guarded route answers 401 before any gate runs; the
        // four AllowAny rows reach the body even anonymously.
        for row in GATES {
            let expected = match row.gate {
                Gate::AllowAny => GateOutcome::Allow,
                _ => GateOutcome::Unauthenticated,
            };
            assert_eq!(
                decide_path(row.method, row.path, None, &scope()),
                expected,
                "anon {} {}",
                row.method,
                row.path
            );
        }
    }

    #[test]
    fn outcome_bodies_are_byte_identical() {
        assert_eq!(
            FORBIDDEN_BODY,
            "{\"error\":\"You don't have the required permissions.\"}"
        );
        assert_eq!(
            CLASS_DENIED_BODY,
            "{\"detail\":\"You do not have permission to perform this action.\"}"
        );
        assert_eq!(
            ANON_BODY,
            "{\"detail\":\"Authentication credentials were not provided.\"}"
        );
        assert_eq!(
            MISSING_KEY_BODY,
            "{\"error\":\"The required key does not exist.\"}"
        );
        assert_eq!(outcome_body(GateOutcome::Allow), None);
        assert_eq!(outcome_body(GateOutcome::Deny), Some(FORBIDDEN_BODY));
        assert_eq!(
            outcome_body(GateOutcome::DenyClass),
            Some(CLASS_DENIED_BODY)
        );
        assert_eq!(
            outcome_body(GateOutcome::MissingSlug),
            Some(MISSING_KEY_BODY)
        );
        assert_eq!(outcome_body(GateOutcome::Unauthenticated), Some(ANON_BODY));
    }

    #[test]
    fn sticky_creator_gate_is_owner_only() {
        // `sticky.py:54-60` + `app/permissions/base.py:24-38`: with
        // `allowed_roles=[]` the role branch can never match — the
        // creator check is the only pass path, for any role.
        for method in ["PATCH", "DELETE"] {
            let row = gate_for(method, "workspaces/<slug>/stickies/<pk>/").expect("sticky row");
            for caller in [Caller::Admin, Caller::Member, Caller::Guest] {
                let mut creator = allow_facts(caller, "acme", &[], true);
                assert_eq!(
                    decide_gate(&row.gate, &scope(), &creator),
                    GateOutcome::Allow,
                    "creator {caller:?} passes {method}"
                );
                // The `app`-copy membership gate fires first: a
                // creator who is not a workspace member is refused
                // without reaching the bypass.
                creator.is_workspace_member = false;
                assert_eq!(
                    decide_gate(&row.gate, &scope(), &creator),
                    GateOutcome::Deny,
                    "non-member creator is refused first on {method}"
                );
            }
            // Non-creator admins deny too: the empty role list matches nothing.
            assert_eq!(
                decide_path(
                    method,
                    "workspaces/<slug>/stickies/<pk>/",
                    Some(Caller::Admin),
                    &scope()
                ),
                GateOutcome::Deny,
                "non-creator admin denies on {method}"
            );
        }
    }

    #[test]
    fn draft_creator_checks_run_before_the_role_check() {
        // `draft.py:156-162,186-187,199-200`: a creator passes
        // regardless of role; only non-creators reach the role list.
        // (The retrieve/partial_update lookups run against `Issue` —
        // the tag test above pins the model; here the bypass order.)
        let cases = [
            ("GET", "workspaces/<slug>/draft-issues/<pk>/", ADMIN),
            (
                "PATCH",
                "workspaces/<slug>/draft-issues/<pk>/",
                ADMIN_MEMBER,
            ),
            ("DELETE", "workspaces/<slug>/draft-issues/<pk>/", ADMIN),
        ];
        for (method, path, allowed) in cases {
            let row = gate_for(method, path).expect("draft row");
            for caller in [Caller::Member, Caller::Guest] {
                let creator = allow_facts(caller, "acme", allowed, true);
                assert_eq!(
                    decide_gate(&row.gate, &scope(), &creator),
                    GateOutcome::Allow,
                    "creator {caller:?} passes {method} {path}"
                );
                let mut outsider = allow_facts(caller, "acme", allowed, true);
                outsider.is_workspace_member = false;
                assert_eq!(
                    decide_gate(&row.gate, &scope(), &outsider),
                    GateOutcome::Deny,
                    "non-member creator is refused first on {method} {path}"
                );
            }
        }
        // Non-creator expectations (the role check decides): member
        // passes PATCH (ADMIN/MEMBER) but denies retrieve (ADMIN-only).
        assert_eq!(
            decide_path(
                "PATCH",
                "workspaces/<slug>/draft-issues/<pk>/",
                Some(Caller::Member),
                &scope()
            ),
            GateOutcome::Allow
        );
        assert_eq!(
            decide_path(
                "GET",
                "workspaces/<slug>/draft-issues/<pk>/",
                Some(Caller::Member),
                &scope()
            ),
            GateOutcome::Deny
        );
    }

    #[test]
    fn composed_workspace_crud_denies_with_both_bodies() {
        // PATCH: the class (PUT/PATCH -> Admin/Member) passes members,
        // the ADMIN decorator refuses them; guests fail at the class.
        assert_eq!(
            decide_path(
                "PATCH",
                "workspaces/<slug>/",
                Some(Caller::Member),
                &scope()
            ),
            GateOutcome::Deny,
            "member PATCH passes the class, fails the decorator"
        );
        assert_eq!(
            decide_path("PATCH", "workspaces/<slug>/", Some(Caller::Guest), &scope()),
            GateOutcome::DenyClass,
            "guest PATCH fails at the class, before the decorator runs"
        );
        // DELETE: the class (DELETE -> Admin) refuses members too.
        for caller in [Caller::Member, Caller::Guest, Caller::Outsider] {
            assert_eq!(
                decide_path("DELETE", "workspaces/<slug>/", Some(caller), &scope()),
                GateOutcome::DenyClass,
                "non-admin DELETE {caller:?} fails at the class"
            );
        }
        assert_eq!(
            decide_path(
                "DELETE",
                "workspaces/<slug>/",
                Some(Caller::Admin),
                &scope()
            ),
            GateOutcome::Allow
        );
        // PUT has no decorator: members pass — full update is looser
        // than partial, ported as-is.
        assert_eq!(
            decide_path("PUT", "workspaces/<slug>/", Some(Caller::Member), &scope()),
            GateOutcome::Allow,
            "member PUT allows while member PATCH denies"
        );
        // Collection list: even outsiders get the 400 — the class
        // passes every login on SAFE, then the decorator KeyErrors.
        for caller in CALLERS {
            assert_eq!(
                decide_path("GET", "workspaces/", Some(caller), &scope()),
                GateOutcome::MissingSlug,
                "list 400s for {caller:?}"
            );
        }
    }

    #[test]
    fn owner_gate_ignores_active_state() {
        // `workspace.py:56-58`: no `is_active` filter — a deactivated
        // admin still passes, unlike every other class.
        let mut deactivated_admin = class_facts(Caller::Admin, "acme");
        deactivated_admin.is_member = false;
        deactivated_admin.has_admin_or_member_role = false;
        deactivated_admin.has_admin_role = false;
        deactivated_admin.is_admin_unfiltered = true;
        assert_eq!(
            decide_class_owner(&scope(), &deactivated_admin),
            GateOutcome::Allow,
            "deactivated admin passes the Owner gate"
        );
        // ...while the same facts fail the Admin gate (active-only).
        assert_eq!(
            decide_class_admin(&scope(), &deactivated_admin),
            GateOutcome::DenyClass
        );
        // And an active guest fails the Owner gate.
        assert_eq!(
            decide_class_owner(&scope(), &class_facts(Caller::Guest, "acme")),
            GateOutcome::DenyClass
        );
    }

    #[test]
    fn cross_workspace_facts_deny_every_protected_action() {
        // Tenant-isolation probe, one per protected row: an ADMIN
        // holding valid rows in another workspace denies on every
        // membership gate. Auth-only and AllowAny rows pass any
        // authenticated caller at the gate (isolation there is the
        // handlers' queryset scoping, not a role check); the
        // collection list 400s (the bug fires before any membership
        // read).
        let other_allow = allow_facts(Caller::Admin, "other", ADMIN_MEMBER_GUEST, true);
        let other_class = class_facts(Caller::Admin, "other");
        let mut protected = 0;
        for row in GATES {
            let outcome = match row.gate {
                Gate::AllowAny | Gate::Authenticated => {
                    decide_gate(&row.gate, &scope(), &other_allow)
                }
                Gate::Workspace { .. } | Gate::WorkspaceCreator { .. } | Gate::CollectionList => {
                    decide_gate(&row.gate, &scope(), &other_allow)
                }
                Gate::ClassBase => decide_class_base(row.method, &scope(), &other_class),
                Gate::ClassAdmin => decide_class_admin(&scope(), &other_class),
                Gate::ClassOwner => decide_class_owner(&scope(), &other_class),
                Gate::ClassEntity => decide_class_entity(row.method, &scope(), &other_class),
                Gate::ClassViewer => decide_class_viewer(&scope(), &other_class),
                Gate::BaseThenAllow { .. } => {
                    decide_base_then_allow(row.method, &scope(), &other_allow, &other_class)
                }
            };
            let expected = match row.gate {
                Gate::AllowAny | Gate::Authenticated => GateOutcome::Allow,
                Gate::CollectionList => GateOutcome::MissingSlug,
                Gate::Workspace { .. } | Gate::WorkspaceCreator { .. } => {
                    protected += 1;
                    GateOutcome::Deny
                }
                Gate::ClassBase
                | Gate::ClassAdmin
                | Gate::ClassOwner
                | Gate::ClassEntity
                | Gate::ClassViewer => {
                    protected += 1;
                    GateOutcome::DenyClass
                }
                Gate::BaseThenAllow { .. } => {
                    protected += 1;
                    GateOutcome::DenyClass
                }
            };
            assert_eq!(outcome, expected, "isolation {} {}", row.method, row.path);
        }
        assert_eq!(
            protected, 65,
            "one isolation probe per protected row (31 decorator + 5 creator + 27 class + 2 composed)"
        );
    }

    #[test]
    fn decide_gate_fails_closed_on_wrong_facts() {
        // Class and composed gates need their own decide functions;
        // reaching them through `decide_gate` with decorator facts is
        // a caller bug and denies (anonymous still 401s).
        let gates = [
            Gate::ClassBase,
            Gate::ClassAdmin,
            Gate::ClassOwner,
            Gate::ClassEntity,
            Gate::ClassViewer,
            Gate::BaseThenAllow { roles: ADMIN },
        ];
        let authed = allow_facts(Caller::Admin, "acme", ADMIN_MEMBER_GUEST, false);
        for gate in gates {
            assert_eq!(
                decide_gate(&gate, &scope(), &authed),
                GateOutcome::Deny,
                "fail-closed {gate:?}"
            );
            assert_eq!(
                decide_gate(&gate, &scope(), &anon_allow("acme")),
                GateOutcome::Unauthenticated,
                "anon {gate:?}"
            );
        }
    }

    #[test]
    fn throttle_specs_match_the_kernels() {
        // Scopes are the kernel consts by construction; quotas must
        // equal the kernel's `parse_rate` of the DRF rate strings.
        assert_eq!(
            throttle_kernel::parse_rate(Some(throttle_kernel::EMAIL_VERIFICATION_THROTTLE_RATE)),
            Some((
                EMAIL_VERIFICATION_THROTTLE.requests,
                EMAIL_VERIFICATION_THROTTLE.window_secs
            )),
        );
        assert_eq!(
            throttle_kernel::parse_rate(Some(throttle_kernel::AUTHENTICATION_THROTTLE_RATE)),
            Some((
                AUTHENTICATION_THROTTLE.requests,
                AUTHENTICATION_THROTTLE.window_secs
            )),
        );
        assert_eq!(EMAIL_VERIFICATION_THROTTLE.requests, 3);
        assert_eq!(EMAIL_VERIFICATION_THROTTLE.window_secs, 3600);
        assert_eq!(AUTHENTICATION_THROTTLE.requests, 30);
        assert_eq!(AUTHENTICATION_THROTTLE.window_secs, 60);
        // Key format agrees with the kernel (`throttle_<scope>_<ident>`).
        assert_eq!(
            throttle_kernel::throttle_cache_key(EMAIL_VERIFICATION_THROTTLE.scope, "u1"),
            "throttle_email_verification_u1",
        );
        // Identity rules: the user throttle keys by pk when authed
        // (IP fallback for anon); the anon throttle bypasses authed
        // callers entirely.
        assert_eq!(
            throttle_kernel::user_cache_key(
                EMAIL_VERIFICATION_THROTTLE.scope,
                Some("u1"),
                "9.9.9.9"
            ),
            "throttle_email_verification_u1",
        );
        assert_eq!(
            throttle_kernel::anon_cache_key(AUTHENTICATION_THROTTLE.scope, true, "9.9.9.9"),
            None,
        );
        assert_eq!(
            throttle_kernel::anon_cache_key(AUTHENTICATION_THROTTLE.scope, false, "9.9.9.9"),
            Some("throttle_authentication_9.9.9.9".to_owned()),
        );
    }

    #[test]
    fn throttle_wiring_covers_exactly_two_sites() {
        let wired: Vec<(&str, &str)> = GATES
            .iter()
            .filter(|row| throttle_for(row.method, row.path).is_some())
            .map(|row| (row.method, row.path))
            .collect();
        assert_eq!(
            wired,
            [
                ("POST", "users/me/email/generate-code/"),
                ("GET", "timezones/"),
            ],
            "generate-code + timezones only; adding a wire must extend this list"
        );
        let email = throttle_for("POST", "users/me/email/generate-code/").expect("email wire");
        assert_eq!(email.spec, EMAIL_VERIFICATION_THROTTLE);
        assert_eq!(email.ident, ThrottleIdent::UserPkOrIp);
        let auth = throttle_for("GET", "timezones/").expect("timezone wire");
        assert_eq!(auth.spec, AUTHENTICATION_THROTTLE);
        assert_eq!(auth.ident, ThrottleIdent::IpAnonOnly);
        // Neighbors wire nothing (the get_throttles branch returns the
        // project-wide defaults there — cross-cutting, out of scope).
        assert!(throttle_for("PATCH", "users/me/email/").is_none());
        assert!(throttle_for("GET", "users/me/").is_none());
        assert!(gate_for("POST", "users/me/email/generate-code/").is_some());
    }

    #[test]
    fn throttle_denial_bytes_match_the_kernel() {
        assert_eq!(
            throttle_denied_json(),
            "{\"error_code\":5900,\"error_message\":\"RATE_LIMIT_EXCEEDED\"}"
        );
    }

    #[test]
    fn cache_response_keys_and_store_rule() {
        assert_eq!(CACHE_RESPONSE_TIMEOUT_SECS, 7200);
        assert_eq!(CACHE_PAGE_TIMEOUT_SECS, 7200);
        // Key derivation: full path + user id; anon gets the bare path.
        assert_eq!(
            cache_response_key("/api/workspaces/acme/labels/", Some("u1")),
            "/api/workspaces/acme/labels/:u1",
        );
        assert_eq!(
            cache_response_key("/api/workspaces/acme/labels/", None),
            "/api/workspaces/acme/labels/",
        );
        assert_eq!(generate_cache_key("/p/", None), "/p/");
        assert_eq!(generate_cache_key("/p/", Some("")), "/p/");
        assert_eq!(generate_cache_key("/p/", Some("u1")), "/p/:u1");
        // Store rule: 200-only, skipped when DEBUG.
        assert!(should_cache_response(200, false));
        assert!(!should_cache_response(200, true));
        assert!(!should_cache_response(201, false));
        assert!(!should_cache_response(403, false));
        assert!(!should_cache_response(404, false));
        // Sites: exactly the labels + estimates GETs.
        assert!(cache_response_site("GET", "workspaces/<slug>/labels/"));
        assert!(cache_response_site("GET", "workspaces/<slug>/estimates/"));
        let sites: Vec<(&str, &str)> = GATES
            .iter()
            .filter(|row| cache_response_site(row.method, row.path))
            .map(|row| (row.method, row.path))
            .collect();
        assert_eq!(
            sites,
            [
                ("GET", "workspaces/<slug>/labels/"),
                ("GET", "workspaces/<slug>/estimates/"),
            ]
        );
        assert!(!cache_response_site("GET", "workspaces/<slug>/states/"));
    }

    #[test]
    fn invalidation_sites_match_the_decorators() {
        // Point counts + orders, top-to-bottom decorator order.
        let join = invalidations_for(InvalidateAction::JoinPost);
        assert_eq!(join.len(), 4);
        assert!(join
            .iter()
            .all(|inv| inv.order == InvalidationOrder::NoGate));
        assert_eq!(join[0].path_template, INVALIDATE_WORKSPACES);
        assert!(!join[0].user && !join[0].multiple && !join[0].url_params);
        assert_eq!(join[1].path_template, INVALIDATE_ME_WORKSPACES);
        assert!(join[1].user && join[1].multiple && !join[1].url_params);
        assert_eq!(join[2].path_template, INVALIDATE_MEMBERS_TEMPLATE);
        assert!(!join[2].user && join[2].multiple && join[2].url_params);
        assert_eq!(join[3].path_template, INVALIDATE_ME_SETTINGS);
        assert!(join[3].user && join[3].multiple && !join[3].url_params);

        let bulk = invalidations_for(InvalidateAction::JoinBulkCreate);
        assert_eq!(bulk.len(), 2);
        assert!(bulk
            .iter()
            .all(|inv| inv.order == InvalidationOrder::AfterPermissionCheck));

        let leave = invalidations_for(InvalidateAction::Leave);
        assert_eq!(leave.len(), 3);
        assert!(leave
            .iter()
            .all(|inv| inv.order == InvalidationOrder::BeforeGate));
        assert_eq!(leave[0].path_template, INVALIDATE_MEMBERS_TEMPLATE);
        assert!(leave[0].url_params && !leave[0].user && leave[0].multiple);
        assert_eq!(leave[1].path_template, INVALIDATE_ME_SETTINGS);
        assert!(leave[1].user && !leave[1].multiple && !leave[1].url_params);
        assert_eq!(leave[2].path_template, INVALIDATE_ME_WORKSPACES_NOSLASH);

        let approve = invalidations_for(InvalidateAction::ApproveJoinRequest);
        assert_eq!(approve.len(), 3);
        assert!(approve
            .iter()
            .all(|inv| inv.order == InvalidationOrder::AfterPermissionCheck));

        // `:slug` substitution.
        assert_eq!(
            resolve_invalidation_path(INVALIDATE_MEMBERS_TEMPLATE, "acme"),
            "/api/workspaces/acme/members/"
        );
        assert_eq!(
            bulk_create_direct_key("acme"),
            "/api/workspaces/acme/members/"
        );

        // Final keys: url_params + user flags compose per
        // `invalidate_cache_directly`.
        let (key, multiple) = invalidation_key(&join[2], "acme", Some("u1"));
        assert_eq!(key, "/api/workspaces/acme/members/");
        assert!(multiple);
        let (key, multiple) = invalidation_key(&join[1], "acme", Some("u1"));
        assert_eq!(key, "/api/users/me/workspaces/:u1");
        assert!(multiple);
        let (key, multiple) = invalidation_key(&leave[1], "acme", Some("u1"));
        assert_eq!(key, "/api/users/me/settings/:u1");
        assert!(!multiple);
        // The slash-less leave path, byte-for-byte: a key that never
        // matches anything cached. Ported as-is.
        let (key, multiple) = invalidation_key(&leave[2], "acme", Some("u1"));
        assert_eq!(key, "api/users/me/workspaces/");
        assert!(multiple);
        assert_eq!(INVALIDATE_ME_WORKSPACES_NOSLASH, "api/users/me/workspaces/");
    }

    #[test]
    fn header_sites_carry_the_decorator_bytes() {
        assert_eq!(CACHE_CONTROL_PRIVATE_12, "private, max-age=12");
        assert_eq!(VARY_COOKIE, "Cookie");
        assert_eq!(VARY_ACCEPT_ENCODING, "Accept-Encoding");
        for path in ["users/me/", "users/me/settings/", "users/me/profile/"] {
            assert_eq!(
                headers_for("GET", path),
                RouteHeaders {
                    cache_control: Some(CACHE_CONTROL_PRIVATE_12),
                    vary: Some(VARY_COOKIE),
                    gzip: false,
                },
                "{path}"
            );
        }
        assert_eq!(
            headers_for("GET", "workspaces/<slug>/draft-issues/"),
            RouteHeaders {
                cache_control: None,
                vary: Some(VARY_ACCEPT_ENCODING),
                gzip: true,
            },
        );
        // Every other table row carries no decorator headers.
        let plain = RouteHeaders {
            cache_control: None,
            vary: None,
            gzip: false,
        };
        for row in GATES {
            let expected = match (row.method, row.path) {
                ("GET", "users/me/")
                | ("GET", "users/me/settings/")
                | ("GET", "users/me/profile/") => headers_for(row.method, row.path),
                ("GET", "workspaces/<slug>/draft-issues/") => headers_for(row.method, row.path),
                _ => plain,
            };
            assert_eq!(
                headers_for(row.method, row.path),
                expected,
                "headers {} {}",
                row.method,
                row.path
            );
        }
        // PATCH on the header sites carries no decorators (GET-only).
        assert_eq!(headers_for("PATCH", "users/me/"), plain);
        assert_eq!(
            headers_for("POST", "workspaces/<slug>/draft-issues/"),
            plain
        );
    }
}
