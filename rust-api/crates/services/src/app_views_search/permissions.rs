//! View + search permission guards (D-29, stage 5).
//!
//! Port of the `allow_permission` gates in
//! `apps/api/pi_dash/app/views/view/base.py` on top of the decorator in
//! `app/permissions/base.py:19-88`, plus the search-endpoint authN and the
//! guest `created_by` scoping in `app/views/search/issue.py:104-166`.
//!
//! The module is pure: membership facts are caller inputs ([`Membership`]),
//! never queries. Tenant isolation (the `workspace__slug=` / `project_id=`
//! filters on every membership row) is the caller's job, exactly like the
//! Python filters. Every fallible check returns the exact DRF error body
//! and status the Python view returns; the `#[cfg(test)]` suite replays
//! every row of `rust-api/fixtures/app_views_search/FX-GUARD.json`.
//!
//! Django-session authentication runs before every gate
//! ([`require_authenticated`]): anonymous callers never reach the role
//! check, mirroring `IsAuthenticated` on `BaseViewSet` / `BaseAPIView`
//! (`app/views/base.py:87,195`).
//!
//! Gate map (`app/views/view/base.py`, `app` creator branch everywhere):
//!
//! * `WorkspaceViewViewSet.list` (`:71`): WORKSPACE `[ADMIN, MEMBER, GUEST]`
//!   + guest narrowing to owned rows (`:75-76`).
//! * `WorkspaceViewViewSet.partial_update` (`:80`): WORKSPACE `roles=[]` +
//!   creator — only creators pass — then locked/owner rechecks (`:85-94`).
//! * `WorkspaceViewViewSet.retrieve` (`:102`): undecorated, authN only.
//! * `WorkspaceViewViewSet.destroy` (`:114`): WORKSPACE `[ADMIN]` + creator,
//!   then the admin-or-owner recheck (`:118-134`).
//! * `WorkspaceViewIssuesViewSet.list` (`:216`): WORKSPACE
//!   `[ADMIN, MEMBER, GUEST]`; the project permission Q (`:142-162`) owns
//!   to the query layer
//!   ([`project_permission_filter`](super::queries_views::project_permission_filter)).
//! * `IssueViewViewSet.list` (`:289`): PROJECT `[ADMIN, MEMBER, GUEST]` +
//!   guest narrowing (`:293-303`).
//! * `IssueViewViewSet.retrieve` (`:308`): PROJECT `[ADMIN, MEMBER, GUEST]`
//!   + guest 403 (`:317-331`).
//! * `IssueViewViewSet.partial_update` (`:343`): PROJECT `roles=[]` +
//!   creator, then locked/owner rechecks (`:347-356`).
//! * `IssueViewViewSet.destroy` (`:365`): PROJECT `[ADMIN]` + creator, then
//!   the admin-or-owner recheck (`:368-392`).
//! * `IssueViewFavoriteViewSet.create` (`:413`) / `destroy` (`:423`):
//!   PROJECT `[ADMIN, MEMBER]`, no creator bypass.
//! * Search (`app/views/search/issue.py:104-166`, `search/base.py`):
//!   `IsAuthenticated` only on all three endpoints; guest `created_by`
//!   scoping on the issue search (`:146-149`).
//!
//! Ported bugs (translate, don't redesign):
//!
//! * B1 (`view/base.py:102-112`): workspace retrieve serializes `.first()`
//!   unconditionally — unknown pk answers 200 `null`, not 404. The gate
//!   half is [`workspace_view_retrieve_undecorated`]; the null body owns to
//!   the handler.
//! * B1b (`view/base.py:317-327`): project retrieve dereferences
//!   `issue_view.owned_by` after `.first()`; unknown pk plus a guest
//!   without `guest_view_all_features` raises `AttributeError` → 500, not
//!   404. [`project_view_retrieve_guest_gate`] pins the crash order via
//!   [`ProjectViewLookup::Missing`].
//! * B4 (`view/base.py:404-411`): the favorite list queryset raises
//!   `FieldError` before any row; owned by the query layer
//!   ([`favorite_list_sql`](super::queries_views::favorite_list_sql)).
//!   The favorite *gates* here cover create/destroy only.

use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// `ROLE.ADMIN.value` (`app/permissions/base.py:14`).
pub const ADMIN: i16 = 20;
/// `ROLE.MEMBER.value` (`app/permissions/base.py:15`).
pub const MEMBER: i16 = 15;
/// `ROLE.GUEST.value` (`app/permissions/base.py:16`).
pub const GUEST: i16 = 5;

// ---------------------------------------------------------------------------
// Error responses
// ---------------------------------------------------------------------------

/// An exact DRF error response: HTTP status plus JSON body.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorBody {
    /// HTTP status code.
    pub status: u16,
    /// Byte-exact JSON body.
    pub body: Value,
}

