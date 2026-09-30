//! D-21 api-v1 permission + throttle gates (stage 5, PIDASHCONV-415).
//!
//! Ports four units for the api-v1 assets / stickies / intake surface:
//!
//! 1. Sticky `WorkspaceUserPermission` — member-of-workspace gate for all 5
//!    `ModelViewSet` actions (`apps/api/pi_dash/api/views/sticky.py:24-28`,
//!    class `app/permissions/workspace.py:103-110`).
//! 2. Intake `ProjectLitePermission` — project-membership gate for
//!    list/create/retrieve/update/delete
//!    (`apps/api/pi_dash/api/views/intake.py:60,225`, class
//!    `app/permissions/project.py:133-143`).
//! 3. Asset base stack — `APIKeyAuthentication` + `IsAuthenticated` +
//!    `ApiKeyRateThrottle` (default) / `ServiceTokenRateThrottle`
//!    (service-token path); `BaseViewSet` carries the same auth with the
//!    subclass permission appended
//!    (`apps/api/pi_dash/api/views/base.py:100-131,223-231`,
//!    `apps/api/pi_dash/api/middleware/api_authentication.py:20-80`,
//!    `apps/api/pi_dash/api/rate_limit.py:12-70`).
//! 4. Intake inline role matrix — patch 400, guest whitelist, `role > 15`
//!    intake attrs, delete cascade + 403
//!    (`apps/api/pi_dash/api/views/intake.py:328-341,373-392,474-490`).
//!
//! Fixture: `rust-api/fixtures/v1_assets/fx-perm.json` (`fx-perm`; trace:
//! `rust-api/fixtures/v1_assets/TRACE.md`). The `#[cfg(test)]` suite replays
//! every golden row, so removing any gate below breaks the build.
//!
//! Shape of the port: the class-gate kernels live in the read-only F-06
//! foundation (`pidash_auth::permissions::{project,workspace}`); this module
//! only pins which gate each D-21 route carries ([`gate_for`]), the throttle
//! selection ([`throttle_for`]) with its rates and header names, and the
//! view-inline role matrix as pure functions over caller-fetched facts.
//! Row fetching stays with the handler layer (issues 419/421/423/426), which
//! must honor the fetch-scoping traps documented on [`decide_sticky_gate`]
//! and [`decide_intake_gate`].
//!
//! Gate order (preserved, not redesigned): DRF `initial()` runs API-key
//! authentication (`api/views/base.py:101`, `BaseViewSet:226`), then the
//! slug→UUID rewrite (`base.py:52-104`, skipped for anonymous callers so
//! slugs cannot be probed via 404-vs-401), then `check_permissions`, then
//! `check_throttles`, then the handler body. Anonymous callers therefore 401
//! on every D-21 route and never reach a gate.
//!
//! Response shapes (handlers render these; recorded here so the matrix has
//! one home):
//!
//! * Anonymous on any D-21 route: 401 `{"detail":"Authentication
//!   credentials were not provided."}` — the auth layer answers before any
//!   gate runs.
//! * Denied member: 403 [`CLASS_DENIAL_BODY`] — the DRF-default
//!   `PermissionDenied` body. Neither class sets `message`, so every class
//!   denial renders through `APIView.permission_denied` with `message=None`
//!   (verified: no `message` attribute in either permissions file).
//! * Intake patch by a low-role non-author: 400 [`EDIT_DENIED_BODY`].
//! * Intake delete by a non-creator non-admin when the issue cascades: 403
//!   [`DELETE_DENIED_BODY`].
//!
//! Ported quirks (translate, don't redesign — fixture `bugs`):
//!
//! * BUG-1 the patch non-author denial is 400, not 403
//!   (`views/intake.py:337-341`). [`patch_edit_gate`] pins the 400.
//! * BUG-2 intake attributes from `role <= 15` callers are silently dropped
//!   (no serializer built, still 200) rather than rejected
//!   (`views/intake.py:387-392`). [`intake_fields_writable`] pins the
//!   `role > 15` boundary; the silent drop is the handler's `if` shape.
//! * BUG-3 an accepted (`status == 1`) intake issue deletes WITHOUT the
//!   creator/admin check (`views/intake.py:475-493`: the guard sits inside
//!   the `status in [-2,-1,0,2]` branch only). [`delete_guard`] applies only
//!   when [`destroy_cascades_to_issue`] is true; the status-1 path deletes
//!   the intake row with no guard.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52` (zero drift
//! Ported-from→HEAD on all D-21 permission and view sources, verified
//! 2026-09-30).

