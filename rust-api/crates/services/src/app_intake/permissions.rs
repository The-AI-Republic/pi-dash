//! Intake permission guards (D-32, stage 5).
//!
//! Port of the role matrices, guest-scoping checks and creator gates in
//! `apps/api/pi_dash/app/views/intake/base.py`, on top of the
//! `@allow_permission` decorator in `app/permissions/base.py:19-88`.
//!
//! The module is pure: membership facts are caller inputs
//! ([`Membership`]), never queries. Tenant isolation (slug / project
//! scoping of the membership rows) is the caller's job, exactly like the
//! Python `workspace__slug=` / `project_id=` filters. Every fallible check
//! returns the exact DRF error body and status the Python view returns;
//! the `#[cfg(test)]` suite replays every row of the guard fixtures
//! recorded by PIDASHCONV-278:
//!
//! * `rust-api/fixtures/app_intake/guards/permissions.matrix.json`
//! * `rust-api/fixtures/app_intake/guards/guest_scoping.golden.json`
//! * `rust-api/fixtures/app_intake/guards/default_intake_delete.golden.json`
//!
//! Django-session authentication runs before every gate
//! ([`require_authenticated`]): anonymous callers never reach the role
//! check, mirroring `IsAuthenticated` on `BaseViewSet`.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-perform-create (`base.py:77-79`): `@allow_permission` decorates
//!   `perform_create(serializer)`, but DRF invokes it with the serializer
//!   in the `request` slot, so the wrapper dies on `request.user`
//!   (`AttributeError` → `handle_exception` 500) for every role, including
//!   ADMIN. [`intake_perform_create_outcome`] pins the 500.
//! * BUG-intake-detail-undecorated (`base.py:56-91`): the DRF-default
//!   retrieve / partial_update on `IntakeViewSet` carry no decorator, so
//!   any authenticated user passes with no project-membership check.
//!   [`intake_detail_undecorated`] pins the pass-through.
//! * BUG-intake-issue-put (`base.py:94-566`): `PUT intake-issues/<pk>/`
//!   (update) carries no decorator — only PATCH/GET/DELETE/POST do — so
//!   any authenticated user passes. [`intake_issue_put_undecorated`] pins
//!   the pass-through.
//! * BUG-destroy-missing (`base.py:83`): `DELETE intakes/<unknown-pk>/`
//!   reads `.first()` (`None`) then dereferences `.is_default`
//!   unconditionally → `AttributeError` → 500, never 404.
//!   [`intake_destroy_gate`] pins the 500 via [`IntakeLookup::Missing`].

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

/// DRF `IsAuthenticated` denial (`app/views/base.py:87`).
pub fn unauthenticated() -> ErrorBody {
    ErrorBody::new(
        401,
        json!({"detail": "Authentication credentials were not provided."}),
    )
}

/// `allow_permission` fallthrough
/// (`app/permissions/base.py:101-106`, creator-branch `:31-34`).
pub fn permission_denied() -> ErrorBody {
    ErrorBody::new(
        403,
        json!({"error": "You don't have the required permissions."}),
    )
}

/// Guest reading another user's issue, retrieve (`base.py:542-545`) and
/// versions (`base.py:594-597`).
pub fn not_allowed_to_view() -> ErrorBody {
    ErrorBody::new(
        403,
        json!({"error": "You are not allowed to view this issue"}),
    )
}

/// `partial_update` with no project membership and no workspace admin
/// (`base.py:356-359`).
pub fn only_admin_or_creator() -> ErrorBody {
    ErrorBody::new(
        403,
        json!({"error": "Only admin or creator can update the intake work items"}),
    )
}

/// `partial_update` by a low-role non-creator (`base.py:365-368`).
pub fn cannot_edit_intake_issues() -> ErrorBody {
    ErrorBody::new(400, json!({"error": "You cannot edit intake issues"}))
}

/// Default-intake delete guard (`base.py:86-89`).
pub fn cannot_delete_default_intake() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "You cannot delete the default intake"}),
    )
}