impl ErrorBody {
    fn new(status: u16, body: Value) -> Self {
        Self { status, body }
    }
}

/// DRF `IsAuthenticated` denial (`app/views/base.py:87,195`; DRF
/// `NotAuthenticated`, verified `status_code=401` against DRF 3.18.1).
pub fn unauthenticated() -> ErrorBody {
    ErrorBody::new(
        401,
        json!({"detail": "Authentication credentials were not provided."}),
    )
}

/// `allow_permission` fallthrough (`app/permissions/base.py:101-106`,
/// creator-branch `:31-34`).
pub fn permission_denied() -> ErrorBody {
    ErrorBody::new(
        403,
        json!({"error": "You don't have the required permissions."}),
    )
}

/// Locked-view recheck (`view/base.py:86,349`).
pub fn view_locked() -> ErrorBody {
    ErrorBody::new(400, json!({"error": "view is locked"}))
}

/// Non-owner write recheck (`view/base.py:91-94,353-356`).
pub fn only_owner_can_update() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "Only the owner of the view can update the view"}),
    )
}

/// Non-admin non-owner delete recheck (`view/base.py:130-133,389-392`).
pub fn only_admin_or_owner() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "Only admin or owner can delete the view"}),
    )
}

/// Guest reading another user's project view (`view/base.py:324-328`;
/// the `"issue"` wording is the source's — ported verbatim).
pub fn not_allowed_to_view() -> ErrorBody {
    ErrorBody::new(
        403,
        json!({"error": "You are not allowed to view this issue"}),
    )
}

/// Missing row on a `.get()` write path (`app/views/base.py:133-137`,
/// `handle_exception` `ObjectDoesNotExist` branch).
pub fn view_not_found() -> ErrorBody {
    ErrorBody::new(404, json!({"error": "The required object does not exist."}))
}

/// `handle_exception` generic branch (`app/views/base.py:148-152`):
/// `AttributeError` (B1b), serializer crashes, anything unclassified.
pub fn server_error() -> ErrorBody {
    ErrorBody::new(
        500,
        json!({"error": "Something went wrong please try again later"}),
    )
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// Membership facts the gates read, resolved by the caller with the same
/// row filters Python uses: project rows match
/// `(member, workspace__slug, project_id, is_active=True)` and workspace
/// rows match `(member, workspace__slug, is_active=True)`, both through
/// the soft-delete-aware `objects` manager (`deleted_at IS NULL`)
/// (`app/permissions/base.py:26-30,45-50,53-59,65-76`). `None` means no
/// such row exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Membership {
    /// Role value of the active, non-deleted `ProjectMember` row.
    pub project_role: Option<i16>,
    /// Role value of the active, non-deleted `WorkspaceMember` row.
    pub workspace_role: Option<i16>,
}

impl Membership {
    /// Authenticated user with no membership anywhere.
    pub fn outsider() -> Self {
        Self {
            project_role: None,
            workspace_role: None,
        }
    }

    /// Any active workspace row exists (the creator-branch precheck,
    /// `app/permissions/base.py:26-30`).
    pub fn workspace_member(&self) -> bool {
        self.workspace_role.is_some()
    }

    /// Active workspace row with role exactly `ADMIN`
    /// (`role=ROLE.ADMIN.value`, not `>=`).
    pub fn workspace_admin(&self) -> bool {
        self.workspace_role == Some(ADMIN)
    }
}

// ---------------------------------------------------------------------------
// Decorator core
// ---------------------------------------------------------------------------

/// Django-session authN: anonymous callers are rejected before any role
/// check (`IsAuthenticated` on `BaseViewSet` / `BaseAPIView`).
pub fn require_authenticated(authenticated: bool) -> Result<(), ErrorBody> {
    if authenticated {
        Ok(())
    } else {
        Err(unauthenticated())
    }
}

/// `allow_permission(allowed_roles, level="WORKSPACE")`
/// (`app/permissions/base.py:45-50`): an active workspace membership whose
/// role is allowed passes; `roles=[]` matches nothing, so creator-only
/// gates rely entirely on the bypass. Everything else is the 403
/// fallthrough.
pub fn allow_workspace(allowed_roles: &[i16], m: &Membership) -> Result<(), ErrorBody> {
    if m.workspace_role
        .is_some_and(|role| allowed_roles.contains(&role))
    {
        return Ok(());
    }
    Err(permission_denied())
}

