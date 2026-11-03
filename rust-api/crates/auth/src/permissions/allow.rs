#![forbid(unsafe_code)]

//! The `@allow_permission` decorator (`app/permissions/base.py`, duplicated
//! in `utils/permissions/base.py`) as a pure decision function.
//!
//! The decorator wraps a view and either calls it or answers `403
//! {"error": "You don't have the required permissions."}` (compact DRF
//! JSON). The two source trees differ in exactly one place: the `app` copy
//! refuses non-workspace-members before honoring the creator bypass, while
//! the `utils` copy honors the creator bypass unconditionally. Both are
//! preserved via [`CreatorGate`]; every other branch is shared.
//!
//! `allowed_roles` conversion (`role.value if isinstance(role, ROLE)`)
//! happens before the query, so the kernel takes the precomputed
//! membership booleans and the caller converts its role lists.

use pidash_types::WorkspaceId;

use super::scope_allows;
use crate::scope::TenantScope;

/// Which `@allow_permission` branch the decorator enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowLevel {
    /// `level="WORKSPACE"`: the `slug` membership with an allowed role.
    Workspace,
    /// Any other level (including the default `"PROJECT"`): the
    /// project-membership rule plus the workspace-admin bypass.
    Project,
}

/// Which source tree's creator branch to enforce.
///
/// `App` is `app/permissions/base.py` (membership gate first); `Utils` is
/// `utils/permissions/base.py` (creator bypass first). This is the one
/// intentional divergence between the two copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreatorGate {
    App,
    Utils,
}

/// How the decorator is configured for one view.
pub struct AllowSpec {
    pub level: AllowLevel,
    pub creator_gate: CreatorGate,
    /// `creator and model` both set: the creator bypass is live.
    pub creator_bypass: bool,
}

/// Pre-fetched membership facts for one `(user, slug[, project_id])`.
///
/// Each `*_exists` mirrors one `Model.objects.filter(...).exists()` in the
/// decorator: active-row filters (`is_active=True`) are part of the
/// caller's SQL, and `workspace` is the slug that SQL filtered on.
pub struct AllowFacts {
    pub workspace: WorkspaceId,
    /// `request.user` authenticated (anonymous users match no row).
    pub authenticated: bool,
    /// Active workspace membership, any role.
    pub is_workspace_member: bool,
    /// Active workspace membership with a role in `allowed_roles`.
    pub has_allowed_workspace_role: bool,
    /// `model.objects.filter(id=pk, created_by=user).exists()`.
    pub is_creator: bool,
    /// Active project membership with a role in `allowed_roles`.
    pub has_allowed_project_role: bool,
    /// Active project membership, any role.
    pub is_project_member: bool,
    /// Active workspace membership with role exactly `ADMIN`
    /// (`role=ROLE.ADMIN.value`, not `>=`).
    pub is_workspace_admin: bool,
}

/// Mirror of the decorator body: true when the view runs.
pub fn decide_allow(spec: &AllowSpec, scope: &TenantScope, facts: &AllowFacts) -> bool {
    if !scope_allows(scope, &facts.workspace) {
        return false;
    }
    if !facts.authenticated {
        return false;
    }
    if spec.creator_bypass {
        match spec.creator_gate {
            CreatorGate::App => {
                // The app copy refuses non-members up front, without
                // reaching the role checks below.
                if !facts.is_workspace_member {
                    return false;
                }
                if facts.is_creator {
                    return true;
                }
            }
            CreatorGate::Utils => {
                if facts.is_creator {
                    return true;
                }
            }
        }
    }
    match spec.level {
        AllowLevel::Workspace => facts.has_allowed_workspace_role,
        AllowLevel::Project => {
            if facts.has_allowed_project_role {
                return true;
            }
            // Workspace admins who are part of the project pass regardless
            // of their project role.
            facts.is_project_member && facts.is_workspace_admin
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::Role;

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn facts() -> AllowFacts {
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        }
    }

    fn spec(level: AllowLevel) -> AllowSpec {
        AllowSpec {
            level,
            creator_gate: CreatorGate::App,
            creator_bypass: false,
        }
    }

    #[test]
    fn anonymous_is_denied() {
        let mut f = facts();
        f.authenticated = false;
        assert!(!decide_allow(&spec(AllowLevel::Workspace), &scope(), &f));
        assert!(!decide_allow(&spec(AllowLevel::Project), &scope(), &f));
    }

    #[test]
    fn cross_workspace_facts_deny() {
        let other = TenantScope::new(WorkspaceId::from("other"));
        assert!(!decide_allow(
            &spec(AllowLevel::Workspace),
            &other,
            &facts()
        ));
    }

    #[test]
    fn workspace_level_needs_allowed_role() {
        assert!(decide_allow(
            &spec(AllowLevel::Workspace),
            &scope(),
            &facts()
        ));
        let mut f = facts();
        f.has_allowed_workspace_role = false;
        assert!(!decide_allow(&spec(AllowLevel::Workspace), &scope(), &f));
    }

    #[test]
    fn project_level_allowed_role_passes() {
        let mut f = facts();
        f.has_allowed_project_role = true;
        assert!(decide_allow(&spec(AllowLevel::Project), &scope(), &f));
    }

    #[test]
    fn project_level_workspace_admin_bypass_needs_membership() {
        let mut both = facts();
        both.is_project_member = true;
        both.is_workspace_admin = true;
        assert!(decide_allow(&spec(AllowLevel::Project), &scope(), &both));

        let mut no_project = facts();
        no_project.is_workspace_admin = true;
        assert!(!decide_allow(
            &spec(AllowLevel::Project),
            &scope(),
            &no_project
        ));

        let mut no_admin = facts();
        no_admin.is_project_member = true;
        assert!(!decide_allow(
            &spec(AllowLevel::Project),
            &scope(),
            &no_admin
        ));
    }

    #[test]
    fn app_gate_refuses_non_member_before_creator_check() {
        let spec = AllowSpec {
            level: AllowLevel::Project,
            creator_gate: CreatorGate::App,
            creator_bypass: true,
        };
        let mut f = facts();
        f.is_workspace_member = false;
        f.is_creator = true;
        f.has_allowed_project_role = true;
        // Membership gate fires first: even the creator with an allowed
        // project role is refused.
        assert!(!decide_allow(&spec, &scope(), &f));

        let mut member = facts();
        member.is_creator = true;
        assert!(decide_allow(&spec, &scope(), &member));
    }

    #[test]
    fn utils_gate_honors_creator_without_membership() {
        let spec = AllowSpec {
            level: AllowLevel::Project,
            creator_gate: CreatorGate::Utils,
            creator_bypass: true,
        };
        let mut f = facts();
        f.is_workspace_member = false;
        f.is_creator = true;
        assert!(decide_allow(&spec, &scope(), &f));

        f.is_creator = false;
        assert!(!decide_allow(&spec, &scope(), &f));
    }

    #[test]
    fn role_values_convert_like_the_decorator() {
        assert_eq!(Role::Admin.value(), crate::permissions::ROLE_ADMIN);
    }
}