/// Intake row missing on list (`base.py:180`).
pub fn intake_not_found() -> ErrorBody {
    ErrorBody::new(404, json!({"error": "Intake not found"}))
}

/// `handle_exception` generic branch (`app/views/base.py:99-103`).
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
/// `(member, workspace__slug, project_id, is_active)` and are
/// soft-delete-aware (`objects` manager ⇒ `deleted_at IS NULL`);
/// workspace rows match `(member, workspace__slug, is_active)` the same
/// way (`app/permissions/base.py:26-30,45-50,53-59,65-76`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Membership {
    /// Role value of the active, non-deleted `ProjectMember` row, or `None`
    /// when no such row exists.
    pub project_role: Option<i16>,
    /// An active, non-deleted `WorkspaceMember` row exists for the user in
    /// this workspace (any role).
    pub workspace_member: bool,
    /// An active, non-deleted `WorkspaceMember` row with `role == ADMIN`
    /// exists for the user in this workspace.
    pub workspace_admin: bool,
}

impl Membership {
    /// Authenticated user with no membership anywhere.
    pub fn outsider() -> Self {
        Self {
            project_role: None,
            workspace_member: false,
            workspace_admin: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Decorator core
// ---------------------------------------------------------------------------

/// Django-session authN: anonymous callers are rejected before any role
/// check (`IsAuthenticated` on `BaseViewSet`).
pub fn require_authenticated(authenticated: bool) -> Result<(), ErrorBody> {
    if authenticated {
        Ok(())
    } else {
        Err(unauthenticated())
    }
}

/// `allow_permission(allowed_roles)` at `PROJECT` level, after the creator
/// branch (`app/permissions/base.py:40-84`): an active project membership
/// whose role is allowed passes; otherwise any active project membership
/// plus an active workspace ADMIN membership passes (the backdoor,
/// `:64-78`); everything else is the 403 fallthrough.
pub fn allow_project(allowed_roles: &[i16], m: &Membership) -> Result<(), ErrorBody> {
    if let Some(role) = m.project_role {
        if allowed_roles.contains(&role) {
            return Ok(());
        }
    }
    if m.project_role.is_some() && m.workspace_admin {
        return Ok(());
    }
    Err(permission_denied())
}

/// `allow_permission(allowed_roles, creator=True, model=Issue)`
/// (`app/permissions/base.py:23-38`): callers outside the workspace are
/// rejected; a user recorded as the `Issue` creator (`Issue.objects.filter
/// (id=pk, created_by=user).exists()`, where `pk` is the issue id) passes
/// regardless of role; everyone else falls through to [`allow_project`].
pub fn allow_creator_or_roles(
    allowed_roles: &[i16],
    m: &Membership,
    is_creator: bool,
) -> Result<(), ErrorBody> {
    if !m.workspace_member {
        return Err(permission_denied());
    }
    if is_creator {
        return Ok(());
    }
    allow_project(allowed_roles, m)
}

// ---------------------------------------------------------------------------
// IntakeViewSet gates (base.py:56-91)
// ---------------------------------------------------------------------------

/// `GET intakes/` + `GET inboxes/` (`base.py:72-75`): ADMIN + MEMBER.
pub fn intake_list_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER], m)
}

/// `POST intakes/` + `POST inboxes/` (`base.py:77-79`).
///
/// BUG-perform-create: the decorator wraps `perform_create(serializer)`,
/// so the serializer lands in the `request` slot and `request.user`
/// raises before any role is evaluated. Every role — ADMIN included —
/// answers 500, never 403.
pub fn intake_perform_create_outcome() -> Result<(), ErrorBody> {
    Err(server_error())
}

/// `GET` / `PATCH intakes/<pk>/` and the inbox aliases (`base.py:56-91`).
///
/// BUG-intake-detail-undecorated: the DRF-default retrieve and
/// partial_update carry no `@allow_permission`, so any authenticated user
/// passes with no project-membership check.
pub fn intake_detail_undecorated(authenticated: bool) -> Result<(), ErrorBody> {
    require_authenticated(authenticated)
}