use pidash_auth::permissions::project::{decide_project_lite, ProjectFacts};
use pidash_auth::permissions::workspace::{decide_workspace_user, WorkspaceFacts};
use pidash_auth::permissions::{ROLE_GUEST, ROLE_MEMBER};
use pidash_auth::scope::TenantScope;

/// The DRF-default permission-denied body every D-21 class denial renders.
///
/// Byte-exact: `{"detail":"You do not have permission to perform this
/// action."}` (compact separators, `rest_framework` defaults). Alias of
/// [`crate::permissions::DEFAULT_DENIED_BODY`] so handlers have one home.
pub const CLASS_DENIAL_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;

/// Byte-exact patch denial for a low-role non-author (`views/intake.py:338`):
/// `{"error":"You cannot edit intake work items"}` with status 400.
/// Single key, so insertion order is trivially preserved.
pub const EDIT_DENIED_BODY: &str = r#"{"error":"You cannot edit intake work items"}"#;

/// Byte-exact delete denial when the issue cascades
/// (`views/intake.py:486`):
/// `{"error":"Only admin or creator can delete the work item"}` with
/// status 403.
pub const DELETE_DENIED_BODY: &str =
    r#"{"error":"Only admin or creator can delete the work item"}"#;

/// `ROLE` values (`app/permissions/base.py:13-16`), re-exported from the
/// F-06 kernel so handlers compare against one home.
pub use pidash_auth::permissions::{
    ROLE_ADMIN as ADMIN_ROLE, ROLE_GUEST as GUEST_ROLE, ROLE_MEMBER as MEMBER_ROLE,
};

// ---------------------------------------------------------------------------
// Route → gate table
// ---------------------------------------------------------------------------

/// One D-21 api-v1 route family (methods vary per route; the class gate does
/// not branch on method — both classes ignore it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1AssetsRoute {
    /// `StickyViewSet` (`api/views/sticky.py:24`): all 5 `ModelViewSet`
    /// actions (list/create/retrieve/update/destroy) carry
    /// `WorkspaceUserPermission` (`sticky.py:28`).
    Sticky,
    /// `IntakeIssueListCreateAPIEndpoint` (`api/views/intake.py:60`):
    /// list + create carry `ProjectLitePermission`.
    IntakeListCreate,
    /// `IntakeIssueDetailAPIEndpoint` (`api/views/intake.py:225`):
    /// retrieve + update + delete carry `ProjectLitePermission`.
    IntakeDetail,
    /// `UserAssetEndpoint` (`api/views/asset.py:48`): no
    /// `permission_classes` — base `IsAuthenticated` only.
    UserAsset,
    /// `UserServerAssetEndpoint` (`api/views/asset.py:246`): no
    /// `permission_classes` — base `IsAuthenticated` only.
    UserServerAsset,
    /// `GenericAssetEndpoint` (`api/views/asset.py:403`): no
    /// `permission_classes` — base `IsAuthenticated` only.
    GenericAsset,
}

/// Which permission class guards a D-21 route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V1AssetsGate {
    /// `WorkspaceUserPermission` (`app/permissions/workspace.py:103-110`):
    /// any active workspace membership, every method.
    WorkspaceUser,
    /// `ProjectLitePermission` (`app/permissions/project.py:133-143`):
    /// any active project membership, every method.
    ProjectLite,
    /// No `permission_classes`: base `IsAuthenticated` only. Any
    /// authenticated caller passes.
    AuthOnly,
}

