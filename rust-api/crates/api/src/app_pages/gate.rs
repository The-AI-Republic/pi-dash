//! Page permission gates + inline view guards (D-30, stage 5, PIDASHCONV-312).
//!
//! Ports `apps/api/pi_dash/app/permissions/page.py:18-125`
//! (`ProjectPagePermission`) and the inline guards in
//! `apps/api/pi_dash/app/views/page/base.py` that specialize it. Fixture:
//! `rust-api/fixtures/app_pages/guards/permissions.golden.json` (trace:
//! `rust-api/fixtures/app_pages/TRACE.md`).
//!
//! Shape of the port, following the [`crate::app_issues`] `Gate` precedent:
//! the permission-class decision kernel lives in the read-only F-06
//! foundation (`pidash_auth::permissions::page::{decide_page,
//! check_project_action_access}`); this module pins which gate each page
//! route carries, adds the async [`tenant_context`]/[`resolve_gate`]
//! row fetching (handlers call it — no handler takes an unscoped database
//! handle for these routes), and ports the view-body inline guards as pure
//! decide functions with byte-exact bodies.
//!
//! Gate order (preserved, not redesigned): DRF `initial()` runs session
//! authentication, then `check_permissions` (the class below), then the
//! handler body with its inline guards. Anonymous callers 401 before any
//! gate; the class denies through DRF's default body (the class defines no
//! `message` attribute); a `page_id` the workspace-scoped lookup cannot
//! find raises `DoesNotExist` inside the permission check, which
//! `BaseViewSet.handle_exception` maps to 404
//! (`app/views/base.py:133-136`) — never the view's own `"Page not found"`
//! body, which only the project-scoped queryset path returns.
//!
//! Response shapes (handlers render these; recorded here so the matrix has
//! one home):
//!
//! * Anonymous on any page route: 401 [`UNAUTHENTICATED_BODY`] — the
//!   session layer answers before any gate runs.
//! * Denied member (class half): 403 [`crate::permissions::DEFAULT_DENIED_BODY`].
//! * Permission-lookup miss (unknown/cross-workspace/soft-deleted id):
//!   404 [`OBJECT_NOT_FOUND_BODY`].
//! * Inline-guard denials: per-action `{"error": …}` bodies below (400,
//!   except destroy-owner/admin and duplicate-private which are 403).
//!
//! Ported quirks (translate, don't redesign):
//!
//! * `partial_update` catches `Page.DoesNotExist` (page or parent fetch)
//!   into the owner-access 400 body (`base.py:196-200`).
//! * `archive`/`unarchive` deny only when the requester IS a member with
//!   `role <= 15` AND is not the owner (`base.py:317-322,348-353`); a
//!   non-member falls through to the action (the class normally gates
//!   first). `destroy` instead requires `role == 20`
//!   (`base.py:382-389`) — different admin tests per action.
//! * `retrieve` evaluates the guest rule BEFORE the page-None check and
//!   dereferences `page.owned_by` there (`base.py:212-226` vs `:228`): a
//!   missing page with a restricted guest raises `AttributeError`, i.e.
//!   the generic 500 body — ported as [`RetrieveOutcome::ServerError`].
//! * `archive` answers `{"archived_at": str(datetime.now())}` from a
//!   SECOND `now()` call, which may differ from the stored value
//!   (double-now); owned by the handlers issue, noted here only.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use pidash_auth::permissions::page::{decide_page, PageFacts, PageLookup, PageOutcome, PageRef};
use pidash_auth::permissions::ROLE_GUEST;
use pidash_auth::scope::TenantScope;
use pidash_types::{ProjectId, WorkspaceId};

use crate::state::AppState;