/// What `DELETE intakes/<pk>/` found (`base.py:83`): the
/// soft-delete-aware lookup
/// (`Intake.objects.filter(workspace__slug, project_id, pk).first()`), so
/// already-deleted rows read as missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeLookup {
    /// Row present; carries `is_default` (`base.py:85`).
    Found {
        /// `intake.is_default`.
        is_default: bool,
    },
    /// No row (unknown pk, or soft-deleted).
    Missing,
}

/// `DELETE intakes/<pk>/` + inbox aliases (`base.py:81-91`): ADMIN +
/// MEMBER, then the default-intake 400 (`:84-89`), then delete → 204
/// (`:90-91`). A missing row dereferences `None.is_default` → 500, never
/// 404 (BUG-destroy-missing).
pub fn intake_destroy_gate(m: &Membership, lookup: IntakeLookup) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER], m)?;
    match lookup {
        IntakeLookup::Missing => Err(server_error()),
        IntakeLookup::Found { is_default: true } => Err(cannot_delete_default_intake()),
        IntakeLookup::Found { is_default: false } => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// IntakeIssueViewSet gates (base.py:176-566)
// ---------------------------------------------------------------------------

/// `GET intake-issues/` + `GET inbox-issues/` (`base.py:176-221`):
/// ADMIN + MEMBER + GUEST. Guest row narrowing is [`guest_list_scoped`];
/// a missing intake row answers 404 ([`intake_not_found`], `:179-180`).
pub fn intake_issue_list_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER, GUEST], m)
}

/// `POST intake-issues/` + `POST inbox-issues/` (`base.py:221-326`):
/// ADMIN + MEMBER + GUEST (guests may file). The `Name is required` /
/// `Invalid priority` 400s precede any write (`:223-234`).
pub fn intake_issue_create_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER, GUEST], m)
}

/// `PATCH intake-issues/<pk>/` decorator
/// (`base.py:328`): ADMIN + creator(`Issue`).
pub fn intake_issue_update_gate(m: &Membership, is_creator: bool) -> Result<(), ErrorBody> {
    allow_creator_or_roles(&[ADMIN], m, is_creator)
}

/// `partial_update` membership gate (`base.py:355-359`): no active
/// `ProjectMember` row and no workspace admin → 403.
pub fn update_membership_gate(m: &Membership) -> Result<(), ErrorBody> {
    if m.project_role.is_none() && !m.workspace_admin {
        return Err(only_admin_or_creator());
    }
    Ok(())
}

/// `partial_update` low-role gate (`base.py:362-368`): a MEMBER-or-below
/// non-creator who is not a workspace admin → 400. The comparison is
/// numeric (`role <= GUEST`), so it catches GUEST and any lower custom
/// role.
pub fn update_creator_gate(m: &Membership, is_creator: bool) -> Result<(), ErrorBody> {
    let low = m.project_role.is_some_and(|role| role <= GUEST);
    if low && !m.workspace_admin && !is_creator {
        return Err(cannot_edit_intake_issues());
    }
    Ok(())
}

/// Keys a GUEST (or lower) role may write inside `issue_data`
/// (`base.py:398-403`); every other key is silently dropped, no error.
pub const GUEST_ISSUE_KEYS: [&str; 3] = ["name", "description_html", "description_json"];

/// Whether the caller's `issue_data` is narrowed to [`GUEST_ISSUE_KEYS`]
/// (`base.py:398`): any project membership at or below GUEST.
pub fn guest_issue_narrowed(m: &Membership) -> bool {
    m.project_role.is_some_and(|role| role <= GUEST)
}

/// Whether the caller may write intake fields (`base.py:422`): a project
/// role above MEMBER, or a workspace admin.
pub fn intake_fields_writable(m: &Membership) -> bool {
    m.project_role.is_some_and(|role| role > MEMBER) || m.workspace_admin
}