/// Map a D-21 route to its gate, exactly as the view classes declare.
///
/// The asset views (`asset.py:48,246,403`) declare no `permission_classes`,
/// so they fall back to the base-class `IsAuthenticated`.
pub fn gate_for(route: V1AssetsRoute) -> V1AssetsGate {
    match route {
        V1AssetsRoute::Sticky => V1AssetsGate::WorkspaceUser,
        V1AssetsRoute::IntakeListCreate | V1AssetsRoute::IntakeDetail => V1AssetsGate::ProjectLite,
        V1AssetsRoute::UserAsset | V1AssetsRoute::UserServerAsset | V1AssetsRoute::GenericAsset => {
            V1AssetsGate::AuthOnly
        }
    }
}

/// Decide a D-21 route: `AuthOnly` passes any authenticated caller; the
/// class gates delegate to the F-06 kernels. Anonymous callers match no
/// membership row, so the kernels deny them — the handler layer maps that
/// denial to 401 (never reached the gate) versus 403.
pub fn check_route_gate(
    route: V1AssetsRoute,
    scope: &TenantScope,
    workspace: &WorkspaceFacts,
    project: &ProjectFacts,
) -> bool {
    match gate_for(route) {
        V1AssetsGate::AuthOnly => workspace.authenticated || project.authenticated,
        V1AssetsGate::WorkspaceUser => decide_sticky_gate(scope, workspace),
        V1AssetsGate::ProjectLite => decide_intake_gate(scope, project),
    }
}

// ---------------------------------------------------------------------------
// Class gates (units 1–2)
// ---------------------------------------------------------------------------

/// Sticky gate: `WorkspaceUserPermission.has_permission`
/// (`app/permissions/workspace.py:103-110`) — anonymous refuses first, then
/// `WorkspaceMember(member=user, workspace__slug=view.workspace_slug,
/// is_active=True).exists()`.
///
/// Fetch-scoping trap for handlers: the membership row MUST be fetched with
/// the `(member, workspace__slug, is_active)` filters — `view.workspace_slug`
/// is `self.kwargs.get("slug")` (`api/views/base.py:199-201`). Fetching
/// without the slug (or without `is_active`) widens the gate beyond what
/// Python checks. The scope check (`scope_allows`) is the kernel's share of
/// the same trap: facts fetched for another workspace deny here.
pub fn decide_sticky_gate(scope: &TenantScope, facts: &WorkspaceFacts) -> bool {
    decide_workspace_user(scope, facts)
}

/// Intake gate: `ProjectLitePermission.has_permission`
/// (`app/permissions/project.py:133-143`) — anonymous refuses first, then
/// `ProjectMember(workspace__slug=view.workspace_slug, member=user,
/// project_id=view.project_id, is_active=True).exists()`.
///
/// Fetch-scoping trap for handlers: the membership row MUST be fetched with
/// all four filters — `(workspace__slug, member, project_id, is_active)`.
/// `view.workspace_slug` is `self.kwargs.get("slug")` and `view.project_id`
/// is `self.kwargs.get("project_id")` (`api/views/base.py:199-208`).
/// Dropping `project_id` would port the D-19 BUG-7 shape, which does NOT
/// exist here: this class filters on `project_id`, so a membership in
/// another project of the same workspace denies.
pub fn decide_intake_gate(scope: &TenantScope, facts: &ProjectFacts) -> bool {
    decide_project_lite(scope, facts)
}

// ---------------------------------------------------------------------------
// Base stack (unit 3): authentication + throttles
// ---------------------------------------------------------------------------

/// `authentication_classes = [APIKeyAuthentication]`
/// (`api/views/base.py:101`, `BaseViewSet:226`): the `X-Api-Key` header
/// carries an `APIToken` row (unexpired, active) or, for `mt_`-prefixed
/// tokens, a `MachineToken` row
/// (`api/middleware/api_authentication.py:20-80`).
pub const API_KEY_HEADER: &str = "X-Api-Key";

/// Detail of the `AuthenticationFailed` raised for an unknown, expired,
/// inactive, or revoked token (`api_authentication.py:39,53-62`): DRF
/// renders it as `{"detail": <this>}`. The status is DRF's auth-failure
/// shape, owned by the auth layer — recorded here only so the matrix has
/// one home for the string.
pub const INVALID_TOKEN_DETAIL: &str = "Given API token is not valid";