/// `allow_permission(allowed_roles)` at the default `PROJECT` level, after
/// the creator branch (`app/permissions/base.py:52-96`): an active project
/// membership whose role is allowed passes; otherwise any active project
/// membership plus an active workspace ADMIN membership passes (the
/// backdoor, `:82-96`); everything else is the 403 fallthrough.
pub fn allow_project(allowed_roles: &[i16], m: &Membership) -> Result<(), ErrorBody> {
    if m.project_role
        .is_some_and(|role| allowed_roles.contains(&role))
    {
        return Ok(());
    }
    if m.project_role.is_some() && m.workspace_admin() {
        return Ok(());
    }
    Err(permission_denied())
}

/// `allow_permission(allowed_roles, level="WORKSPACE", creator=True,
/// model=IssueView)` (`app/permissions/base.py:24-38`, `app` copy):
/// callers outside the workspace are refused first; a user recorded as the
/// view creator (`IssueView.objects.filter(id=pk,
/// created_by=user).exists()`) passes regardless of role; everyone else
/// falls through to [`allow_workspace`].
pub fn allow_workspace_creator_or_roles(
    allowed_roles: &[i16],
    m: &Membership,
    is_creator: bool,
) -> Result<(), ErrorBody> {
    if !m.workspace_member() {
        return Err(permission_denied());
    }
    if is_creator {
        return Ok(());
    }
    allow_workspace(allowed_roles, m)
}

/// `allow_permission(allowed_roles, creator=True, model=IssueView)` at
/// `PROJECT` level: same workspace precheck and creator bypass, then
/// [`allow_project`]. Note the precheck is still the *workspace*
/// membership (`kwargs["slug"]`, `:26-30`) — the project id plays no part
/// in the bypass.
pub fn allow_project_creator_or_roles(
    allowed_roles: &[i16],
    m: &Membership,
    is_creator: bool,
) -> Result<(), ErrorBody> {
    if !m.workspace_member() {
        return Err(permission_denied());
    }
    if is_creator {
        return Ok(());
    }
    allow_project(allowed_roles, m)
}

// ---------------------------------------------------------------------------
// Unit 1 — WorkspaceView gates (view/base.py:60-135)
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/views/` (`:71`): WORKSPACE ADMIN + MEMBER +
/// GUEST.
pub fn workspace_view_list_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_workspace(&[ADMIN, MEMBER, GUEST], m)
}

/// `list` guest narrowing (`:75-76`): an active GUEST workspace membership
/// narrows the queryset to `owned_by=request.user`. The role test is an
/// exact `role == GUEST` filter.
pub fn workspace_view_list_guest_scoped(m: &Membership) -> bool {
    m.workspace_role == Some(GUEST)
}

/// `GET workspaces/<slug>/views/<pk>/` (`:102`): no decorator — any
/// authenticated caller passes (even a workspace outsider; that is the
/// source as written). B1 (200 `null` on unknown pk) owns to the handler.
pub fn workspace_view_retrieve_undecorated(authenticated: bool) -> Result<(), ErrorBody> {
    require_authenticated(authenticated)
}

/// `PATCH workspaces/<slug>/views/<pk>/` (`:80`): WORKSPACE `roles=[]` +
/// creator — non-creators always fall through to 403 here.
pub fn workspace_view_partial_gate(m: &Membership, is_creator: bool) -> Result<(), ErrorBody> {
    allow_workspace_creator_or_roles(&[], m, is_creator)
}

/// What the `partial_update` row lookup found. Both workspace (`:82`) and
/// project (`:345`) paths read through `select_for_update().get(...)`,
/// which raises `ObjectDoesNotExist` → 404 on a missing row
/// (`handle_exception`, `app/views/base.py:133-137`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewRow {
    /// Row present; carries the locked flag and whether the caller owns it.
    Found {
        /// `view.is_locked`.
        is_locked: bool,
        /// `view.owned_by == request.user`.
        is_owner: bool,
    },
    /// No row (unknown pk).
    Missing,
}

/// `partial_update` in-body rechecks (`:85-94`, `:347-356`, identical
/// bodies): locked rows 400 first, then non-owners 400 — after the gate,
/// so these fire for creators only.
pub fn view_partial_recheck(lookup: ViewRow) -> Result<(), ErrorBody> {
    match lookup {
        ViewRow::Missing => Err(view_not_found()),
        ViewRow::Found {
            is_locked: true, ..
        } => Err(view_locked()),
        ViewRow::Found {
            is_owner: false, ..
        } => Err(only_owner_can_update()),
        ViewRow::Found {
            is_locked: false,
            is_owner: true,
        } => Ok(()),
    }
}

/// `DELETE workspaces/<slug>/views/<pk>/` (`:114`): WORKSPACE `[ADMIN]` +
/// creator — creators pass before the role check, admins pass the role
/// check, everyone else 403s at the gate.
pub fn workspace_view_destroy_gate(m: &Membership, is_creator: bool) -> Result<(), ErrorBody> {
    allow_workspace_creator_or_roles(&[ADMIN], m, is_creator)
}

/// `destroy` in-body recheck (`:118-134`): an active ADMIN workspace row
/// (`role=20`) or the owner deletes (plus the favorite cleanup); anything
/// else 400s — even a MEMBER who passed no gate can never get here, and a
/// creator who is neither admin nor owner falls here. The `.get()` 404 on
/// a missing row precedes it ([`view_not_found`]).
pub fn workspace_view_destroy_recheck(m: &Membership, is_owner: bool) -> Result<(), ErrorBody> {
    if m.workspace_role == Some(ADMIN) || is_owner {
        return Ok(());
    }
    Err(only_admin_or_owner())
}

// ---------------------------------------------------------------------------
// Unit 2 — IssueView (project view) gates (view/base.py:256-398)
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/projects/<project_id>/views/` (`:289`):
/// PROJECT ADMIN + MEMBER + GUEST.
pub fn project_view_list_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER, GUEST], m)
}

