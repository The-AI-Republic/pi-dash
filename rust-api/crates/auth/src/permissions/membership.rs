#![forbid(unsafe_code)]

//! Workspace/project membership and role helpers
//! (`pi_dash/core/permissions.py`, the single source of truth the assistant
//! tool layer shares with the views; `runner/services/permissions.py`
//! re-exports them).
//!
//! Each helper takes the role the caller's SQL already fetched
//! (`WorkspaceMember` row with `is_active=True`, `.values_list("role")`
//! `.first()`): `None` means "no active row", which is also what anonymous
//! callers pass, since the Python guards return `False`/`None` for them.
//! Role comparisons are `>=`, except [`check_project_role`]'s
//! workspace-admin bypass, which needs role exactly `ADMIN` (`role=...`,
//! not `>=` — ported as written).

use super::{ROLE_ADMIN, ROLE_MEMBER};

/// `is_workspace_member`: an active membership row exists.
pub fn is_workspace_member(role: Option<i32>) -> bool {
    role.is_some()
}

/// `is_workspace_admin`: active role `>= ADMIN` (20).
pub fn is_workspace_admin(role: Option<i32>) -> bool {
    matches!(role, Some(role) if role >= ROLE_ADMIN)
}

/// `is_at_least_member`: active role `>= MEMBER` (15) — not Guest.
pub fn is_at_least_member(role: Option<i32>) -> bool {
    matches!(role, Some(role) if role >= ROLE_MEMBER)
}

/// Facts for [`check_project_role`]: the three `EXISTS` queries of the
/// `PROJECT` branch, with `allowed_roles` already converted to values
/// (`[int(r) for r in allowed_roles]`).
pub struct ProjectRoleFacts {
    pub authenticated: bool,
    /// Active `ProjectMember` row with a role in `allowed_roles`.
    pub has_allowed_role: bool,
    /// Active `ProjectMember` row, any role.
    pub is_project_member: bool,
    /// Active `WorkspaceMember` row with role exactly `ADMIN`.
    pub is_workspace_admin: bool,
}

/// Mirror of `check_project_role`: allowed project role passes, else (when
/// `allow_workspace_admin_bypass`) project membership plus workspace Admin.
pub fn check_project_role(facts: &ProjectRoleFacts, allow_workspace_admin_bypass: bool) -> bool {
    if !facts.authenticated {
        return false;
    }
    if facts.has_allowed_role {
        return true;
    }
    if allow_workspace_admin_bypass && facts.is_project_member && facts.is_workspace_admin {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::ROLE_GUEST;

    #[test]
    fn membership_predicates() {
        assert!(!is_workspace_member(None));
        assert!(is_workspace_member(Some(ROLE_GUEST)));
        assert!(!is_workspace_admin(None));
        assert!(!is_workspace_admin(Some(ROLE_MEMBER)));
        assert!(is_workspace_admin(Some(ROLE_ADMIN)));
        assert!(!is_at_least_member(Some(ROLE_GUEST)));
        assert!(is_at_least_member(Some(ROLE_MEMBER)));
        assert!(is_at_least_member(Some(ROLE_ADMIN)));
    }

    fn facts() -> ProjectRoleFacts {
        ProjectRoleFacts {
            authenticated: true,
            has_allowed_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        }
    }

    #[test]
    fn project_role_mirrors_allow_project_branch() {
        let mut f = facts();
        assert!(!check_project_role(&f, true));
        f.has_allowed_role = true;
        assert!(check_project_role(&f, true));
        f.has_allowed_role = false;
        f.is_project_member = true;
        f.is_workspace_admin = true;
        assert!(check_project_role(&f, true));
        // Bypass disabled: membership plus admin is not enough.
        assert!(!check_project_role(&f, false));
        f.authenticated = false;
        assert!(!check_project_role(&f, true));
    }
}