/// Which throttle `BaseAPIView.get_throttles` returns
/// (`api/views/base.py:118-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleClass {
    /// Default: `ApiKeyRateThrottle` — no `X-Api-Key` header, or the key is
    /// not a service token.
    ApiKey,
    /// `ServiceTokenRateThrottle` — the header matches an `APIToken` row
    /// with `is_service=True` (`base.py:123`).
    ServiceToken,
}

/// Mirror `get_throttles` (`base.py:118-131`): `api_key` is the `X-Api-Key`
/// header value (`None` when absent); `service_token_found` is
/// `APIToken.filter(token=api_key, is_service=True).first()` existing.
/// The lookup itself stays handler-owned (explicit request context); the
/// branch is pinned here.
pub fn throttle_for(api_key: Option<&str>, service_token_found: bool) -> ThrottleClass {
    match api_key {
        Some(_) if service_token_found => ThrottleClass::ServiceToken,
        _ => ThrottleClass::ApiKey,
    }
}

/// DRF `scope` of `ApiKeyRateThrottle` (`api/rate_limit.py:13`).
pub const API_KEY_THROTTLE_SCOPE: &str = "api_key";
/// DRF `scope` of `ServiceTokenRateThrottle` (`api/rate_limit.py:54`).
pub const SERVICE_TOKEN_THROTTLE_SCOPE: &str = "service_token";
/// Default rate of `ApiKeyRateThrottle`: `60/minute`, overridable via the
/// `API_KEY_RATE_LIMIT` env var (`api/rate_limit.py:14`).
pub const API_KEY_THROTTLE_REQUESTS_PER_MINUTE: u32 = 60;
/// Name of the env var overriding the API-key rate (`rate_limit.py:14`).
pub const API_KEY_RATE_LIMIT_ENV: &str = "API_KEY_RATE_LIMIT";
/// Rate of `ServiceTokenRateThrottle`: `300/minute` (`rate_limit.py:55`).
pub const SERVICE_TOKEN_THROTTLE_REQUESTS_PER_MINUTE: u32 = 300;
/// Rate window in seconds (`/minute`).
pub const THROTTLE_WINDOW_SECS: u64 = 60;

/// Mirror `get_cache_key` (`rate_limit.py:16-22,57-63`): no header (or an
/// empty one — falsy in Python, `if not api_key`) yields `None`, and DRF
/// skips throttles with a `None` key (`SimpleRateThrottle.allow_request`
/// returns `True`, the identity path in the fixture). Otherwise the key is
/// `"<scope>:<api_key>"`.
pub fn throttle_cache_key(scope: &str, api_key: Option<&str>) -> Option<String> {
    match api_key {
        None | Some("") => None,
        Some(key) => Some(format!("{scope}:{key}")),
    }
}

/// `scope` for a [`ThrottleClass`] (the cache-key namespace).
pub fn throttle_scope(class: ThrottleClass) -> &'static str {
    match class {
        ThrottleClass::ApiKey => API_KEY_THROTTLE_SCOPE,
        ThrottleClass::ServiceToken => SERVICE_TOKEN_THROTTLE_SCOPE,
    }
}

/// Request-`META` keys the throttles set on allow (`rate_limit.py:40-41`),
/// copied to the response by `BaseAPIView.finalize_response`
/// (`api/views/base.py:184-197`). META key and response header share the
/// same spelling.
pub const RATE_LIMIT_REMAINING_HEADER: &str = "X-RateLimit-Remaining";
/// See [`RATE_LIMIT_REMAINING_HEADER`].
pub const RATE_LIMIT_RESET_HEADER: &str = "X-RateLimit-Reset";

// ---------------------------------------------------------------------------
// Intake inline role matrix (unit 4)
// ---------------------------------------------------------------------------

/// Denial marker for the patch low-role gate: status 400 with
/// [`EDIT_DENIED_BODY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchDeny;

impl PatchDeny {
    /// `status.HTTP_400_BAD_REQUEST` (`views/intake.py:340`).
    pub const STATUS: u16 = 400;
    /// [`EDIT_DENIED_BODY`].
    pub const BODY: &'static str = EDIT_DENIED_BODY;
}

/// Denial marker for the delete creator/admin guard: status 403 with
/// [`DELETE_DENIED_BODY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteDeny;