/// Exact bytes of the DRF `IsAuthenticated` denial (401): what anonymous
/// callers on every page route receive before any gate runs.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// `handle_exception`'s `ObjectDoesNotExist` branch (404): the
/// workspace-scoped permission lookup miss — unknown, cross-workspace, or
/// soft-deleted page id (`app/views/base.py:133-136`).
pub const OBJECT_NOT_FOUND_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s generic 500 branch (`app/views/base.py:144-148`):
/// what the retrieve guest-before-None `AttributeError` renders as.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// `Page.PRIVATE_ACCESS` / `PUBLIC_ACCESS` (`db/models/page.py:24-25`).
pub const PRIVATE_ACCESS: i32 = 1;
/// See [`PRIVATE_ACCESS`].
pub const PUBLIC_ACCESS: i32 = 0;

/// Retrieve guest rule (`base.py:212-226`): restricted guests may not view
/// another member's page.
pub const RETRIEVE_GUEST_BODY: &str = r#"{"error":"You are not allowed to view this page"}"#;
/// Retrieve queryset miss (`base.py:228-229`): the page is absent from the
/// project-scoped queryset.
pub const PAGE_NOT_FOUND_BODY: &str = r#"{"error":"Page not found"}"#;
/// `partial_update` locked page (`base.py:163-164`).
pub const PAGE_LOCKED_BODY: &str = r#"{"error":"Page is locked"}"#;
/// Owner-only access change (`base.py:176-180` partial_update,
/// `:281-285` access endpoint, `:196-200` DoesNotExist branch).
pub const ACCESS_OWNER_BODY: &str =
    r#"{"error":"Access cannot be updated since this page is owned by someone else"}"#;
/// Archive owner-or-admin-15 (`base.py:317-326`).
pub const ARCHIVE_OWNER_ADMIN_BODY: &str =
    r#"{"error":"Only the owner or admin can archive the page"}"#;
/// Unarchive owner-or-admin-15 (`base.py:348-357`; "un archive" with a space).
pub const UNARCHIVE_OWNER_ADMIN_BODY: &str =
    r#"{"error":"Only the owner or admin can un archive the page"}"#;
/// Destroy must-archive (`base.py:376-380`).
pub const DESTROY_MUST_ARCHIVE_BODY: &str =
    r#"{"error":"The page should be archived before deleting"}"#;
/// Destroy owner-or-admin-20 (`base.py:382-394`).
pub const DESTROY_OWNER_ADMIN_BODY: &str = r#"{"error":"Only admin or owner can delete the page"}"#;
/// Duplicate private-owner check (`base.py:590-591`).
pub const DUPLICATE_PRIVATE_BODY: &str = r#"{"error":"Permission denied"}"#;

// ---------------------------------------------------------------------------
// Class half: Gate + tenant_context (app_issues precedent)
// ---------------------------------------------------------------------------

/// The resolved tenant for one page request: who acts, in which project,
/// with which role, and whether guest scoping applies. Handlers build
/// their membership facts only through [`resolve_gate`], never from an
/// unscoped handle.
#[derive(Debug, Clone, Copy)]
pub struct Gate {
    pub user_id: uuid::Uuid,
    pub project_id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub role: i32,
    /// `role == GUEST && !project.guest_view_all_features`: list scopes to
    /// owned rows, retrieve applies the guest rule.
    pub guest_scoped: bool,
}

/// Facts the tenant fetch returns: the request workspace plus the actor's
/// project role and the project's guest flag.
pub struct TenantFacts {
    pub workspace_id: uuid::Uuid,
    pub role: i32,
    pub guest_view_all: bool,
}

/// What a gate check denies with, before body rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    /// No session / bad session: 401 [`UNAUTHENTICATED_BODY`].
    Unauthorized,
    /// Class denial (anonymous already excluded): 403 DRF-default body.
    Forbidden,
    /// Workspace-scoped page lookup miss: 404 [`OBJECT_NOT_FOUND_BODY`].
    ObjectNotFound,
    /// Database failure: 500 [`SERVER_ERROR_BODY`].
    ServerError,
}

