#![forbid(unsafe_code)]

//! Page permissions (`app/permissions/page.py`, duplicated in
//! `utils/permissions/page.py`).
//!
//! `ProjectPagePermission.has_permission` runs before any object fetch: it
//! first requires project membership (the `_check_access_and_get_role`
//! hook), then — when the URL carries a `page_id` — fetches the page with
//! `Page.objects.get(id=page_id, workspace__slug=slug)`. A missing page
//! raises through to a 404, not a 403 ([`PageOutcome::PageNotFound`]).
//! Owners always pass; private pages otherwise deny in the base
//! implementation (`_has_private_page_action_access` returns `False` — the
//! cloud overlay overrides it, an F-10 seam); public pages use the role
//! matrix below.

use pidash_types::{ProjectId, WorkspaceId};

use super::{scope_allows, ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
use crate::scope::TenantScope;

/// A fetched page row: ownership and visibility only. `access` is
/// `Page.PRIVATE_ACCESS=1` / `PUBLIC_ACCESS=0` (`db/models/page.py`).
pub struct PageRef {
    pub owned_by_requester: bool,
    pub is_private: bool,
}

/// The page lookup result for a request carrying `page_id`.
///
/// `Absent` means the URL has no `page_id` (list/create views): the check
/// goes straight to the role matrix.
pub enum PageLookup {
    Absent,
    Present(PageRef),
    Missing,
}

/// Facts for one `(user, slug, project_id[, page_id])` request.
pub struct PageFacts {
    pub workspace: WorkspaceId,
    pub project_id: ProjectId,
    pub authenticated: bool,
    /// Active project membership role, if any (the hook denies when empty).
    pub project_role: Option<i32>,
    pub page: PageLookup,
}

/// Decision including the 404 case (DRF raises `Page.DoesNotExist`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOutcome {
    Allow,
    Deny,
    PageNotFound,
}

/// Mirror of `has_permission`.
pub fn decide_page(method: &str, scope: &TenantScope, facts: &PageFacts) -> PageOutcome {
    if !scope_allows(scope, &facts.workspace) || !facts.authenticated {
        return PageOutcome::Deny;
    }
    let role = match facts.project_role {
        None => return PageOutcome::Deny,
        Some(role) => role,
    };
    if let PageLookup::Present(page) = &facts.page {
        if page.owned_by_requester {
            return PageOutcome::Allow;
        }
        if page.is_private {
            // Base `_has_private_page_action_access`: only the owner.
            return PageOutcome::Deny;
        }
    }
    if let PageLookup::Missing = facts.page {
        return PageOutcome::PageNotFound;
    }
    if check_project_action_access(method, role) {
        PageOutcome::Allow
    } else {
        PageOutcome::Deny
    }
}

/// `_check_project_action_access`: POST needs Admin/Member; safe methods
/// allow Admin/Member/Guest; PUT/PATCH need Admin/Member; DELETE needs
/// Admin; anything else denies.
pub fn check_project_action_access(method: &str, role: i32) -> bool {
    if method == "POST" {
        return role == ROLE_ADMIN || role == ROLE_MEMBER;
    }
    if super::is_safe_method(method) {
        return role == ROLE_ADMIN || role == ROLE_MEMBER || role == ROLE_GUEST;
    }
    if method == "PUT" || method == "PATCH" {
        return role == ROLE_ADMIN || role == ROLE_MEMBER;
    }
    if method == "DELETE" {
        return role == ROLE_ADMIN;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn facts(role: Option<i32>) -> PageFacts {
        PageFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated: true,
            project_role: role,
            page: PageLookup::Absent,
        }
    }

    fn page(owner: bool, private: bool) -> PageLookup {
        PageLookup::Present(PageRef {
            owned_by_requester: owner,
            is_private: private,
        })
    }

    #[test]
    fn non_member_denies_before_page_fetch() {
        assert_eq!(
            decide_page("GET", &scope(), &facts(None)),
            PageOutcome::Deny
        );
    }

    #[test]
    fn missing_page_is_not_found() {
        let mut f = facts(Some(ROLE_ADMIN));
        f.page = PageLookup::Missing;
        assert_eq!(decide_page("GET", &scope(), &f), PageOutcome::PageNotFound);
        // ...even for a non-member? No: membership is checked first.
        let mut anon = facts(None);
        anon.page = PageLookup::Missing;
        assert_eq!(decide_page("GET", &scope(), &anon), PageOutcome::Deny);
    }

    #[test]
    fn owner_passes_private_pages() {
        let mut f = facts(Some(ROLE_GUEST));
        f.page = page(true, true);
        assert_eq!(decide_page("DELETE", &scope(), &f), PageOutcome::Allow);
    }

    #[test]
    fn private_non_owner_denies() {
        let mut f = facts(Some(ROLE_ADMIN));
        f.page = page(false, true);
        assert_eq!(decide_page("GET", &scope(), &f), PageOutcome::Deny);
    }

    #[test]
    fn role_matrix_for_public_pages() {
        // Guest reads but cannot create, update, or delete.
        assert!(check_project_action_access("GET", ROLE_GUEST));
        assert!(!check_project_action_access("POST", ROLE_GUEST));
        assert!(!check_project_action_access("PATCH", ROLE_GUEST));
        assert!(!check_project_action_access("DELETE", ROLE_GUEST));
        // Member everything but delete.
        assert!(check_project_action_access("POST", ROLE_MEMBER));
        assert!(check_project_action_access("PUT", ROLE_MEMBER));
        assert!(!check_project_action_access("DELETE", ROLE_MEMBER));
        // Admin everything; unknown methods deny.
        assert!(check_project_action_access("DELETE", ROLE_ADMIN));
        assert!(!check_project_action_access("TRACE", ROLE_ADMIN));
    }

    #[test]
    fn public_page_uses_matrix() {
        let mut f = facts(Some(ROLE_GUEST));
        f.page = page(false, false);
        assert_eq!(decide_page("GET", &scope(), &f), PageOutcome::Allow);
        assert_eq!(decide_page("DELETE", &scope(), &f), PageOutcome::Deny);
    }

    #[test]
    fn cross_workspace_and_anonymous_deny() {
        let other = TenantScope::new(WorkspaceId::from("other"));
        assert_eq!(
            decide_page("GET", &other, &facts(Some(ROLE_ADMIN))),
            PageOutcome::Deny
        );
        let mut f = facts(Some(ROLE_ADMIN));
        f.authenticated = false;
        assert_eq!(decide_page("GET", &scope(), &f), PageOutcome::Deny);
    }
}
