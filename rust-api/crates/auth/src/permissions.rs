#![forbid(unsafe_code)]

//! Permission kernel (F-06): every access rule the Django backend enforces.
//!
//! Python references (one tree replaces all of these):
//!
//! - `pi_dash/app/permissions/` (`base.py`, `workspace.py`, `project.py`,
//!   `page.py`) and its byte-identical copy `pi_dash/utils/permissions/`.
//! - `pi_dash/core/permissions.py` — workspace/project role helpers, the
//!   single source of truth the assistant tool layer shares with the views.
//! - `pi_dash/runner/services/permissions.py` — runner visibility and
//!   management rules (workspace helpers re-exported from core).
//! - `pi_dash/managed_runner/permissions.py` — `IsDesktopSession`.
//! - `pi_dash/license/api/permissions/instance.py` —
//!   `InstanceAdminPermission`.
//!
//! The modules are pure, like the rest of this crate: every
//! `Model.objects.filter(...).exists()` check becomes an explicit boolean
//! (or role) in a `*Facts` struct, and row fetching stays the caller's SQL.
//! Each `decide_*` function additionally takes the request's [`TenantScope`]
//! and denies when the facts were fetched for a different workspace, so a
//! handler cannot authorize outside its workspace even with stale facts —
//! the workspace-scoped handle by construction. (`TenantScope` itself is
//! deny-by-default: only the exact same workspace authorizes.)
//!
//! Ported bugs and divergences (kept byte-for-byte, listed for follow-up):
//!
//! - `app/permissions/base.py` gates the creator bypass on workspace
//!   membership first, while `utils/permissions/base.py` honors it
//!   unconditionally ([`allow::CreatorGate`]).
//! - `WorkspaceOwnerPermission` omits `is_active=True` (inactive admins
//!   pass); every sibling filters active rows.
//! - `WorkspaceViewerPermission` and `WorkspaceUserPermission` are
//!   identical (any active membership).
//! - `WorkSpaceBasePermission` falls off the end (returns `None`, denied)
//!   for unmatched methods; the kernel returns `false` there.
//! - `ProjectPagePermission` uses `Page.objects.get`, so a missing page is
//!   a 404, not a 403 ([`page::PageOutcome::PageNotFound`]).
//! - `can_mutate_states`' workspace-admin override still requires project
//!   membership (a `None` membership returns `false` first).
//! - `check_project_role`'s workspace-admin bypass needs workspace role
//!   exactly `ADMIN` (`role=ROLE.ADMIN`), not `>= ADMIN`.
//! - `InstanceAdminPermission` matches `role__gte=15` with no
//!   active/verified filter on the row.
//! - Runner visibility has a single value (`PRIVATE=0`); anything else
//!   denies by default.
//! - `request_is_desktop` treats a broken session store as "not desktop".

use pidash_types::WorkspaceId;

use crate::scope::TenantScope;

pub mod allow;
pub mod desktop;
pub mod instance;
pub mod page;
pub mod project;
pub mod runner;
pub mod workspace;

/// Workspace/project role values (`ROLE_CHOICES`: Admin=20, Member=15,
/// Guest=5, in `pi_dash/db/models/workspace.py` and `project.py`).
pub const ROLE_ADMIN: i32 = 20;
/// Workspace/project role values (`ROLE_CHOICES`: Admin=20, Member=15,
/// Guest=5, in `pi_dash/db/models/workspace.py` and `project.py`).
pub const ROLE_MEMBER: i32 = 15;
/// Workspace/project role values (`ROLE_CHOICES`: Admin=20, Member=15,
/// Guest=5, in `pi_dash/db/models/workspace.py` and `project.py`).
pub const ROLE_GUEST: i32 = 5;

/// Roles of `ROLE` (`app/permissions/base.py`, `db/models/project.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Member,
    Guest,
}

impl Role {
    /// Numeric value stored in the `role` column.
    pub fn value(self) -> i32 {
        match self {
            Role::Admin => ROLE_ADMIN,
            Role::Member => ROLE_MEMBER,
            Role::Guest => ROLE_GUEST,
        }
    }

    /// Map a stored value back; unknown values (possible only through raw
    /// SQL, since the column has choices) match no role list.
    pub fn from_value(value: i32) -> Option<Role> {
        match value {
            ROLE_ADMIN => Some(Role::Admin),
            ROLE_MEMBER => Some(Role::Member),
            ROLE_GUEST => Some(Role::Guest),
            _ => None,
        }
    }
}

/// DRF safe methods (`rest_framework.permissions.SAFE_METHODS`).
pub fn is_safe_method(method: &str) -> bool {
    matches!(method, "GET" | "HEAD" | "OPTIONS")
}

/// Deny unless `facts_workspace` is the scope's workspace.
///
/// Every Python rule filters its membership rows by workspace slug, so the
/// caller passes the slug its SQL filtered on; a mismatch means the facts
/// belong to another workspace and the check fails closed.
pub fn scope_allows(scope: &TenantScope, facts_workspace: &WorkspaceId) -> bool {
    scope.workspace_id() == facts_workspace
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_values_match_django_choices() {
        assert_eq!(Role::Admin.value(), 20);
        assert_eq!(Role::Member.value(), 15);
        assert_eq!(Role::Guest.value(), 5);
    }

    #[test]
    fn unknown_role_value_matches_nothing() {
        assert_eq!(Role::from_value(0), None);
        assert_eq!(Role::from_value(20), Some(Role::Admin));
    }

    #[test]
    fn safe_methods_match_drf() {
        for method in ["GET", "HEAD", "OPTIONS"] {
            assert!(is_safe_method(method), "{method}");
        }
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            assert!(!is_safe_method(method), "{method}");
        }
    }

    #[test]
    fn cross_workspace_facts_deny() {
        let scope = TenantScope::new(WorkspaceId::from("ws-a"));
        assert!(scope_allows(&scope, &WorkspaceId::from("ws-a")));
        assert!(!scope_allows(&scope, &WorkspaceId::from("ws-b")));
    }
}