fn json_response(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static page-gate response")
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::Unauthorized => json_response(StatusCode::UNAUTHORIZED, UNAUTHENTICATED_BODY),
            Denial::Forbidden => json_response(
                StatusCode::FORBIDDEN,
                crate::permissions::DEFAULT_DENIED_BODY,
            ),
            Denial::ObjectNotFound => json_response(StatusCode::NOT_FOUND, OBJECT_NOT_FOUND_BODY),
            Denial::ServerError => {
                json_response(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
            }
        }
    }
}

/// Session auth + membership hook + (when the URL carries `page_id`) the
/// workspace-scoped page fetch + role matrix, in Django's order
/// (`page.py:28-53`): anonymous is rejected before anything else; the
/// membership hook denies non-members before the page fetch, so an unknown
/// id for a non-member answers 403, never 404; owners bypass with any
/// method and any role; private non-owned pages deny; public pages use the
/// role matrix.
pub async fn resolve_gate(
    state: &AppState,
    method: &str,
    slug: &str,
    project_id: &uuid::Uuid,
    page_id: Option<uuid::Uuid>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Gate, Denial> {
    let pool = state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let tenant = tenant_context(pool, slug, project_id, &user_id).await?;
    let scope = TenantScope::new(WorkspaceId::from(slug.to_owned()));
    let page = match page_id {
        None => PageLookup::Absent,
        Some(id) => match fetch_page(pool, slug, &id, &user_id).await? {
            Some(page) => PageLookup::Present(page),
            None => PageLookup::Missing,
        },
    };
    let facts = PageFacts {
        workspace: WorkspaceId::from(slug.to_owned()),
        project_id: ProjectId::from(project_id.to_string()),
        authenticated: true,
        project_role: Some(tenant.role),
        page,
    };
    match decide_page(method, &scope, &facts) {
        PageOutcome::Allow => Ok(Gate {
            user_id,
            project_id: *project_id,
            workspace_id: tenant.workspace_id,
            role: tenant.role,
            guest_scoped: tenant.role == ROLE_GUEST && !tenant.guest_view_all,
        }),
        PageOutcome::Deny => Err(Denial::Forbidden),
        PageOutcome::PageNotFound => Err(Denial::ObjectNotFound),
    }
}

/// View-body tenant facts: the actor's active project role plus the
/// project's guest flag and workspace id. Denies (403) when the actor
/// holds no active project-membership row (`_check_access_and_get_role`,
/// `page.py:70-78`); a missing project row with a membership row present
/// cannot happen (FK) and fails closed the same way.
pub async fn tenant_context(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<TenantFacts, Denial> {
    // `role` is a `PositiveSmallIntegerField` (SMALLINT): decode as `i16`.
    let role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let role = role.map(|row| i32::from(row.0)).ok_or(Denial::Forbidden)?;
    let project: Option<(uuid::Uuid, bool)> = sqlx::query_as(
        r#"SELECT p.workspace_id, p.guest_view_all_features FROM projects p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let (workspace_id, guest_view_all) = project.ok_or(Denial::Forbidden)?;
    Ok(TenantFacts {
        workspace_id,
        role,
        guest_view_all,
    })
}

/// `Page.objects.get(id=page_id, workspace__slug=slug)` (`page.py:42`):
/// workspace-scoped only — NOT project-scoped. Soft-deleted rows do not
/// count (the default manager). Missing rows are `None` here; the kernel
/// maps them to the 404.
async fn fetch_page(
    pool: &sqlx::PgPool,
    slug: &str,
    page_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Option<PageRef>, Denial> {
    // `access` is a `PositiveSmallIntegerField` (SMALLINT): decode as `i16`.
    let row: Option<(uuid::Uuid, i16)> = sqlx::query_as(
        r#"SELECT p.owned_by_id, p.access FROM pages p
           JOIN workspaces w ON w.id = p.workspace_id
           WHERE p.id = $1 AND w.slug = $2 AND p.deleted_at IS NULL"#,
    )
    .bind(page_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|(owned_by_id, access)| PageRef {
        owned_by_requester: owned_by_id == *user_id,
        is_private: access == PRIVATE_ACCESS as i16,
    }))
}

/// `request.user` from the Django session (`_auth_user_id`). No session,
/// no key, or a non-UUID id means anonymous → 401. (Django PKs are UUIDs;
/// a session id that is not a UUID cannot be a user.)
fn actor_user_id(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Option<uuid::Uuid> {
    let handle = extension?.0;
    let mut session = handle.snapshot();
    let raw = session.get("_auth_user_id")?.as_str()?.to_owned();
    raw.parse::<uuid::Uuid>().ok()
}

// ---------------------------------------------------------------------------
// Inline guards (view-body half, pure decide functions)
// ---------------------------------------------------------------------------

/// `retrieve` guest rule (`base.py:212-229`).
///
/// The guest check runs BEFORE the page-None check and dereferences the
/// page row, so a missing page with a restricted guest is
/// [`RetrieveOutcome::ServerError`] (the `AttributeError` → generic 500),
/// not the 404.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrieveOutcome {
    Allow,
    /// 400 [`RETRIEVE_GUEST_BODY`].
    GuestDenied,
    /// 404 [`PAGE_NOT_FOUND_BODY`] (project-scoped queryset miss).
    NotFound,
    /// 500 [`SERVER_ERROR_BODY`] (guest rule on a missing page).
    ServerError,
}

/// `owned_by_requester`: `Some` when the project-scoped queryset found the
/// page, `None` when it did not. Handlers pass [`Gate::guest_scoped`] for
/// `guest_scoped_member` (the `role == 5` exists-check over the same active
/// membership row, plus the project's `guest_view_all_features` flag).
pub fn check_retrieve_guest(
    guest_scoped_member: bool,
    page_owner_is_requester: Option<bool>,
) -> RetrieveOutcome {
    if guest_scoped_member {
        match page_owner_is_requester {
            Some(true) => RetrieveOutcome::Allow,
            Some(false) => RetrieveOutcome::GuestDenied,
            None => RetrieveOutcome::ServerError,
        }
    } else {
        match page_owner_is_requester {
            Some(_) => RetrieveOutcome::Allow,
            None => RetrieveOutcome::NotFound,
        }
    }
}

/// `list` guest scoping (`base.py:294-304`): restricted guests see only
/// their own rows (`queryset.filter(owned_by=request.user)`).
pub fn list_guest_scoped(role: i32, guest_view_all: bool) -> bool {
    role == ROLE_GUEST && !guest_view_all
}

/// `archive` owner-or-admin-15 (`base.py:317-326`).
///
/// Denies (400) only when the requester IS an active member with
/// `role <= 15` AND is not the owner — a non-member falls through to the
/// action (ported as observed; the class normally gates first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateOpOutcome {
    Allow,
    Deny,
}

pub fn check_archive(requester_is_owner: bool, member_lte15_exists: bool) -> StateOpOutcome {
    if member_lte15_exists && !requester_is_owner {
        StateOpOutcome::Deny
    } else {
        StateOpOutcome::Allow
    }
}

/// `unarchive` owner-or-admin-15 (`base.py:348-357`): same shape as
/// [`check_archive`], different body ([`UNARCHIVE_OWNER_ADMIN_BODY`]).
pub fn check_unarchive(requester_is_owner: bool, member_lte15_exists: bool) -> StateOpOutcome {
    check_archive(requester_is_owner, member_lte15_exists)
}

/// `destroy` (`base.py:376-394`): must-archive first, then
/// owner-or-admin-20.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestroyOutcome {
    Allow,
    /// 400 [`DESTROY_MUST_ARCHIVE_BODY`].
    MustArchive,
    /// 403 [`DESTROY_OWNER_ADMIN_BODY`].
    Deny,
}

pub fn check_destroy(
    archived: bool,
    requester_is_owner: bool,
    admin20_exists: bool,
) -> DestroyOutcome {
    if !archived {
        return DestroyOutcome::MustArchive;
    }
    if !requester_is_owner && !admin20_exists {
        return DestroyOutcome::Deny;
    }
    DestroyOutcome::Allow
}

/// Owner-only access change (`base.py:176-180`, `:281-285`): denies (400)
/// when the effective access differs from the stored one and the
/// requester is not the owner. An absent `access` key defaults to the
/// stored value, so it never denies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessOutcome {
    Allow,
    /// 400 [`ACCESS_OWNER_BODY`].
    Deny,
}