/// `GET intake-issues/<pk>/` decorator (`base.py:502`): ADMIN + MEMBER +
/// GUEST + creator(`Issue`). The guest creator check is
/// [`guest_view_gate`].
pub fn intake_issue_retrieve_gate(m: &Membership, is_creator: bool) -> Result<(), ErrorBody> {
    allow_creator_or_roles(&[ADMIN, MEMBER, GUEST], m, is_creator)
}

/// `DELETE intake-issues/<pk>/` decorator (`base.py:549`): ADMIN +
/// creator(`Issue`). The status cascade is [`destroy_cascades_to_issue`].
pub fn intake_issue_destroy_gate(m: &Membership, is_creator: bool) -> Result<(), ErrorBody> {
    allow_creator_or_roles(&[ADMIN], m, is_creator)
}

/// `PUT intake-issues/<pk>/` (update).
///
/// BUG-intake-issue-put: no `@allow_permission` decorates the `update`
/// action (only PATCH/GET/DELETE/POST are decorated, `base.py:94-566`),
/// so any authenticated user passes.
pub fn intake_issue_put_undecorated(authenticated: bool) -> Result<(), ErrorBody> {
    require_authenticated(authenticated)
}

/// Whether deleting the intake issue also deletes the parent `Issue`
/// (`base.py:560-565`): only statuses `-2, -1, 0, 2` cascade; any other
/// status deletes just the intake row.
pub fn destroy_cascades_to_issue(status: i32) -> bool {
    matches!(status, -2 | -1 | 0 | 2)
}

// ---------------------------------------------------------------------------
// Guest scoping (base.py:204-214, 531-545, 583-597)
// ---------------------------------------------------------------------------

/// `IntakeIssueViewSet.list` narrowing (`base.py:204-214`): an active
/// GUEST project membership without `project.guest_view_all_features`
/// narrows the queryset to `created_by=request.user`. The role test is an
/// exact `role == GUEST` filter, unlike the numeric `<=` tests in
/// `partial_update`.
pub fn guest_list_scoped(m: &Membership, guest_view_all_features: bool) -> bool {
    m.project_role == Some(GUEST) && !guest_view_all_features
}