impl DeleteDeny {
    /// `status.HTTP_403_FORBIDDEN` (`views/intake.py:487`).
    pub const STATUS: u16 = 403;
    /// [`DELETE_DENIED_BODY`].
    pub const BODY: &'static str = DELETE_DENIED_BODY;
}

/// Patch low-role gate (`views/intake.py:336-341`): `project_member.role <=
/// 5` (numeric — catches `GUEST` and any lower custom role) and the caller
/// is not the intake issue's creator (`str(created_by_id) != str(user.id)`)
/// answers 400 [`EDIT_DENIED_BODY`]. BUG-1 ported as written: the denial is
/// 400, not 403.
///
/// `role` is the caller's active `ProjectMember.role`
/// (`workspace__slug, project_id, member, is_active` row,
/// `views/intake.py:329-334`); `is_author` compares the resolved string ids.
pub fn patch_edit_gate(role: i32, is_author: bool) -> Result<(), PatchDeny> {
    if role <= ROLE_GUEST && !is_author {
        return Err(PatchDeny);
    }
    Ok(())
}

/// Keys a `role <= 5` caller may write inside `issue_data`
/// (`views/intake.py:373-380`); every other key is silently dropped. The
/// `description_json` value itself falls back through
/// `issue_data.description or issue_data.description_json or {}` (`:374`) —
/// the fallback chain is the handler's shape; the key set is pinned here.
pub const GUEST_ISSUE_KEYS: [&str; 3] = ["name", "description_html", "description_json"];

/// Whether the caller's `issue_data` is narrowed to [`GUEST_ISSUE_KEYS`]
/// (`views/intake.py:373`): any project membership at or below `GUEST`.
pub fn guest_issue_narrowed(role: i32) -> bool {
    role <= ROLE_GUEST
}

/// Whether the caller may write intake-issue attributes
/// (`views/intake.py:387`): `project_member.role > 15` builds the
/// `IntakeIssueUpdateSerializer`. BUG-2 ported as written: `role <= 15`
/// builds no serializer and the request still answers 200 with the intake
/// fields silently ignored.
pub fn intake_fields_writable(role: i32) -> bool {
    role > ROLE_MEMBER
}

/// Whether deleting the intake issue also deletes the parent `Issue`
/// (`views/intake.py:474`): only statuses `-2, -1, 0, 2` cascade; any other
/// status (including accepted `1`) deletes just the intake row.
pub fn destroy_cascades_to_issue(status: i32) -> bool {
    matches!(status, -2 | -1 | 0 | 2)
}