pub fn check_access(
    page_access: i32,
    requested_access: Option<i32>,
    requester_is_owner: bool,
) -> AccessOutcome {
    let effective = requested_access.unwrap_or(page_access);
    if effective != page_access && !requester_is_owner {
        AccessOutcome::Deny
    } else {
        AccessOutcome::Allow
    }
}

/// `partial_update` (`base.py:154-200`): lock first, then the scoped
/// parent re-fetch, then the owner-access rule. A missing page or parent
/// row denies with the SAME owner-access 400 body (`:196-200`, ported as
/// observed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialUpdateOutcome {
    Allow,
    /// 400 [`PAGE_LOCKED_BODY`].
    Locked,
    /// 400 [`ACCESS_OWNER_BODY`] (access change by non-owner, or a
    /// missing page/parent row — same body either way).
    AccessDenied,
}

pub fn check_partial_update(
    is_locked: bool,
    page_found: bool,
    parent_found: Option<bool>,
    page_access: i32,
    requested_access: Option<i32>,
    requester_is_owner: bool,
) -> PartialUpdateOutcome {
    if is_locked {
        return PartialUpdateOutcome::Locked;
    }
    if !page_found || parent_found == Some(false) {
        return PartialUpdateOutcome::AccessDenied;
    }
    match check_access(page_access, requested_access, requester_is_owner) {
        AccessOutcome::Allow => PartialUpdateOutcome::Allow,
        AccessOutcome::Deny => PartialUpdateOutcome::AccessDenied,
    }
}

