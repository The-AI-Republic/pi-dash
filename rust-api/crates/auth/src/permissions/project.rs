#![forbid(unsafe_code)]

//! Project permission classes and the state-mutation helper
//! (`app/permissions/project.py`, duplicated in `utils/permissions/project.py`).
//!
//! `ROLE` here is `pi_dash/db/models/project.py` (`ADMIN=20`, `MEMBER=15`,
//! `GUEST=5`). `view.workspace_slug` / `view.project_id` scope every query
//! and are carried as facts checked against the request scope.

use pidash_types::{ProjectId, WorkspaceId};

use super::{scope_allows, ROLE_ADMIN, ROLE_MEMBER};
use crate::scope::TenantScope;

/// Pre-fetched membership facts for one `(user, slug, project_id)`.
///
/// Each boolean mirrors one `...objects.filter(...).exists()` with the
/// caller's SQL carrying the `is_active=True` filter.
pub struct ProjectFacts {
    pub workspace: WorkspaceId,
    pub project_id: ProjectId,
    pub authenticated: bool,
    /// Active workspace membership, any role.
    pub is_workspace_member: bool,
    /// Active workspace membership with role in `[ADMIN, MEMBER]`.
    pub has_workspace_admin_or_member: bool,
    /// Active workspace membership with role `ADMIN`.
    pub is_workspace_admin: bool,
    /// Active project membership, any role.
    pub is_project_member: bool,
    /// Active project membership with role `ADMIN`.
    pub is_project_admin: bool,
    /// Active project membership with role in `[ADMIN, MEMBER]`.
    pub has_project_admin_or_member: bool,
    /// Active project membership matched by `project__identifier`
    /// (the `project_identifier` view branch).
    pub has_identifier_membership: bool,
    /// Whether the view carries a `project_identifier` (safe methods only).
    pub has_project_identifier: bool,
}

/// `ProjectBasePermission`: safe methods need workspace membership
/// (filtering in the queryset); POST needs workspace Admin/Member; anything
/// else needs project Admin, or project membership plus workspace Admin.
pub fn decide_project_base(method: &str, scope: &TenantScope, facts: &ProjectFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    if super::is_safe_method(method) {
        return facts.is_workspace_member;
    }
    if method == "POST" {
        return facts.has_workspace_admin_or_member;
    }
    if facts.is_project_admin {
        return true;
    }
    facts.is_project_member && facts.is_workspace_admin
}

/// `ProjectMemberPermission`: safe methods need any active project row in
/// the workspace (no project filter in Python); POST needs workspace
/// Admin/Member; anything else needs project Admin/Member.
pub fn decide_project_member(method: &str, scope: &TenantScope, facts: &ProjectFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    if super::is_safe_method(method) {
        return facts.is_project_member;
    }
    if method == "POST" {
        return facts.has_workspace_admin_or_member;
    }
    facts.has_project_admin_or_member
}

/// `ProjectEntityPermission`: with a `project_identifier` on the view, safe
/// methods check that identifier's membership; otherwise safe methods need
/// project membership and writes need project Admin/Member.
pub fn decide_project_entity(method: &str, scope: &TenantScope, facts: &ProjectFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    if facts.has_project_identifier && super::is_safe_method(method) {
        return facts.has_identifier_membership;
    }
    if super::is_safe_method(method) {
        return facts.is_project_member;
    }
    facts.has_project_admin_or_member
}

/// `ProjectAdminPermission`: active project membership with role `ADMIN`.
pub fn decide_project_admin(scope: &TenantScope, facts: &ProjectFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    facts.is_project_admin
}

/// `ProjectLitePermission`: any active project membership.
pub fn decide_project_lite(scope: &TenantScope, facts: &ProjectFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    facts.is_project_member
}

/// Facts for `can_mutate_states`: the caller's single membership row plus
/// the project's `members_can_edit_states` flag and the workspace-admin
/// check. `project_role` is `None` when no active membership row exists.
pub struct StateMutationFacts {
    pub authenticated: bool,
    pub project_role: Option<i32>,
    pub members_can_edit_states: bool,
    pub is_workspace_admin: bool,
}

/// `can_mutate_states`: project admins always; members when the project
/// allows it; workspace admins who are project members (a missing
/// membership row denies before the admin override is reached).
pub fn can_mutate_states(facts: &StateMutationFacts) -> bool {
    if !facts.authenticated {
        return false;
    }
    match facts.project_role {
        None => false,
        Some(role) if role == ROLE_ADMIN => true,
        Some(role) if role == ROLE_MEMBER && facts.members_can_edit_states => true,
        _ => facts.is_workspace_admin,
    }
}