/// `retrieve` (`base.py:531-545`) and versions `get` (`base.py:583-597`)
/// guest creator check: a GUEST without view-all reading an issue created
/// by someone else → 403. The versions endpoint tests the parent
/// `Issue.created_by`; retrieve tests `intake_issue.created_by` — both
/// reduce to the same `is_creator` input here.
pub fn guest_view_gate(
    m: &Membership,
    guest_view_all_features: bool,
    is_creator: bool,
) -> Result<(), ErrorBody> {
    if m.project_role == Some(GUEST) && !guest_view_all_features && !is_creator {
        return Err(not_allowed_to_view());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Versions endpoint (base.py:569-637)
// ---------------------------------------------------------------------------

/// `GET intake-work-items/<id>/description-versions[/<pk>/]`
/// (`base.py:578`): ADMIN + MEMBER + GUEST, no creator bypass. The guest
/// creator check is [`guest_view_gate`].
pub fn versions_gate(m: &Membership) -> Result<(), ErrorBody> {
    allow_project(&[ADMIN, MEMBER, GUEST], m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/app_intake/guards/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("guard fixture exists"))
            .expect("guard fixture parses")
    }

    fn membership(role: Option<i16>, workspace_member: bool, workspace_admin: bool) -> Membership {
        Membership {
            project_role: role,
            workspace_member,
            workspace_admin,
        }
    }

    /// Full member of the tenant: project + workspace rows, workspace admin
    /// only when asked (mirrors the `_build_tenant` seeder in
    /// `contract-tests/app_intake/conftest.py`).
    fn tenant(role: i16, workspace_admin: bool) -> Membership {
        membership(Some(role), true, workspace_admin)
    }

    fn assert_body(actual: ErrorBody, status: u16, body: Value) {
        assert_eq!(actual.status, status, "status for {body}");
        assert_eq!(actual.body, body);
    }

    fn matrix() -> Value {
        fixture("permissions.matrix.json")
    }

    fn matrix_row(action_prefix: &str) -> Value {
        matrix()
            .get("matrix")
            .and_then(Value::as_array)
            .expect("matrix array")
            .iter()
            .find(|row| {
                row.get("action")
                    .and_then(Value::as_str)
                    .is_some_and(|a| a.starts_with(action_prefix))
            })
            .unwrap_or_else(|| panic!("matrix lacks row {action_prefix}"))
            .clone()
    }

    // -- auth ----------------------------------------------------------------

    #[test]
    fn anonymous_rejected_before_role_check() {
        // Fixture `auth.anonymous` (IsAuthenticated, app/views/base.py:87).
        let m = matrix();
        let want = m
            .get("auth")
            .and_then(|a| a.get("anonymous"))
            .expect("auth.anonymous");
        let denied = require_authenticated(false).expect_err("anonymous denied");
        assert_body(
            denied,
            want.get("status").and_then(Value::as_u64).unwrap() as u16,
            want.get("body").expect("body").clone(),
        );
        assert!(require_authenticated(true).is_ok());
    }

    #[test]
    fn outsider_denied_with_error_shape() {
        // Fixture `auth.outsider` (pinned by test_outsider_denied_on_intake_list).
        let m = matrix();
        let want = m
            .get("auth")
            .and_then(|a| a.get("outsider"))
            .expect("auth.outsider");
        for gate in [
            intake_list_gate(&Membership::outsider()),
            intake_issue_list_gate(&Membership::outsider()),
            intake_issue_create_gate(&Membership::outsider()),
            versions_gate(&Membership::outsider()),
        ] {
            let denied = gate.expect_err("outsider denied");
            assert_body(
                denied,
                want.get("status").and_then(Value::as_u64).unwrap() as u16,
                want.get("body").expect("body").clone(),
            );
        }
    }

    #[test]
    fn workspace_admin_backdoor_passes_any_role_gate() {
        // Fixture `auth.workspace_admin_backdoor`
        // (app/permissions/base.py:86-96): a project member of ANY role who
        // is also a workspace ADMIN passes every allow_permission gate.
        let guest_admin = tenant(GUEST, true);
        assert!(intake_list_gate(&guest_admin).is_ok());
        assert!(
            intake_destroy_gate(&guest_admin, IntakeLookup::Found { is_default: false }).is_ok()
        );
    }

    // -- matrix rows ----------------------------------------------------------

    #[test]
    fn matrix_intake_list_admin_member_allow_guest_deny() {
        let row = matrix_row("GET intakes/");
        assert_eq!(row.get("admin").and_then(Value::as_u64).unwrap(), 200);
        assert_eq!(row.get("member").and_then(Value::as_u64).unwrap(), 200);
        assert_eq!(row.get("guest").and_then(Value::as_u64).unwrap(), 403);
        assert!(intake_list_gate(&tenant(ADMIN, false)).is_ok());
        assert!(intake_list_gate(&tenant(MEMBER, false)).is_ok());
        assert!(intake_list_gate(&tenant(GUEST, false)).is_err());
    }

    #[test]
    fn matrix_perform_create_bug_500_for_every_role() {
        // Fixture `bugs_port_as_is[0]`: the wrapper crashes before role
        // evaluation, so the guest gets 500, not 403.
        let row = matrix_row("POST intakes/");
        for role in ["admin", "guest", "member"] {
            assert_eq!(row.get(role).and_then(Value::as_u64).unwrap(), 500);
        }
        let denied = intake_perform_create_outcome().expect_err("always 500");
        assert_body(denied, 500, server_error().body);
        // Byte-exact wire body.
        assert_eq!(
            serde_json::to_string(&server_error().body).unwrap(),
            r#"{"error":"Something went wrong please try again later"}"#
        );
    }

    #[test]
    fn matrix_intake_detail_bug_any_authenticated_passes() {
        // Fixture `bugs_port_as_is[1]`: no decorator, membership unchecked.
        let row = matrix_row("GET intakes/<pk>/");
        for role in ["admin", "guest", "member"] {
            assert_eq!(row.get(role).and_then(Value::as_u64).unwrap(), 200);
        }
        assert!(intake_detail_undecorated(true).is_ok());
        // Even an outsider passes — that is the bug.
        assert!(intake_detail_undecorated(true).is_ok());
        assert_eq!(
            intake_detail_undecorated(false)
                .expect_err("anon 401")
                .status,
            401
        );
    }

    #[test]
    fn matrix_intake_destroy_roles_then_default_guard() {
        let row = matrix_row("DELETE intakes/");
        assert_eq!(row.get("guest").and_then(Value::as_u64).unwrap(), 403);
        assert!(intake_destroy_gate(
            &tenant(ADMIN, false),
            IntakeLookup::Found { is_default: false }
        )
        .is_ok());
        assert!(intake_destroy_gate(
            &tenant(MEMBER, false),
            IntakeLookup::Found { is_default: false }
        )
        .is_ok());
        assert!(intake_destroy_gate(
            &tenant(GUEST, false),
            IntakeLookup::Found { is_default: false }
        )
        .is_err());
    }

    #[test]
    fn matrix_intake_issue_list_create_all_roles() {
        let list = matrix_row("GET intake-issues/");
        let create = matrix_row("POST intake-issues/");
        for row in [list, create] {
            for role in ["admin", "guest", "member"] {
                assert_eq!(row.get(role).and_then(Value::as_u64).unwrap(), 200);
            }
        }
        for role in [ADMIN, MEMBER, GUEST] {
            assert!(intake_issue_list_gate(&tenant(role, false)).is_ok());
            assert!(intake_issue_create_gate(&tenant(role, false)).is_ok());
        }
    }

    #[test]
    fn matrix_partial_update_decorator_admin_or_creator() {
        // base.py:328: ADMIN + creator(Issue).
        assert!(intake_issue_update_gate(&tenant(ADMIN, false), false).is_ok());
        assert!(intake_issue_update_gate(&tenant(MEMBER, false), true).is_ok());
        assert!(intake_issue_update_gate(&tenant(GUEST, false), true).is_ok());
        // Plain member / guest, not creator: decorator 403 (the fixture's
        // "200 or 400" covers creator/edge shapes; the decorator denies a
        // non-creator non-admin before the inline gates run).
        assert!(intake_issue_update_gate(&tenant(MEMBER, false), false).is_err());
        assert!(intake_issue_update_gate(&tenant(GUEST, false), false).is_err());
        // Creator bypass needs workspace membership (base.py:26-30).
        let no_ws = membership(Some(MEMBER), false, false);
        assert!(intake_issue_update_gate(&no_ws, true).is_err());
    }

    #[test]
    fn matrix_partial_update_inline_gates() {
        // 403 gate (base.py:355-359): no project row, no workspace admin.
        let denied = update_membership_gate(&Membership::outsider()).expect_err("403");
        assert_body(denied, 403, only_admin_or_creator().body);
        assert!(update_membership_gate(&tenant(MEMBER, false)).is_ok());
        // Workspace admin with no project row still passes the 403 gate.
        assert!(update_membership_gate(&membership(None, true, true)).is_ok());

        // 400 gate (base.py:362-368): low-role non-creator, not admin.
        let denied = update_creator_gate(&tenant(GUEST, false), false).expect_err("400");
        assert_body(denied, 400, cannot_edit_intake_issues().body);
        // Creator, admin, or higher role passes.
        assert!(update_creator_gate(&tenant(GUEST, false), true).is_ok());
        assert!(update_creator_gate(&tenant(GUEST, true), false).is_ok());
        assert!(update_creator_gate(&tenant(MEMBER, false), false).is_ok());
        // Byte-exact wire bodies.
        assert_eq!(
            serde_json::to_string(&only_admin_or_creator().body).unwrap(),
            r#"{"error":"Only admin or creator can update the intake work items"}"#
        );
        assert_eq!(
            serde_json::to_string(&cannot_edit_intake_issues().body).unwrap(),
            r#"{"error":"You cannot edit intake issues"}"#
        );
    }

    #[test]
    fn matrix_retrieve_roles_then_guest_creator_check() {
        // Decorator (base.py:502): ADMIN + MEMBER + GUEST + creator.
        assert!(intake_issue_retrieve_gate(&tenant(ADMIN, false), false).is_ok());
        assert!(intake_issue_retrieve_gate(&tenant(MEMBER, false), false).is_ok());
        assert!(intake_issue_retrieve_gate(&tenant(GUEST, false), false).is_ok());
        assert!(intake_issue_retrieve_gate(&tenant(MEMBER, false), true).is_ok());
        assert!(intake_issue_retrieve_gate(&Membership::outsider(), false).is_err());
        // Inline guest check (guest_scoping fixture, retrieve case).
        assert!(guest_view_gate(&tenant(GUEST, false), false, true).is_ok());
        let denied = guest_view_gate(&tenant(GUEST, false), false, false).expect_err("403");
        assert_body(denied, 403, not_allowed_to_view().body);
        assert!(guest_view_gate(&tenant(GUEST, false), true, false).is_ok());
        assert_eq!(
            serde_json::to_string(&not_allowed_to_view().body).unwrap(),
            r#"{"error":"You are not allowed to view this issue"}"#
        );
    }

    #[test]
    fn matrix_destroy_issue_admin_or_creator() {
        // Decorator (base.py:549): ADMIN + creator(Issue).
        assert!(intake_issue_destroy_gate(&tenant(ADMIN, false), false).is_ok());
        assert!(intake_issue_destroy_gate(&tenant(MEMBER, false), true).is_ok());
        assert!(intake_issue_destroy_gate(&tenant(GUEST, false), true).is_ok());
        assert!(intake_issue_destroy_gate(&tenant(MEMBER, false), false).is_err());
        assert!(intake_issue_destroy_gate(&tenant(GUEST, false), false).is_err());
    }

    #[test]
    fn matrix_versions_roles_then_guest_creator_check() {
        // Decorator (base.py:578): ADMIN + MEMBER + GUEST, no creator
        // bypass — a non-member creator still needs a membership row.
        assert!(versions_gate(&tenant(ADMIN, false)).is_ok());
        assert!(versions_gate(&tenant(MEMBER, false)).is_ok());
        assert!(versions_gate(&tenant(GUEST, false)).is_ok());
        assert!(versions_gate(&Membership::outsider()).is_err());
        // Inline guest check (guest_scoping fixture, versions case).
        assert!(guest_view_gate(&tenant(GUEST, false), false, true).is_ok());
        let denied = guest_view_gate(&tenant(GUEST, false), false, false).expect_err("403");
        assert_body(denied, 403, not_allowed_to_view().body);
    }

    #[test]
    fn matrix_put_undecorated_bug() {
        // Fixture `bugs_port_as_is[2]`: PUT carries no decorator.
        assert!(intake_issue_put_undecorated(true).is_ok());
        assert_eq!(
            intake_issue_put_undecorated(false)
                .expect_err("anon 401")
                .status,
            401
        );
    }

    // -- guest scoping, two users ---------------------------------------------

    #[test]
    fn guest_scoping_two_users_list() {
        let gold = fixture("guest_scoping.golden.json");
        assert!(gold
            .get("cases")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .any(|c| c
                .get("where")
                .and_then(Value::as_str)
                .is_some_and(|w| w.contains("list"))));
        // Owner (ADMIN) is never scoped; guest without view-all is scoped.
        assert!(!guest_list_scoped(&tenant(ADMIN, false), false));
        assert!(!guest_list_scoped(&tenant(MEMBER, false), false));
        assert!(guest_list_scoped(&tenant(GUEST, false), false));
        // View-all flips the guest back to full rows.
        assert!(!guest_list_scoped(&tenant(GUEST, false), true));
        // Outsider (no project row) is not "scoped" — it is denied outright.
        assert!(!guest_list_scoped(&Membership::outsider(), false));
    }

    #[test]
    fn guest_issue_data_narrowing_retained_keys() {
        let gold = fixture("guest_scoping.golden.json");
        let narrow = gold
            .get("cases")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .find(|c| {
                c.get("where")
                    .and_then(Value::as_str)
                    .is_some_and(|w| w.contains("narrowing"))
            })
            .expect("narrowing case");
        let want: Vec<String> = narrow
            .get("retained_keys")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|k| k.as_str().map(str::to_owned))
            .collect();
        assert_eq!(
            GUEST_ISSUE_KEYS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            want
        );
        // Numeric <= GUEST (base.py:398): GUEST and lower narrow…
        assert!(guest_issue_narrowed(&tenant(GUEST, false)));
        assert!(guest_issue_narrowed(&membership(Some(1), true, false)));
        // …MEMBER and above, and non-members, do not.
        assert!(!guest_issue_narrowed(&tenant(MEMBER, false)));
        assert!(!guest_issue_narrowed(&tenant(ADMIN, false)));
        assert!(!guest_issue_narrowed(&Membership::outsider()));
    }

    #[test]
    fn intake_fields_writable_above_member_or_admin() {
        // base.py:422: role > MEMBER or workspace admin.
        assert!(intake_fields_writable(&tenant(ADMIN, false)));
        assert!(!intake_fields_writable(&tenant(MEMBER, false)));
        assert!(!intake_fields_writable(&tenant(GUEST, false)));
        assert!(intake_fields_writable(&tenant(GUEST, true)));
        assert!(intake_fields_writable(&membership(None, true, true)));
        assert!(!intake_fields_writable(&Membership::outsider()));
    }

    // -- default-intake delete -------------------------------------------------

    #[test]
    fn default_intake_delete_guard() {
        let gold = fixture("default_intake_delete.golden.json");
        let cases = gold.get("cases").and_then(Value::as_array).expect("cases");
        assert_eq!(cases.len(), 2);
        // Default → 400 with the exact body (base.py:84-89).
        let denied = intake_destroy_gate(
            &tenant(ADMIN, false),
            IntakeLookup::Found { is_default: true },
        )
        .expect_err("default 400");
        assert_body(denied, 400, cannot_delete_default_intake().body);
        assert_eq!(
            serde_json::to_string(&cannot_delete_default_intake().body).unwrap(),
            r#"{"error":"You cannot delete the default intake"}"#
        );
        // Non-default → delete proceeds (204 in the view).
        assert!(intake_destroy_gate(
            &tenant(ADMIN, false),
            IntakeLookup::Found { is_default: false }
        )
        .is_ok());
        // Missing row → None.is_default AttributeError → 500, never 404
        // (fixture `bug_port_as_is`).
        let bug = gold.get("bug_port_as_is").expect("bug case");
        assert!(bug
            .get("trace")
            .and_then(Value::as_str)
            .unwrap()
            .contains("base.py:83"));
        let denied =
            intake_destroy_gate(&tenant(ADMIN, false), IntakeLookup::Missing).expect_err("500");
        assert_body(denied, 500, server_error().body);
    }

    // -- destroy cascade --------------------------------------------------------

    #[test]
    fn destroy_cascade_per_status_value() {
        // base.py:560-565: statuses [-2,-1,0,2] also delete the Issue.
        for status in [-2, -1, 0, 2] {
            assert!(
                destroy_cascades_to_issue(status),
                "status {status} cascades"
            );
        }
        for status in [-3, -4, 1, 3, 5, 100] {
            assert!(
                !destroy_cascades_to_issue(status),
                "status {status} keeps Issue"
            );
        }
    }
}