/// `duplicate` private-owner check (`base.py:590-591`): denies (403
/// [`DUPLICATE_PRIVATE_BODY`]) when the source page is private and the
/// requester is not its owner.
pub fn check_duplicate_private(page_access: i32, requester_is_owner: bool) -> bool {
    page_access == PRIVATE_ACCESS && !requester_is_owner
}

/// Description `Q(owned_by=user) | Q(access=0)` row scoping
/// (`base.py:502-508` retrieve, `:522-528` partial_update): the queryset
/// predicate as a row decision. Handlers compose it as
/// `WHERE (owned_by_id = $user OR access = 0)` over the
/// workspace+project+not-deleted fetch.
pub fn description_visible_to_requester(requester_is_owner: bool, page_access: i32) -> bool {
    requester_is_owner || page_access == PUBLIC_ACCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn facts(role: Option<i32>, page: PageLookup, authenticated: bool) -> PageFacts {
        PageFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated,
            project_role: role,
            page,
        }
    }

    fn page(owner: bool, private: bool) -> PageLookup {
        PageLookup::Present(PageRef {
            owned_by_requester: owner,
            is_private: private,
        })
    }

    fn decide(method: &str, role: Option<i32>, page: PageLookup) -> PageOutcome {
        decide_page(method, &scope(), &facts(role, page, true))
    }

    /// F30-09 `permission_matrix.anonymous` + `non_member`
    /// (`page.py:28-29,70-78`).
    #[test]
    fn anonymous_and_non_member_deny_before_page_fetch() {
        let anon = facts(Some(ROLE_ADMIN), PageLookup::Absent, false);
        assert_eq!(decide_page("GET", &scope(), &anon), PageOutcome::Deny);
        assert_eq!(
            decide("GET", None, PageLookup::Absent),
            PageOutcome::Deny,
            "no project-member row -> (False, None) -> deny"
        );
        // Membership is checked before the page fetch: a non-member with a
        // missing page still denies (never the 404).
        assert_eq!(decide("GET", None, PageLookup::Missing), PageOutcome::Deny);
    }

    /// F30-09 `permission_matrix.owner_bypass` (`page.py:44-46`).
    #[test]
    fn owner_bypasses_with_any_method_and_role() {
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE", "TRACE"] {
            assert_eq!(
                decide(method, Some(ROLE_GUEST), page(true, true)),
                PageOutcome::Allow,
                "owner bypass {method}"
            );
        }
    }

    /// F30-09 `permission_matrix.private_page_non_owner` (`page.py:48-50`
    /// + `:80-85`: base `_has_private_page_action_access` is `False`).
    #[test]
    fn private_non_owner_denies_even_admin() {
        assert_eq!(
            decide("GET", Some(ROLE_ADMIN), page(false, true)),
            PageOutcome::Deny
        );
    }

    /// F30-09 `permission_matrix.public_page_role_matrix` (`page.py:87-125`).
    #[test]
    fn public_page_role_matrix() {
        // `PageLookup` is not `Copy`, so each decision builds its row fresh.
        let public = || page(false, false);
        // POST admin/member only.
        assert_eq!(
            decide("POST", Some(ROLE_ADMIN), public()),
            PageOutcome::Allow
        );
        assert_eq!(
            decide("POST", Some(ROLE_MEMBER), public()),
            PageOutcome::Allow
        );
        assert_eq!(
            decide("POST", Some(ROLE_GUEST), public()),
            PageOutcome::Deny
        );
        // Safe methods: all roles.
        for method in ["GET", "HEAD", "OPTIONS"] {
            for role in [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST] {
                assert_eq!(
                    decide(method, Some(role), public()),
                    PageOutcome::Allow,
                    "{method} role {role}"
                );
            }
        }
        // PUT/PATCH admin/member only.
        for method in ["PUT", "PATCH"] {
            assert_eq!(
                decide(method, Some(ROLE_ADMIN), public()),
                PageOutcome::Allow
            );
            assert_eq!(
                decide(method, Some(ROLE_MEMBER), public()),
                PageOutcome::Allow
            );
            assert_eq!(
                decide(method, Some(ROLE_GUEST), public()),
                PageOutcome::Deny
            );
        }
        // DELETE admin only.
        assert_eq!(
            decide("DELETE", Some(ROLE_ADMIN), public()),
            PageOutcome::Allow
        );
        assert_eq!(
            decide("DELETE", Some(ROLE_MEMBER), public()),
            PageOutcome::Deny
        );
        assert_eq!(
            decide("DELETE", Some(ROLE_GUEST), public()),
            PageOutcome::Deny
        );
        // Default deny.
        assert_eq!(
            decide("TRACE", Some(ROLE_ADMIN), public()),
            PageOutcome::Deny
        );
        // No page_id on the URL goes straight to the matrix.
        assert_eq!(
            decide("GET", Some(ROLE_GUEST), PageLookup::Absent),
            PageOutcome::Allow
        );
        assert_eq!(
            decide("POST", Some(ROLE_GUEST), PageLookup::Absent),
            PageOutcome::Deny
        );
    }

    /// F30-09: missing page is the 404 branch for members
    /// (`page.py:42` lookup miss).
    #[test]
    fn missing_page_is_not_found_for_members() {
        assert_eq!(
            decide("GET", Some(ROLE_ADMIN), PageLookup::Missing),
            PageOutcome::PageNotFound
        );
    }

    #[test]
    fn cross_workspace_and_role_values() {
        // Roles: ADMIN=20, MEMBER=15, GUEST=5 (`app/permissions/base.py:13-17`).
        assert_eq!(ROLE_ADMIN, 20);
        assert_eq!(ROLE_MEMBER, 15);
        assert_eq!(ROLE_GUEST, 5);
        let other = TenantScope::new(WorkspaceId::from("other"));
        let member = facts(Some(ROLE_ADMIN), PageLookup::Absent, true);
        assert_eq!(decide_page("GET", &other, &member), PageOutcome::Deny);
    }

    /// F30-09 `inline_guards`: retrieve guest rule (`base.py:212-229`).
    #[test]
    fn retrieve_guest_rule() {
        // Restricted guest on another member's page: 400.
        assert_eq!(
            check_retrieve_guest(true, Some(false)),
            RetrieveOutcome::GuestDenied
        );
        // Owner guest: allowed.
        assert_eq!(
            check_retrieve_guest(true, Some(true)),
            RetrieveOutcome::Allow
        );
        // Non-guest (or flag on): queryset miss is the 404.
        assert_eq!(check_retrieve_guest(false, None), RetrieveOutcome::NotFound);
        assert_eq!(
            check_retrieve_guest(false, Some(false)),
            RetrieveOutcome::Allow
        );
        // Missing page WITH a restricted guest: the AttributeError port
        // (guest check dereferences the row before the None check).
        assert_eq!(
            check_retrieve_guest(true, None),
            RetrieveOutcome::ServerError
        );
    }

    /// F30-09 `inline_guards`: list guest scoping (`base.py:294-304`).
    #[test]
    fn list_guest_scoping() {
        assert!(list_guest_scoped(ROLE_GUEST, false));
        assert!(!list_guest_scoped(ROLE_GUEST, true));
        assert!(!list_guest_scoped(ROLE_MEMBER, false));
        assert!(!list_guest_scoped(ROLE_ADMIN, false));
    }

    /// F30-09: archive/unarchive owner-or-admin-15 (`base.py:317-326`,
    /// `:348-357`). `member_lte15_exists` is the `role__lte=15` filter
    /// outcome: admins (20) never match it, members/guests do.
    #[test]
    fn archive_unarchive_owner_or_admin() {
        // Owner always passes.
        assert_eq!(check_archive(true, true), StateOpOutcome::Allow);
        assert_eq!(check_archive(true, false), StateOpOutcome::Allow);
        // Member/guest non-owner denies...
        assert_eq!(check_archive(false, true), StateOpOutcome::Deny);
        // ...while an admin (no role<=15 row) or a non-member (no row at
        // all) falls through to the action — ported as observed.
        assert_eq!(check_archive(false, false), StateOpOutcome::Allow);
        assert_eq!(check_unarchive(false, true), StateOpOutcome::Deny);
        assert_eq!(check_unarchive(true, true), StateOpOutcome::Allow);
        assert_eq!(check_unarchive(false, false), StateOpOutcome::Allow);
    }

    /// F30-09: destroy must-archive + owner-or-admin-20 (`base.py:376-394`).
    #[test]
    fn destroy_must_archive_then_owner_or_admin20() {
        // Must-archive fires first, even for the owner.
        assert_eq!(
            check_destroy(false, true, true),
            DestroyOutcome::MustArchive
        );
        assert_eq!(
            check_destroy(false, false, false),
            DestroyOutcome::MustArchive
        );
        // Archived: owner or a role==20 member passes; anyone else 403s.
        assert_eq!(check_destroy(true, true, false), DestroyOutcome::Allow);
        assert_eq!(check_destroy(true, false, true), DestroyOutcome::Allow);
        assert_eq!(check_destroy(true, false, false), DestroyOutcome::Deny);
    }

    /// F30-09: access owner-only (`base.py:176-180`, `:281-285`).
    #[test]
    fn access_change_is_owner_only() {
        assert_eq!(
            check_access(PUBLIC_ACCESS, Some(PRIVATE_ACCESS), false),
            AccessOutcome::Deny
        );
        assert_eq!(
            check_access(PUBLIC_ACCESS, Some(PRIVATE_ACCESS), true),
            AccessOutcome::Allow
        );
        // Same value, or an absent key (defaults to stored): no deny.
        assert_eq!(
            check_access(PUBLIC_ACCESS, Some(PUBLIC_ACCESS), false),
            AccessOutcome::Allow
        );
        assert_eq!(
            check_access(PUBLIC_ACCESS, None, false),
            AccessOutcome::Allow
        );
    }

    /// F30-09: partial_update lock/parent/access (`base.py:154-200`).
    #[test]
    fn partial_update_lock_parent_access() {
        // Locked fires first.
        assert_eq!(
            check_partial_update(true, true, None, PUBLIC_ACCESS, None, true),
            PartialUpdateOutcome::Locked
        );
        // Missing page or parent rows share the owner-access body.
        assert_eq!(
            check_partial_update(false, false, None, PUBLIC_ACCESS, None, true),
            PartialUpdateOutcome::AccessDenied
        );
        assert_eq!(
            check_partial_update(false, true, Some(false), PUBLIC_ACCESS, None, true),
            PartialUpdateOutcome::AccessDenied
        );
        // Access change by non-owner.
        assert_eq!(
            check_partial_update(
                false,
                true,
                None,
                PUBLIC_ACCESS,
                Some(PRIVATE_ACCESS),
                false
            ),
            PartialUpdateOutcome::AccessDenied
        );
        // Happy paths.
        assert_eq!(
            check_partial_update(false, true, None, PUBLIC_ACCESS, None, false),
            PartialUpdateOutcome::Allow
        );
        assert_eq!(
            check_partial_update(
                false,
                true,
                Some(true),
                PUBLIC_ACCESS,
                Some(PRIVATE_ACCESS),
                true
            ),
            PartialUpdateOutcome::Allow
        );
    }

    /// Issue inline guards: duplicate private-owner 403 (`base.py:590-591`).
    #[test]
    fn duplicate_private_owner_only() {
        assert!(check_duplicate_private(PRIVATE_ACCESS, false));
        assert!(!check_duplicate_private(PRIVATE_ACCESS, true));
        assert!(!check_duplicate_private(PUBLIC_ACCESS, false));
    }

    /// Issue inline guards: description `Q(owner-or-public)` scoping
    /// (`base.py:502-508`, `:522-528`).
    #[test]
    fn description_owner_or_public_scoping() {
        assert!(description_visible_to_requester(true, PRIVATE_ACCESS));
        assert!(description_visible_to_requester(false, PUBLIC_ACCESS));
        assert!(!description_visible_to_requester(false, PRIVATE_ACCESS));
    }

    /// Every inline error body/status byte, as the contract suites pin them.
    #[test]
    fn denial_bodies_match_contract_goldens() {
        assert_eq!(
            RETRIEVE_GUEST_BODY,
            r#"{"error":"You are not allowed to view this page"}"#
        );
        assert_eq!(PAGE_NOT_FOUND_BODY, r#"{"error":"Page not found"}"#);
        assert_eq!(PAGE_LOCKED_BODY, r#"{"error":"Page is locked"}"#);
        assert_eq!(
            ACCESS_OWNER_BODY,
            r#"{"error":"Access cannot be updated since this page is owned by someone else"}"#
        );
        assert_eq!(
            ARCHIVE_OWNER_ADMIN_BODY,
            r#"{"error":"Only the owner or admin can archive the page"}"#
        );
        assert_eq!(
            UNARCHIVE_OWNER_ADMIN_BODY,
            r#"{"error":"Only the owner or admin can un archive the page"}"#
        );
        assert_eq!(
            DESTROY_MUST_ARCHIVE_BODY,
            r#"{"error":"The page should be archived before deleting"}"#
        );
        assert_eq!(
            DESTROY_OWNER_ADMIN_BODY,
            r#"{"error":"Only admin or owner can delete the page"}"#
        );
        assert_eq!(DUPLICATE_PRIVATE_BODY, r#"{"error":"Permission denied"}"#);
        assert_eq!(
            OBJECT_NOT_FOUND_BODY,
            r#"{"error":"The required object does not exist."}"#
        );
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
        assert_eq!(PRIVATE_ACCESS, 1);
        assert_eq!(PUBLIC_ACCESS, 0);
    }
}