/// `ProjectStateEntityPermission`: any active project member may read;
/// writes go through [`can_mutate_states`].
pub fn decide_project_state_entity(
    method: &str,
    scope: &TenantScope,
    facts: &ProjectFacts,
    mutation: &StateMutationFacts,
) -> bool {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return false;
    }
    if super::is_safe_method(method) {
        return facts.is_project_member;
    }
    can_mutate_states(mutation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn facts() -> ProjectFacts {
        ProjectFacts {
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

    fn mutation(role: Option<i32>) -> StateMutationFacts {
        StateMutationFacts {
            authenticated: true,
            project_role: role,
            members_can_edit_states: false,
            is_workspace_admin: false,
        }
    }

    #[test]
    fn base_safe_needs_workspace_membership() {
        let mut f = facts();
        assert!(!decide_project_base("GET", &scope(), &f));
        f.is_workspace_member = true;
        assert!(decide_project_base("GET", &scope(), &f));
    }

    #[test]
    fn base_post_needs_workspace_admin_or_member() {
        let mut f = facts();
        assert!(!decide_project_base("POST", &scope(), &f));
        f.has_workspace_admin_or_member = true;
        assert!(decide_project_base("POST", &scope(), &f));
    }

    #[test]
    fn base_write_admin_or_member_plus_workspace_admin() {
        let mut f = facts();
        assert!(!decide_project_base("PATCH", &scope(), &f));
        f.is_project_admin = true;
        assert!(decide_project_base("PATCH", &scope(), &f));
        f.is_project_admin = false;
        f.is_project_member = true;
        assert!(!decide_project_base("PATCH", &scope(), &f));
        f.is_workspace_admin = true;
        assert!(decide_project_base("PATCH", &scope(), &f));
    }

    #[test]
    fn member_perm_write_needs_project_admin_or_member() {
        let mut f = facts();
        f.is_project_member = true;
        assert!(decide_project_member("GET", &scope(), &f));
        assert!(!decide_project_member("PUT", &scope(), &f));
        f.has_project_admin_or_member = true;
        assert!(decide_project_member("PUT", &scope(), &f));
    }

    #[test]
    fn entity_identifier_branch_only_for_safe() {
        let mut f = facts();
        f.has_project_identifier = true;
        f.has_identifier_membership = true;
        assert!(decide_project_entity("GET", &scope(), &f));
        // Writes ignore the identifier branch.
        assert!(!decide_project_entity("PUT", &scope(), &f));
        f.has_project_admin_or_member = true;
        assert!(decide_project_entity("PUT", &scope(), &f));
    }

    #[test]
    fn admin_and_lite() {
        let mut f = facts();
        f.is_project_member = true;
        assert!(!decide_project_admin(&scope(), &f));
        assert!(decide_project_lite(&scope(), &f));
        f.is_project_admin = true;
        assert!(decide_project_admin(&scope(), &f));
    }

    #[test]
    fn mutate_states_matrix() {
        assert!(can_mutate_states(&mutation(Some(ROLE_ADMIN))));
        assert!(!can_mutate_states(&mutation(None)));
        assert!(!can_mutate_states(&mutation(Some(ROLE_MEMBER))));
        let mut flag = mutation(Some(ROLE_MEMBER));
        flag.members_can_edit_states = true;
        assert!(can_mutate_states(&flag));
        // Guest with the flag on still denies.
        let mut guest = mutation(Some(super::super::ROLE_GUEST));
        guest.members_can_edit_states = true;
        assert!(!can_mutate_states(&guest));
        // Workspace admin override still needs a membership row.
        let mut admin = mutation(Some(super::super::ROLE_GUEST));
        admin.is_workspace_admin = true;
        assert!(can_mutate_states(&admin));
        let mut no_row = mutation(None);
        no_row.is_workspace_admin = true;
        assert!(!can_mutate_states(&no_row));
        let mut anon = mutation(Some(ROLE_ADMIN));
        anon.authenticated = false;
        assert!(!can_mutate_states(&anon));
    }

    #[test]
    fn state_entity_read_vs_write() {
        let mut f = facts();
        f.is_project_member = true;
        let m = mutation(Some(ROLE_MEMBER));
        assert!(decide_project_state_entity("GET", &scope(), &f, &m));
        assert!(!decide_project_state_entity("POST", &scope(), &f, &m));
        let admin_mut = mutation(Some(ROLE_ADMIN));
        assert!(decide_project_state_entity(
            "POST",
            &scope(),
            &f,
            &admin_mut
        ));
    }

    #[test]
    fn cross_workspace_denies() {
        let other = TenantScope::new(WorkspaceId::from("other"));
        let mut f = facts();
        f.is_workspace_member = true;
        f.is_project_admin = true;
        assert!(!decide_project_base("GET", &other, &f));
        assert!(!decide_project_admin(&other, &f));
    }
}