/// `list` guest narrowing (`:293-303`): an active GUEST project membership
/// without `project.guest_view_all_features` narrows the queryset to
/// `owned_by=request.user`. Exact `role == GUEST`, like the workspace side.
pub fn project_view_list_guest_scoped(m: &Membership, guest_view_all_features: bool) -> bool {
    m.project_role == Some(GUEST) && !guest_view_all_features
}

/// `GET .../views/<pk>/` (`:308`): PROJECT ADMIN + MEMBER + GUEST.
pub fn project_view_retrieve_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER, GUEST], m)
}

/// What the `retrieve` row lookup found: `.get_queryset().filter(...).
/// first()` — `None` on unknown pk, no 404.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectViewLookup {
    /// Row present; carries whether the caller owns it.
    Found {
        /// `view.owned_by == request.user`.
        is_owner: bool,
    },
    /// No row (unknown pk).
    Missing,
}

/// `retrieve` guest recheck (`:317-331`): a GUEST without
/// `guest_view_all_features` reading another user's view 403s with the
/// source-verbatim body. B1b: the `owned_by` dereference runs before any
/// `None` guard, so a missing row under that branch raises
/// `AttributeError` → 500 — the crash order is the port. Missing rows
/// outside the branch serialize `None` → 200 `null` (handler-owned).
pub fn project_view_retrieve_guest_gate(
    m: &Membership,
    guest_view_all_features: bool,
    lookup: ProjectViewLookup,
) -> Result<(), ErrorBody> {
    if m.project_role == Some(GUEST) && !guest_view_all_features {
        return match lookup {
            ProjectViewLookup::Found { is_owner: true } => Ok(()),
            ProjectViewLookup::Found { is_owner: false } => Err(not_allowed_to_view()),
            ProjectViewLookup::Missing => Err(server_error()),
        };
    }
    Ok(())
}

/// `PATCH .../views/<pk>/` (`:343`): PROJECT `roles=[]` + creator — only
/// creators pass the gate.
pub fn project_view_partial_gate(m: &Membership, is_creator: bool) -> Result<(), ErrorBody> {
    allow_project_creator_or_roles(&[], m, is_creator)
}

/// `DELETE .../views/<pk>/` (`:365`): PROJECT `[ADMIN]` + creator.
pub fn project_view_destroy_gate(m: &Membership, is_creator: bool) -> Result<(), ErrorBody> {
    allow_project_creator_or_roles(&[ADMIN], m, is_creator)
}

/// `destroy` in-body recheck (`:368-392`): an active ADMIN project row
/// (`role=20`) or the owner (`owned_by_id == user.id`) deletes (plus the
/// favorite and recent-visit cleanup); anything else 400s. The `.get()`
/// 404 on a missing row precedes it ([`view_not_found`]).
pub fn project_view_destroy_recheck(m: &Membership, is_owner: bool) -> Result<(), ErrorBody> {
    if m.project_role == Some(ADMIN) || is_owner {
        return Ok(());
    }
    Err(only_admin_or_owner())
}

// ---------------------------------------------------------------------------
// Unit 3 — View-issues list gate (view/base.py:138-253)
// ---------------------------------------------------------------------------

/// `GET workspaces/<slug>/issues/` (`:216`): WORKSPACE ADMIN + MEMBER +
/// GUEST. The guest/role row narrowing is the project permission Q
/// (`:142-162`), owned by the query layer — not re-ported here.
pub fn view_issues_list_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_workspace(&[ADMIN, MEMBER, GUEST], m)
}

// ---------------------------------------------------------------------------
// Unit 4 — Favorite gates (view/base.py:401-433)
// ---------------------------------------------------------------------------

/// `POST .../user-favorite-views/` (`:413`): PROJECT ADMIN + MEMBER —
/// guests 403. No creator bypass (`creator` defaults `False`).
pub fn favorite_create_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER], m)
}