/// Delete creator/admin guard (`views/intake.py:478-489`): when the issue
/// cascades, the delete proceeds only for the issue's creator
/// (`issue.created_by_id == user.id`) or an active `role == 20` project
/// member (`workspace__slug, member, role=20, project_id, is_active` row);
/// otherwise 403 [`DELETE_DENIED_BODY`]. BUG-3 ported as written: handlers
/// call this ONLY inside the [`destroy_cascades_to_issue`] branch — the
/// `status == 1` path deletes the intake row with no guard.
///
/// `is_admin` is the `role == ROLE_ADMIN` row existing; the comparison is an
/// exact `role=20` filter (`:483`), not `>=`.
pub fn delete_guard(is_creator: bool, is_admin: bool) -> Result<(), DeleteDeny> {
    if is_creator || is_admin {
        return Ok(());
    }
    Err(DeleteDeny)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::{ProjectId, WorkspaceId};
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/v1_assets/fx-perm.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fx-perm exists"))
            .expect("fx-perm parses")
    }

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn ws_facts(authenticated: bool, is_member: bool) -> WorkspaceFacts {
        WorkspaceFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated,
            has_admin_or_member_role: false,
            has_admin_role: false,
            is_member,
            is_admin_unfiltered: false,
        }
    }

    fn project_facts(authenticated: bool, is_member: bool) -> ProjectFacts {
        ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated,
            is_workspace_member: false,
            has_workspace_admin_or_member: false,
            is_workspace_admin: false,
            is_project_member: is_member,
            is_project_admin: false,
            has_project_admin_or_member: false,
            has_identifier_membership: false,
            has_project_identifier: false,
        }
    }

    fn golden_out(action: &str) -> Value {
        fixture()
            .get("goldens")
            .and_then(Value::as_array)
            .expect("goldens array")
            .iter()
            .find(|row| {
                row.get("actor")
                    .and_then(Value::as_str)
                    .is_some_and(|a| a.contains(action))
            })
            .unwrap_or_else(|| panic!("fx-perm lacks golden {action}"))
            .get("out")
            .expect("out")
            .clone()
    }

    #[test]
    fn fixture_id_and_bugs_match() {
        let fx = fixture();
        assert_eq!(
            fx.get("fixture_id").and_then(Value::as_str),
            Some("fx-perm")
        );
        // The three ported quirks are recorded in-file; the module docs pin
        // the same three. A gate removal that also drops a bug changes this.
        assert_eq!(
            fx.get("bugs").and_then(Value::as_array).map(Vec::len),
            Some(3)
        );
        // Both verbatim deny bodies appear in the goldens: the fixture is
        // pretty-printed JSON, so compact-serialize the golden bodies and
        // compare bytes — this is the render parity the handlers must keep.
        let edit = golden_out("role<=5 non-author");
        assert_eq!(
            serde_json::to_string(edit.get("body").expect("body")).expect("compact"),
            EDIT_DENIED_BODY,
            "400 body golden"
        );
        let delete = golden_out("non-creator non-admin DELETE");
        assert_eq!(
            serde_json::to_string(delete.get("body").expect("body")).expect("compact"),
            DELETE_DENIED_BODY,
            "403 body golden"
        );
    }

    #[test]
    fn role_values_match_python() {
        // `app/permissions/base.py:13-16`, via the F-06 kernel re-export.
        assert_eq!(ADMIN_ROLE, 20);
        assert_eq!(MEMBER_ROLE, 15);
        assert_eq!(GUEST_ROLE, 5);
    }

    #[test]
    fn class_denial_is_drf_default() {
        // Neither class sets `message`, so denials render through
        // `APIView.permission_denied` with `message=None`: compact
        // separators, no spaces.
        assert_eq!(
            CLASS_DENIAL_BODY,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(CLASS_DENIAL_BODY, crate::permissions::DEFAULT_DENIED_BODY);
    }

    #[test]
    fn inline_denies_are_byte_identical() {
        assert_eq!(
            EDIT_DENIED_BODY,
            r#"{"error":"You cannot edit intake work items"}"#
        );
        assert_eq!(PatchDeny::STATUS, 400);
        assert_eq!(PatchDeny::BODY, EDIT_DENIED_BODY);
        assert_eq!(
            DELETE_DENIED_BODY,
            r#"{"error":"Only admin or creator can delete the work item"}"#
        );
        assert_eq!(DeleteDeny::STATUS, 403);
        assert_eq!(DeleteDeny::BODY, DELETE_DENIED_BODY);
        // Fixture goldens carry the same bodies verbatim.
        assert_eq!(
            golden_out("role<=5 non-author"),
            serde_json::json!({"body": {"error": "You cannot edit intake work items"}, "status": 400})
        );
        assert_eq!(
            golden_out("non-creator non-admin DELETE"),
            serde_json::json!({"body": {"error": "Only admin or creator can delete the work item"}, "status": 403})
        );
    }

    #[test]
    fn gate_table_pins_every_route() {
        // Removing or rewiring any gate breaks this table.
        assert_eq!(gate_for(V1AssetsRoute::Sticky), V1AssetsGate::WorkspaceUser);
        assert_eq!(
            gate_for(V1AssetsRoute::IntakeListCreate),
            V1AssetsGate::ProjectLite
        );
        assert_eq!(
            gate_for(V1AssetsRoute::IntakeDetail),
            V1AssetsGate::ProjectLite
        );
        assert_eq!(gate_for(V1AssetsRoute::UserAsset), V1AssetsGate::AuthOnly);
        assert_eq!(
            gate_for(V1AssetsRoute::UserServerAsset),
            V1AssetsGate::AuthOnly
        );
        assert_eq!(
            gate_for(V1AssetsRoute::GenericAsset),
            V1AssetsGate::AuthOnly
        );
    }

    #[test]
    fn sticky_member_gate() {
        // Fixture goldens: workspace member (active) → allow; non-member →
        // deny; anonymous → deny (`app/permissions/workspace.py:103-110`).
        assert!(decide_sticky_gate(&scope(), &ws_facts(true, true)));
        assert!(!decide_sticky_gate(&scope(), &ws_facts(true, false)));
        assert!(!decide_sticky_gate(&scope(), &ws_facts(false, true)));
        assert!(!decide_sticky_gate(&scope(), &ws_facts(false, false)));
    }

    #[test]
    fn sticky_gate_is_workspace_scoped() {
        // Facts fetched for another workspace deny even with membership.
        let other = TenantScope::new(WorkspaceId::from("other"));
        assert!(!decide_sticky_gate(&other, &ws_facts(true, true)));
    }

    #[test]
    fn intake_member_gate() {
        // Fixture goldens: project member (active, any role) → allow;
        // workspace member without a project row → deny; anonymous → deny
        // (`app/permissions/project.py:133-143`).
        assert!(decide_intake_gate(&scope(), &project_facts(true, true)));
        assert!(!decide_intake_gate(&scope(), &project_facts(true, false)));
        assert!(!decide_intake_gate(&scope(), &project_facts(false, true)));
    }

    #[test]
    fn intake_gate_is_project_scoped() {
        // Same-workspace membership in another project is a different facts
        // row (`project_id` filter): no project row here → deny. This is
        // the anti-BUG-7 pin: dropping `project_id` would allow this.
        let other_project = ProjectFacts {
            project_id: ProjectId::from("p-2"),
            ..project_facts(true, false)
        };
        assert!(!decide_intake_gate(&scope(), &other_project));
        let other_ws = TenantScope::new(WorkspaceId::from("other"));
        assert!(!decide_intake_gate(&other_ws, &project_facts(true, true)));
    }

    #[test]
    fn route_check_needs_authn_on_auth_only() {
        // Asset views (`asset.py:48,246,403`) carry no permission class: any
        // signed-in caller passes, anonymous 401s before the gate.
        for route in [
            V1AssetsRoute::UserAsset,
            V1AssetsRoute::UserServerAsset,
            V1AssetsRoute::GenericAsset,
        ] {
            assert!(check_route_gate(
                route,
                &scope(),
                &ws_facts(true, false),
                &project_facts(true, false)
            ));
            assert!(!check_route_gate(
                route,
                &scope(),
                &ws_facts(false, false),
                &project_facts(false, false)
            ));
        }
        // Class-gated routes still deny authed non-members.
        assert!(!check_route_gate(
            V1AssetsRoute::Sticky,
            &scope(),
            &ws_facts(true, false),
            &project_facts(true, true)
        ));
        assert!(!check_route_gate(
            V1AssetsRoute::IntakeDetail,
            &scope(),
            &ws_facts(true, true),
            &project_facts(true, false)
        ));
    }

    #[test]
    fn throttle_selection_mirrors_get_throttles() {
        // Fixture `base_stack.rule` (`views/base.py:118-131`).
        assert_eq!(throttle_for(None, false), ThrottleClass::ApiKey);
        assert_eq!(throttle_for(Some("k"), false), ThrottleClass::ApiKey);
        // A header that is not a service token still takes the default.
        assert_eq!(throttle_for(Some("k"), false), ThrottleClass::ApiKey);
        assert_eq!(throttle_for(Some("svc"), true), ThrottleClass::ServiceToken);
        // No header means no service row to find.
        assert_eq!(throttle_for(None, true), ThrottleClass::ApiKey);
        assert_eq!(throttle_scope(ThrottleClass::ApiKey), "api_key");
        assert_eq!(throttle_scope(ThrottleClass::ServiceToken), "service_token");
    }

    #[test]
    fn throttle_cache_key_absent_allows() {
        // `get_cache_key` None → `SimpleRateThrottle.allow_request` returns
        // True (identity), for BOTH classes (`rate_limit.py:16-22,57-63`).
        assert_eq!(throttle_cache_key(API_KEY_THROTTLE_SCOPE, None), None);
        assert_eq!(throttle_cache_key(API_KEY_THROTTLE_SCOPE, Some("")), None);
        assert_eq!(throttle_cache_key(SERVICE_TOKEN_THROTTLE_SCOPE, None), None);
        assert_eq!(
            throttle_cache_key(API_KEY_THROTTLE_SCOPE, Some("k")),
            Some("api_key:k".to_owned())
        );
        assert_eq!(
            throttle_cache_key(SERVICE_TOKEN_THROTTLE_SCOPE, Some("svc")),
            Some("service_token:svc".to_owned())
        );
    }

    #[test]
    fn throttle_rates_headers_and_auth_strings() {
        assert_eq!(API_KEY_THROTTLE_SCOPE, "api_key");
        assert_eq!(SERVICE_TOKEN_THROTTLE_SCOPE, "service_token");
        assert_eq!(API_KEY_THROTTLE_REQUESTS_PER_MINUTE, 60);
        assert_eq!(API_KEY_RATE_LIMIT_ENV, "API_KEY_RATE_LIMIT");
        assert_eq!(SERVICE_TOKEN_THROTTLE_REQUESTS_PER_MINUTE, 300);
        assert_eq!(THROTTLE_WINDOW_SECS, 60);
        assert_eq!(RATE_LIMIT_REMAINING_HEADER, "X-RateLimit-Remaining");
        assert_eq!(RATE_LIMIT_RESET_HEADER, "X-RateLimit-Reset");
        assert_eq!(API_KEY_HEADER, "X-Api-Key");
        assert_eq!(INVALID_TOKEN_DETAIL, "Given API token is not valid");
    }

    #[test]
    fn patch_low_role_non_author_denied_400() {
        // Fixture golden: role<=5 non-author PATCH → 400 verbatim
        // (`views/intake.py:336-341`). BUG-1: 400, not 403.
        let denied = patch_edit_gate(5, false).expect_err("non-author denied");
        assert_eq!(denied, PatchDeny);
        assert_eq!(PatchDeny::STATUS, 400);
        assert_eq!(PatchDeny::BODY, EDIT_DENIED_BODY);
        // Numeric `<=`: lower custom roles deny too.
        assert!(patch_edit_gate(0, false).is_err());
        // Authors pass at any role.
        assert!(patch_edit_gate(5, true).is_ok());
        assert!(patch_edit_gate(15, false).is_ok());
        assert!(patch_edit_gate(20, false).is_ok());
        // Boundary: 6 is above GUEST.
        assert!(patch_edit_gate(6, false).is_ok());
    }

    #[test]
    fn guest_whitelist_and_intake_attr_boundary() {
        // Fixture goldens: guests whitelisted to name/description only
        // (`:373-380`); intake attrs need role>15 (`:387-392`); role-15
        // PATCH ignores status but saves issue fields.
        assert_eq!(
            GUEST_ISSUE_KEYS,
            ["name", "description_html", "description_json"]
        );
        assert!(guest_issue_narrowed(5));
        assert!(guest_issue_narrowed(0));
        assert!(!guest_issue_narrowed(6));
        assert!(!guest_issue_narrowed(15));
        assert!(!intake_fields_writable(5));
        assert!(!intake_fields_writable(15));
        assert!(intake_fields_writable(16));
        assert!(intake_fields_writable(20));
    }

    #[test]
    fn delete_cascade_and_guard() {
        // Fixture `delete` matrix (`views/intake.py:474-490`).
        for status in [-2, -1, 0, 2] {
            assert!(destroy_cascades_to_issue(status), "cascades {status}");
        }
        assert!(!destroy_cascades_to_issue(1));
        assert!(!destroy_cascades_to_issue(3));
        // Guard inside the cascade branch only.
        assert!(delete_guard(true, false).is_ok());
        assert!(delete_guard(false, true).is_ok());
        assert!(delete_guard(true, true).is_ok());
        let denied = delete_guard(false, false).expect_err("outsider denied");
        assert_eq!(denied, DeleteDeny);
        assert_eq!(DeleteDeny::STATUS, 403);
        assert_eq!(DeleteDeny::BODY, DELETE_DENIED_BODY);
    }
}
