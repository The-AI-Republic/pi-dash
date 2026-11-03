#![forbid(unsafe_code)]

//! Workspace permission classes (`app/permissions/workspace.py`, duplicated
//! in `utils/permissions/workspace.py`).
//!
//! Role literals in this file are the module constants `Admin=20`,
//! `Member=15`, `Guest=5`. `view.workspace_slug` is the slug every query
//! filters on, carried here as [`WorkspaceFacts::workspace`] and checked
//! against the request scope.

use pidash_types::WorkspaceId;

use super::scope_allows;
use crate::scope::TenantScope;

/// Pre-fetched membership facts for one `(user, workspace_slug)`.
///
/// Each boolean mirrors one `WorkspaceMember.objects.filter(...).exists()`
/// with the caller's SQL carrying the `is_active=True` filter where the
/// class uses it — except [`decide_workspace_owner`], which ports the
/// missing `is_active` filter exactly as written (see below).
pub struct WorkspaceFacts {
    pub workspace: WorkspaceId,
    pub authenticated: bool,
    /// Active membership with role in `[Admin, Member]`.
    pub has_admin_or_member_role: bool,
    /// Active membership with role `Admin`.
    pub has_admin_role: bool,
    /// Active membership, any role.
    pub is_member: bool,
    /// Membership with role `Admin`, any active state (the owner check has
    /// no `is_active` filter in Python — ported as written).
    pub is_admin_unfiltered: bool,
}

/// `WorkSpaceBasePermission`: anyone may POST a workspace (creation needs
/// no membership); safe methods pass; PUT/PATCH need Admin/Member; DELETE
/// needs Admin. Anonymous is refused first, so POST still needs a login.
/// Unmatched methods fall off the end of the Python method (`None`) and are
/// denied; the kernel returns `false` there.
pub fn decide_workspace_base(method: &str, scope: &TenantScope, facts: &WorkspaceFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    if method == "POST" {
        return true;
    }
    if super::is_safe_method(method) {
        return true;
    }
    if method == "PUT" || method == "PATCH" {
        return facts.has_admin_or_member_role;
    }
    if method == "DELETE" {
        return facts.has_admin_role;
    }
    false
}

/// `WorkspaceOwnerPermission`: role `Admin`, with no `is_active` filter —
/// inactive admin rows pass. Ported as written.
pub fn decide_workspace_owner(scope: &TenantScope, facts: &WorkspaceFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    facts.is_admin_unfiltered
}

/// `WorkSpaceAdminPermission`: active Admin/Member.
pub fn decide_workspace_admin(scope: &TenantScope, facts: &WorkspaceFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    facts.has_admin_or_member_role
}

/// `WorkspaceEntityPermission`: safe methods need any active membership
/// (filtering happens in the queryset); writes need Admin/Member.
pub fn decide_workspace_entity(method: &str, scope: &TenantScope, facts: &WorkspaceFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    if super::is_safe_method(method) {
        return facts.is_member;
    }
    facts.has_admin_or_member_role
}

/// `WorkspaceViewerPermission` and `WorkspaceUserPermission`: identical in
/// Python — any active membership, every method.
pub fn decide_workspace_viewer(scope: &TenantScope, facts: &WorkspaceFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    facts.is_member
}

/// `WorkspaceViewerPermission` and `WorkspaceUserPermission`: identical in
/// Python — any active membership, every method.
pub fn decide_workspace_user(scope: &TenantScope, facts: &WorkspaceFacts) -> bool {
    decide_workspace_viewer(scope, facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn facts() -> WorkspaceFacts {
        WorkspaceFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            has_admin_or_member_role: false,
            has_admin_role: false,
            is_member: false,
            is_admin_unfiltered: false,
        }
    }

    #[test]
    fn anonymous_denied_everywhere() {
        let mut f = facts();
        f.authenticated = false;
        f.is_member = true;
        f.has_admin_or_member_role = true;
        assert!(!decide_workspace_base("POST", &scope(), &f));
        assert!(!decide_workspace_base("GET", &scope(), &f));
        assert!(!decide_workspace_owner(&scope(), &f));
        assert!(!decide_workspace_admin(&scope(), &f));
        assert!(!decide_workspace_entity("GET", &scope(), &f));
        assert!(!decide_workspace_viewer(&scope(), &f));
        assert!(!decide_workspace_user(&scope(), &f));
    }

    #[test]
    fn base_post_and_safe_pass_for_any_login() {
        let f = facts();
        assert!(decide_workspace_base("POST", &scope(), &f));
        assert!(decide_workspace_base("GET", &scope(), &f));
        assert!(decide_workspace_base("HEAD", &scope(), &f));
        assert!(decide_workspace_base("OPTIONS", &scope(), &f));
    }

    #[test]
    fn base_write_needs_role() {
        let mut f = facts();
        assert!(!decide_workspace_base("PUT", &scope(), &f));
        assert!(!decide_workspace_base("PATCH", &scope(), &f));
        assert!(!decide_workspace_base("DELETE", &scope(), &f));
        f.has_admin_or_member_role = true;
        assert!(decide_workspace_base("PUT", &scope(), &f));
        assert!(decide_workspace_base("PATCH", &scope(), &f));
        assert!(!decide_workspace_base("DELETE", &scope(), &f));
        f.has_admin_role = true;
        assert!(decide_workspace_base("DELETE", &scope(), &f));
    }

    #[test]
    fn base_unknown_method_denies_like_falling_off_the_end() {
        let mut f = facts();
        f.has_admin_or_member_role = true;
        f.has_admin_role = true;
        assert!(!decide_workspace_base("TRACE", &scope(), &f));
    }

    #[test]
    fn owner_ignores_active_state() {
        let mut f = facts();
        f.is_admin_unfiltered = true;
        assert!(decide_workspace_owner(&scope(), &f));
        f.is_admin_unfiltered = false;
        assert!(!decide_workspace_owner(&scope(), &f));
    }

    #[test]
    fn entity_safe_needs_membership_write_needs_role() {
        let mut f = facts();
        assert!(!decide_workspace_entity("GET", &scope(), &f));
        f.is_member = true;
        assert!(decide_workspace_entity("GET", &scope(), &f));
        assert!(!decide_workspace_entity("POST", &scope(), &f));
        f.has_admin_or_member_role = true;
        assert!(decide_workspace_entity("POST", &scope(), &f));
    }

    #[test]
    fn viewer_and_user_are_identical() {
        let mut f = facts();
        f.is_member = true;
        assert!(decide_workspace_viewer(&scope(), &f));
        assert!(decide_workspace_user(&scope(), &f));
        f.is_member = false;
        assert!(!decide_workspace_viewer(&scope(), &f));
        assert!(!decide_workspace_user(&scope(), &f));
    }

    #[test]
    fn cross_workspace_denies() {
        let other = TenantScope::new(WorkspaceId::from("other"));
        let mut f = facts();
        f.is_member = true;
        f.has_admin_or_member_role = true;
        assert!(!decide_workspace_base("GET", &other, &f));
        assert!(!decide_workspace_viewer(&other, &f));
    }
}