/// `DELETE .../user-favorite-views/<view_id>/` (`:423`): PROJECT ADMIN +
/// MEMBER, no creator bypass (the address is `view_id`, not the favorite
/// pk, so the decorator's `id=pk` creator lookup could never match
/// anyway). The `.get()` 404 on a missing favorite precedes the delete.
pub fn favorite_destroy_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER], m)
}

// ---------------------------------------------------------------------------
// Unit 5 — Search gates (app/views/search/)
// ---------------------------------------------------------------------------

/// AuthN for all three search endpoints — global (`search/base.py:43`),
/// issue (`search/issue.py:18`), entity (`search/base.py:289`): every one
/// extends `BaseAPIView` whose only permission class is `IsAuthenticated`
/// (`app/views/base.py:195`), with no `allow_permission` decorator.
pub fn search_auth(authenticated: bool) -> Result<(), ErrorBody> {
    require_authenticated(authenticated)
}

/// Issue-search guest scoping (`search/issue.py:146-149`): an active GUEST
/// project membership on the URL project narrows the results to
/// `created_by=request.user`. Exact `role == GUEST`; `is_active=True` is
/// part of the caller's row filter, like every other gate input.
pub fn issue_search_guest_scoped(m: &Membership) -> bool {
    m.project_role == Some(GUEST)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_views_search/FX-GUARD.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("guard fixture exists"))
            .expect("guard fixture parses")
    }

    fn gates() -> Vec<Value> {
        fixture()
            .get("gates")
            .and_then(Value::as_array)
            .expect("gates array")
            .clone()
    }

    fn gate(action: &str) -> Value {
        gates()
            .iter()
            .find(|row| row.get("action").and_then(Value::as_str) == Some(action))
            .unwrap_or_else(|| panic!("FX-GUARD.json lacks gate {action}"))
            .clone()
    }

    /// Full member of the tenant: project + workspace rows; workspace admin
    /// only when asked.
    fn tenant(project_role: i16, workspace_role: i16) -> Membership {
        Membership {
            project_role: Some(project_role),
            workspace_role: Some(workspace_role),
        }
    }

    fn assert_body(actual: ErrorBody, status: u16, body: Value) {
        assert_eq!(actual.status, status, "status for {body}");
        assert_eq!(actual.body, body);
    }

    // -- fixture shape ------------------------------------------------------

    #[test]
    fn role_values_match_python() {
        // FX-GUARD.json `role_values` (app/permissions/base.py:13-16).
        let roles = fixture().get("role_values").expect("role_values").clone();
        assert_eq!(roles.get("ADMIN").and_then(Value::as_u64).unwrap(), 20);
        assert_eq!(roles.get("MEMBER").and_then(Value::as_u64).unwrap(), 15);
        assert_eq!(roles.get("GUEST").and_then(Value::as_u64).unwrap(), 5);
        assert_eq!(ADMIN, 20);
        assert_eq!(MEMBER, 15);
        assert_eq!(GUEST, 5);
    }

    #[test]
    fn deny_body_matches_allow_fallthrough() {
        // FX-GUARD.json `deny_body` (app/permissions/base.py:101-106).
        let deny = fixture().get("deny_body").expect("deny_body").clone();
        assert_eq!(deny.get("status").and_then(Value::as_u64).unwrap(), 403);
        assert_body(
            permission_denied(),
            403,
            deny.get("body").expect("body").clone(),
        );
        // Byte-exact wire body (compact DRF JSON, no spaces).
        assert_eq!(
            serde_json::to_string(&permission_denied().body).unwrap(),
            r#"{"error":"You don't have the required permissions."}"#
        );
    }

    #[test]
    fn wire_bodies_are_byte_exact() {
        assert_eq!(
            serde_json::to_string(&unauthenticated().body).unwrap(),
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(unauthenticated().status, 401);
        assert_eq!(
            serde_json::to_string(&not_allowed_to_view().body).unwrap(),
            r#"{"error":"You are not allowed to view this issue"}"#
        );
        assert_eq!(not_allowed_to_view().status, 403);
        assert_eq!(
            serde_json::to_string(&view_locked().body).unwrap(),
            r#"{"error":"view is locked"}"#
        );
        assert_eq!(
            serde_json::to_string(&only_owner_can_update().body).unwrap(),
            r#"{"error":"Only the owner of the view can update the view"}"#
        );
        assert_eq!(
            serde_json::to_string(&only_admin_or_owner().body).unwrap(),
            r#"{"error":"Only admin or owner can delete the view"}"#
        );
        assert_eq!(
            serde_json::to_string(&view_not_found().body).unwrap(),
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(view_not_found().status, 404);
        assert_eq!(
            serde_json::to_string(&server_error().body).unwrap(),
            r#"{"error":"Something went wrong please try again later"}"#
        );
    }

    #[test]
    fn all_ten_gates_present_in_fixture() {
        assert_eq!(gates().len(), 10);
        for action in [
            "WorkspaceViewViewSet.list",
            "WorkspaceViewViewSet.partial_update",
            "WorkspaceViewViewSet.destroy",
            "WorkspaceViewIssuesViewSet.list",
            "IssueViewViewSet.list",
            "IssueViewViewSet.retrieve",
            "IssueViewViewSet.partial_update",
            "IssueViewViewSet.destroy",
            "IssueViewFavoriteViewSet.create",
            "IssueViewFavoriteViewSet.destroy",
        ] {
            gate(action);
        }
    }

    // -- auth ---------------------------------------------------------------

    #[test]
    fn anonymous_rejected_before_role_check() {
        // FX-GUARD.json `golden_matrix.nonmember`: every gate 403s; the
        // DRF layer rejects anonymous callers first (401).
        assert_eq!(
            require_authenticated(false)
                .expect_err("anon denied")
                .status,
            401
        );
        assert!(require_authenticated(true).is_ok());
        assert!(search_auth(true).is_ok());
        assert_eq!(search_auth(false).expect_err("anon denied").status, 401);
        assert!(workspace_view_retrieve_undecorated(true).is_ok());
        assert_eq!(
            workspace_view_retrieve_undecorated(false)
                .expect_err("anon denied")
                .status,
            401
        );
    }

    #[test]
    fn nonmember_denied_on_every_gate() {
        // Fixture `golden_matrix.nonmember`: no rows anywhere → 403.
        let m = Membership::outsider();
        for gate in [
            workspace_view_list_gate(&m),
            workspace_view_partial_gate(&m, false),
            workspace_view_destroy_gate(&m, false),
            view_issues_list_gate(&m),
            project_view_list_gate(&m),
            project_view_retrieve_gate(&m),
            project_view_partial_gate(&m, false),
            project_view_destroy_gate(&m, false),
            favorite_create_gate(&m),
            favorite_destroy_gate(&m),
        ] {
            assert_eq!(gate.expect_err("outsider denied").status, 403);
        }
        // Even a claimed creator without a workspace row is refused (the
        // app-copy membership precheck, base.py:26-30).
        assert!(workspace_view_partial_gate(&m, true).is_err());
        assert!(project_view_destroy_gate(&m, true).is_err());
    }

    #[test]
    fn inactive_membership_denies() {
        // Fixture `golden_matrix.inactive_membership`: every check requires
        // `is_active=True`, so an inactive row reads as no row. The caller
        // resolves inactive rows to `None`; the outsider case covers it.
        let m = Membership::outsider();
        assert!(workspace_view_list_gate(&m).is_err());
        assert!(project_view_list_gate(&m).is_err());
        assert!(favorite_create_gate(&m).is_err());
    }

    // -- unit 1: workspace gates --------------------------------------------

    #[test]
    fn workspace_list_all_roles_allow() {
        // Fixture gate `WorkspaceViewViewSet.list` (base.py:71).
        for role in [ADMIN, MEMBER, GUEST] {
            let m = Membership {
                project_role: None,
                workspace_role: Some(role),
            };
            assert!(workspace_view_list_gate(&m).is_ok(), "role {role}");
        }
    }

    #[test]
    fn workspace_list_guest_narrowing_is_exact_role() {
        // Fixture `guest_scoping_in_body.workspace_views` (base.py:75-76).
        let guest = Membership {
            project_role: None,
            workspace_role: Some(GUEST),
        };
        assert!(workspace_view_list_guest_scoped(&guest));
        for role in [ADMIN, MEMBER] {
            let m = Membership {
                project_role: None,
                workspace_role: Some(role),
            };
            assert!(!workspace_view_list_guest_scoped(&m), "role {role}");
        }
        assert!(!workspace_view_list_guest_scoped(&Membership::outsider()));
    }

    #[test]
    fn workspace_partial_only_creators_pass() {
        // Fixture gate `WorkspaceViewViewSet.partial_update` (base.py:80):
        // `roles=[]` so non-creators always fall through to 403.
        let member = Membership {
            project_role: None,
            workspace_role: Some(MEMBER),
        };
        assert!(workspace_view_partial_gate(&member, true).is_ok());
        assert!(workspace_view_partial_gate(&member, false).is_err());
        let admin = Membership {
            project_role: None,
            workspace_role: Some(ADMIN),
        };
        assert!(workspace_view_partial_gate(&admin, false).is_err());
    }

    #[test]
    fn workspace_partial_recheck_locked_first_then_owner() {
        // Fixture gates note + base.py:85-94.
        assert_eq!(
            view_partial_recheck(ViewRow::Missing)
                .expect_err("404")
                .status,
            404
        );
        let locked_owner = ViewRow::Found {
            is_locked: true,
            is_owner: true,
        };
        assert_eq!(
            view_partial_recheck(locked_owner)
                .expect_err("locked")
                .status,
            400
        );
        assert_eq!(
            view_partial_recheck(locked_owner).expect_err("locked").body,
            view_locked().body
        );
        let unlocked_stranger = ViewRow::Found {
            is_locked: false,
            is_owner: false,
        };
        assert_eq!(
            view_partial_recheck(unlocked_stranger)
                .expect_err("owner")
                .body,
            only_owner_can_update().body
        );
        assert!(view_partial_recheck(ViewRow::Found {
            is_locked: false,
            is_owner: true
        })
        .is_ok());
    }

    #[test]
    fn workspace_destroy_admin_or_creator_then_recheck() {
        // Fixture gate `WorkspaceViewViewSet.destroy` (base.py:114).
        let admin = Membership {
            project_role: None,
            workspace_role: Some(ADMIN),
        };
        assert!(workspace_view_destroy_gate(&admin, false).is_ok());
        let member = Membership {
            project_role: None,
            workspace_role: Some(MEMBER),
        };
        assert!(workspace_view_destroy_gate(&member, true).is_ok());
        assert!(workspace_view_destroy_gate(&member, false).is_err());
        let guest = Membership {
            project_role: None,
            workspace_role: Some(GUEST),
        };
        assert!(workspace_view_destroy_gate(&guest, false).is_err());
        // In-body recheck (base.py:118-134): admin row OR owner.
        assert!(workspace_view_destroy_recheck(&admin, false).is_ok());
        assert!(workspace_view_destroy_recheck(&member, true).is_ok());
        assert_eq!(
            workspace_view_destroy_recheck(&member, false)
                .expect_err("400")
                .body,
            only_admin_or_owner().body
        );
    }

    // -- unit 2: project gates ----------------------------------------------

    #[test]
    fn project_list_all_roles_allow() {
        // Fixture gate `IssueViewViewSet.list` (base.py:289).
        for role in [ADMIN, MEMBER, GUEST] {
            assert!(
                project_view_list_gate(&tenant(role, MEMBER)).is_ok(),
                "role {role}"
            );
        }
    }

    #[test]
    fn project_list_guest_scoping_needs_flag_off() {
        // Fixture `guest_scoping_in_body.project_views` (base.py:293-303).
        let guest = tenant(GUEST, MEMBER);
        assert!(project_view_list_guest_scoped(&guest, false));
        assert!(!project_view_list_guest_scoped(&guest, true));
        assert!(!project_view_list_guest_scoped(
            &tenant(MEMBER, MEMBER),
            false
        ));
    }

    #[test]
    fn project_retrieve_guest_recheck_and_b1b_crash_order() {
        // Fixture gate `IssueViewViewSet.retrieve` (base.py:308) +
        // `guest_scoping_in_body.project_views` (base.py:317-331).
        assert!(project_view_retrieve_gate(&tenant(GUEST, MEMBER)).is_ok());
        let guest = tenant(GUEST, MEMBER);
        assert!(project_view_retrieve_guest_gate(
            &guest,
            false,
            ProjectViewLookup::Found { is_owner: true }
        )
        .is_ok());
        let denied = project_view_retrieve_guest_gate(
            &guest,
            false,
            ProjectViewLookup::Found { is_owner: false },
        )
        .expect_err("guest 403");
        assert_body(denied, 403, not_allowed_to_view().body);
        // B1b: missing row under the guest branch → AttributeError → 500.
        assert_eq!(
            project_view_retrieve_guest_gate(&guest, false, ProjectViewLookup::Missing)
                .expect_err("B1b 500")
                .status,
            500
        );
        // Flag on, or a higher role: no recheck at all.
        assert!(project_view_retrieve_guest_gate(&guest, true, ProjectViewLookup::Missing).is_ok());
        let member = tenant(MEMBER, MEMBER);
        assert!(
            project_view_retrieve_guest_gate(&member, false, ProjectViewLookup::Missing).is_ok()
        );
    }

    #[test]
    fn project_partial_only_creators_pass() {
        // Fixture gate `IssueViewViewSet.partial_update` (base.py:343):
        // `roles=[]` so only creators pass.
        let member = tenant(MEMBER, MEMBER);
        assert!(project_view_partial_gate(&member, true).is_ok());
        assert!(project_view_partial_gate(&member, false).is_err());
        // A project ADMIN without workspace admin still falls through:
        // `roles=[]` matches nothing and the fallback needs workspace ADMIN.
        let admin = tenant(ADMIN, MEMBER);
        assert!(project_view_partial_gate(&admin, false).is_err());
        // The workspace-admin fallback (base.py:82-96) sits outside the
        // role check, so it fires even with `roles=[]`.
        let ws_admin = tenant(MEMBER, ADMIN);
        assert!(project_view_partial_gate(&ws_admin, false).is_ok());
        // Guest creator passes via the bypass (fixture
        // `golden_matrix.GUEST_creator`) — workspace row required.
        let guest_creator = tenant(GUEST, GUEST);
        assert!(project_view_partial_gate(&guest_creator, true).is_ok());
    }

    #[test]
    fn project_destroy_admin_or_creator_then_recheck() {
        // Fixture gate `IssueViewViewSet.destroy` (base.py:365).
        let admin = tenant(ADMIN, MEMBER);
        assert!(project_view_destroy_gate(&admin, false).is_ok());
        let member = tenant(MEMBER, MEMBER);
        assert!(project_view_destroy_gate(&member, true).is_ok());
        // Guests can never destroy via the gate (fixture
        // `golden_matrix.GUEST_noncreator`).
        let guest = tenant(GUEST, GUEST);
        assert!(project_view_destroy_gate(&guest, false).is_err());
        // In-body recheck (base.py:368-392): project ADMIN row OR owner.
        assert!(project_view_destroy_recheck(&admin, false).is_ok());
        assert!(project_view_destroy_recheck(&member, true).is_ok());
        assert_eq!(
            project_view_destroy_recheck(&member, false)
                .expect_err("400")
                .body,
            only_admin_or_owner().body
        );
    }

    #[test]
    fn workspace_admin_fallback_passes_without_project_role() {
        // Fixture `golden_matrix.ADMIN_workspace_member_not_project_member`
        // (base.py:82-96): active project row of ANY role + workspace ADMIN.
        let m = Membership {
            project_role: Some(GUEST),
            workspace_role: Some(ADMIN),
        };
        assert!(project_view_list_gate(&m).is_ok());
        assert!(project_view_retrieve_gate(&m).is_ok());
        assert!(favorite_create_gate(&m).is_ok());
        // Without any project row the fallback does NOT fire.
        let no_project = Membership {
            project_role: None,
            workspace_role: Some(ADMIN),
        };
        assert!(project_view_list_gate(&no_project).is_err());
        assert!(favorite_create_gate(&no_project).is_err());
    }

    // -- unit 3: view-issues gate --------------------------------------------

    #[test]
    fn view_issues_list_all_roles_allow() {
        // Fixture gate `WorkspaceViewIssuesViewSet.list` (base.py:216).
        for role in [ADMIN, MEMBER, GUEST] {
            let m = Membership {
                project_role: None,
                workspace_role: Some(role),
            };
            assert!(view_issues_list_gate(&m).is_ok(), "role {role}");
        }
        assert!(view_issues_list_gate(&Membership::outsider()).is_err());
    }

    // -- unit 4: favorite gates ----------------------------------------------

    #[test]
    fn favorite_gates_admin_member_allow_guest_deny() {
        // Fixture gates `IssueViewFavoriteViewSet.create` (:413) /
        // `destroy` (:423): PROJECT [ADMIN, MEMBER], no creator bypass.
        for role in [ADMIN, MEMBER] {
            let m = tenant(role, MEMBER);
            assert!(favorite_create_gate(&m).is_ok(), "role {role}");
            assert!(favorite_destroy_gate(&m).is_ok(), "role {role}");
        }
        let guest = tenant(GUEST, GUEST);
        assert!(favorite_create_gate(&guest).is_err());
        assert!(favorite_destroy_gate(&guest).is_err());
        // Workspace admin alone (no project row) does not pass.
        let ws_admin = Membership {
            project_role: None,
            workspace_role: Some(ADMIN),
        };
        assert!(favorite_create_gate(&ws_admin).is_err());
    }

    // -- unit 5: search gates --------------------------------------------------

    #[test]
    fn search_endpoints_require_auth_only() {
        // All three endpoints are `IsAuthenticated`-only: any authenticated
        // caller passes regardless of membership (BaseAPIView, no
        // `allow_permission` decorator); anonymous callers 401.
        assert!(search_auth(true).is_ok());
        assert_eq!(search_auth(false).expect_err("anon 401").status, 401);
    }

    #[test]
    fn issue_search_guest_scoping_is_exact_role() {
        // Fixture `guest_scoping_in_body.issue_search`
        // (search/issue.py:146-149).
        assert!(issue_search_guest_scoped(&tenant(GUEST, MEMBER)));
        assert!(!issue_search_guest_scoped(&tenant(MEMBER, MEMBER)));
        assert!(!issue_search_guest_scoped(&tenant(ADMIN, ADMIN)));
        assert!(!issue_search_guest_scoped(&Membership::outsider()));
    }
}
