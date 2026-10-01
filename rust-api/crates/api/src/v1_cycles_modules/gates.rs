//! D-20 permission gates + error envelopes (stage 5, PIDASHCONV-309).
//!
//! Ports `ProjectEntityPermission` (`app/permissions/project.py:85-116`) as
//! applied by the 6 cycle + 5 module v1 view classes, and the shared
//! `BaseAPIView.handle_exception` envelopes (`api/views/base.py:133-170`).
//! Fixture id: FX-CYCMOD-06
//! (`rust-api/fixtures/v1_cycles_modules/guards/permissions.golden.json` +
//! `TRACE.md`).
//!
//! Shape of the port: the module is pure, like the F-06 kernel it sits on.
//! Membership facts are caller inputs
//! ([`pidash_auth::permissions::project::ProjectFacts`]); row fetching (the
//! `ProjectMember ...exists()` queries with their `workspace__slug=` /
//! `project_id=` / `is_active=True` filters) stays with the handlers, which
//! fetch through the workspace-scoped handle — the `tenant_context` half of
//! the pilot pattern. This module only maps each route to its [`Gate`],
//! decides through the kernel, and pins the byte-exact denial bodies, so
//! the tenant scope (`workspace__slug` + `project_id` on every path) comes
//! along unchanged.
//!
//! Gate order (preserved, not redesigned): DRF `initial()` runs API-key
//! authentication (`api/views/base.py:101`), then the slug→UUID rewrite
//! (`base.py:52-104`, skipped for anonymous callers so slugs cannot be
//! probed via 404-vs-401), then `check_permissions`, then the handler body.
//! Anonymous callers therefore 401 on every D-20 route and never reach a
//! gate; unregistered methods 405 only after auth passes (notably GET on
//! the module-issue-detail route, which only registers DELETE —
//! `api/urls/module.py:31-35` — while the cycle twin registers GET+DELETE).
//! Handlers must resolve auth before method dispatch: axum answers 405 for
//! unregistered methods by default, which would mistime the 401/405 order
//! Django produces.
//!
//! Denial bodies (byte-exact, compact DRF rendering):
//!
//! - Anonymous on any D-20 route: 401 [`UNAUTHENTICATED_BODY`] — the class
//!   returns `False` for anonymous callers, which DRF renders as
//!   `NotAuthenticated` (no successful authenticator) rather than
//!   `PermissionDenied`. An invalid API token answers 403
//!   `{"detail":"Given API token is not valid"}` from the auth layer
//!   (`api/middleware/api_authentication.py`), outside this port.
//! - Denied member: 403 [`CLASS_DENIAL_BODY`] — the DRF-default
//!   `PermissionDenied` body. The class sets no `message`, so every denial
//!   renders through `APIView.permission_denied` with `message=None`.
//! - Missing row: 404 [`NOT_FOUND_BODY`] — the `BaseAPIView`
//!   `ObjectDoesNotExist` branch (`base.py:154-158`). This is the
//!   "requested resource" spelling, NOT the `BaseViewSet` "required
//!   object" variant the app-plane ports pin.
//!
//! Verified absent, not ported:
//!
//! - Guest `created_by` scoping: `grep guest` over `api/views/{cycle,module}.py`
//!   is empty. Guests read through plain project membership and 403 on
//!   writes; no per-row scoping exists on these routes.
//! - `project_identifier`: none of the 11 classes defines the attribute, so
//!   the kernel's identifier branch (`project.py:91-98`) is dead for D-20.
//!   It stays ported in the kernel and pinned by test, with
//!   `has_project_identifier: false` in every D-20 fact set.
//! - Throttles: neither view file declares `throttle_classes`,
//!   `throttle_scope`, or any `throttle` reference — rate limiting comes
//!   only from the shared `BaseAPIView.get_throttles`
//!   (`ApiKeyRateThrottle` / `ServiceTokenRateThrottle`,
//!   `api/views/base.py:118-131`), cross-cutting infrastructure outside
//!   this guard port.
//! - View-inline delete 403s (`{"error":"Only admin or creator can delete
//!   the cycle|module"}`, `views/cycle.py:578-590`,
//!   `views/module.py:497-509`) live in the handler bodies and are recorded
//!   in the handlers fixture too — the handler issues (PIDASHCONV-362/406)
//!   own them, not this gate port.
//!
//! Ported quirks (translate, don't redesign):
//!
//! - BUG-PORT `dispatch` returns `exc`, not the mapped response
//!   (`api/views/base.py:182`). Unreachable in practice: `handle_exception`
//!   is total (every branch returns a `Response`, never re-raises), so the
//!   outer `except` only catches failures outside the handler call itself
//!   (e.g. inside `finalize_response`). There is no Rust equivalent to
//!   port — handlers return `Response`s, never exception objects — so the
//!   envelopes below mirror `handle_exception`'s branch order exactly and
//!   this bug is recorded here and in the PR only.
//! - Module-issue-detail GET is defined on the view (`views/module.py:800`)
//!   but unrouted (`api/urls/module.py:31-35` registers DELETE only), so it
//!   405s after auth while the cycle twin serves GET. The table carries no
//!   GET row for that path; the asymmetry is pinned by test.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use pidash_auth::permissions::project;
use pidash_auth::scope::TenantScope;
use pidash_types::WorkspaceId;

/// Exact bytes of the anonymous denial on every D-20 route: the class
/// returns `False` for anonymous callers, which DRF renders as
/// `NotAuthenticated` (401), not `PermissionDenied`.
pub const UNAUTHENTICATED_BODY: &str =
    r#"{"detail":"Authentication credentials were not provided."}"#;
/// Exact bytes of the DRF-default permission-denied body every D-20 class
/// denial renders (the class sets no `message`). Alias of
/// [`crate::permissions::DEFAULT_DENIED_BODY`] so handlers have one home.
pub const CLASS_DENIAL_BODY: &str = crate::permissions::DEFAULT_DENIED_BODY;
/// `handle_exception`'s `ObjectDoesNotExist` branch (`base.py:154-158`):
/// the `BaseAPIView` "requested resource" spelling.
pub const NOT_FOUND_BODY: &str = r#"{"error":"The requested resource does not exist."}"#;
/// `handle_exception`'s `IntegrityError` branch (`base.py:142-146`).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s `ValidationError` branch (`base.py:148-152`).
pub const INVALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s `KeyError` branch (`base.py:160-164`).
pub const MISSING_KEY_BODY: &str = r#"{"error":"The required key does not exist."}"#;
/// `handle_exception`'s generic 500 branch (`base.py:167-170`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Which permission class guards a D-20 route+method. All 11 view classes
/// declare `permission_classes = [ProjectEntityPermission]` with no
/// `get_permissions` override, so the table carries one gate — every row
/// below is this variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// `ProjectEntityPermission` (`app/permissions/project.py:85-116`):
    /// safe methods need any active project membership; writes need an
    /// active project ADMIN/MEMBER row.
    ProjectEntity,
}

/// One row of the FX-CYCMOD-06 matrix: a route+method and its gate.
pub struct RouteGate {
    pub method: &'static str,
    pub path: &'static str,
    pub gate: Gate,
    /// Python source of the gate for this row.
    pub source: &'static str,
}

/// All 15 D-20 routes, one entry per registered method, in `api/urls/`
/// order (cycle patterns `api/urls/cycle.py:16-57`, then module patterns
/// `api/urls/module.py:15-51`).
pub static GATES: &[RouteGate] = &[
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:80 (CycleListCreateAPIEndpoint)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:80 (CycleListCreateAPIEndpoint)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:359 (CycleDetailAPIEndpoint)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:359 (CycleDetailAPIEndpoint)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:359 (CycleDetailAPIEndpoint)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:806 (CycleIssueListCreateAPIEndpoint)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:806 (CycleIssueListCreateAPIEndpoint)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:1013 (CycleIssueDetailAPIEndpoint)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:1013 (CycleIssueDetailAPIEndpoint)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/transfer-issues/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:1117 (TransferCycleIssueAPIEndpoint)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/cycles/<uuid>/archive/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:616 (CycleArchiveUnarchiveAPIEndpoint)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/archived-cycles/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:616 (CycleArchiveUnarchiveAPIEndpoint; list branch)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/archived-cycles/<uuid>/unarchive/",
        gate: Gate::ProjectEntity,
        source: "cycle.py:616 (CycleArchiveUnarchiveAPIEndpoint; unarchive branch)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/modules/",
        gate: Gate::ProjectEntity,
        source: "module.py:76 (ModuleListCreateAPIEndpoint)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/modules/",
        gate: Gate::ProjectEntity,
        source: "module.py:76 (ModuleListCreateAPIEndpoint)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/modules/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "module.py:279 (ModuleDetailAPIEndpoint)",
    },
    RouteGate {
        method: "PATCH",
        path: "workspaces/<slug>/projects/<id>/modules/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "module.py:279 (ModuleDetailAPIEndpoint)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/modules/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "module.py:279 (ModuleDetailAPIEndpoint)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/modules/<uuid>/module-issues/",
        gate: Gate::ProjectEntity,
        source: "module.py:536 (ModuleIssueListCreateAPIEndpoint)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/modules/<uuid>/module-issues/",
        gate: Gate::ProjectEntity,
        source: "module.py:536 (ModuleIssueListCreateAPIEndpoint)",
    },
    // No GET row: the view defines `get` (module.py:800) but the URL only
    // registers DELETE (urls/module.py:31-35), so GET 405s after auth.
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/modules/<uuid>/module-issues/<uuid>/",
        gate: Gate::ProjectEntity,
        source: "module.py:736 (ModuleIssueDetailAPIEndpoint; DELETE-only route)",
    },
    RouteGate {
        method: "POST",
        path: "workspaces/<slug>/projects/<id>/modules/<uuid>/archive/",
        gate: Gate::ProjectEntity,
        source: "module.py:891 (ModuleArchiveUnarchiveAPIEndpoint)",
    },
    RouteGate {
        method: "GET",
        path: "workspaces/<slug>/projects/<id>/archived-modules/",
        gate: Gate::ProjectEntity,
        source: "module.py:891 (ModuleArchiveUnarchiveAPIEndpoint; list branch)",
    },
    RouteGate {
        method: "DELETE",
        path: "workspaces/<slug>/projects/<id>/archived-modules/<uuid>/unarchive/",
        gate: Gate::ProjectEntity,
        source: "module.py:891 (ModuleArchiveUnarchiveAPIEndpoint; unarchive branch)",
    },
];

/// Look up the gate for one route+method; `None` is not a registered D-20
/// method (Django answers 405 after auth, 401 for anonymous callers).
pub fn gate_for(method: &str, path: &str) -> Option<&'static RouteGate> {
    GATES
        .iter()
        .find(|row| row.method == method && row.path == path)
}

/// Decide a gate from caller-fetched membership facts.
///
/// Each boolean mirrors one `ProjectMember.objects.filter(...).exists()`
/// with the caller's SQL carrying the `workspace__slug=` / `project_id=`
/// / `is_active=True` filters. `true` passes the request into the handler
/// body, `false` answers 403 [`CLASS_DENIAL_BODY`]. Anonymous callers
/// never reach here (the auth layer 401s first); the kernel's
/// `authenticated` flag denies facts fetched without a user all the same.
///
/// D-20 fact shape (no view defines `project_identifier`): handlers set
/// `has_project_identifier: false`, so safe methods read
/// `is_project_member` (any active role, GUEST included) and writes read
/// `has_project_admin_or_member` (roles 20/15 only).
pub fn decide(
    gate: &Gate,
    method: &str,
    scope: &TenantScope,
    facts: &project::ProjectFacts,
) -> bool {
    match gate {
        Gate::ProjectEntity => project::decide_project_entity(method, scope, facts),
    }
}

/// Which 403 body a denied gate renders: the DRF-default class denial.
/// (Anonymous callers never reach a gate; the auth layer 401s first with
/// [`UNAUTHENTICATED_BODY`].)
pub fn deny_body(gate: &Gate) -> &'static str {
    match gate {
        Gate::ProjectEntity => CLASS_DENIAL_BODY,
    }
}

/// Tenant context for one request: the workspace the URL names. Handlers
/// build membership facts only for this slug, so [`decide`] denies
/// cross-workspace facts even when the rows exist.
pub fn tenant_context(slug: &str) -> TenantScope {
    TenantScope::new(WorkspaceId::from(slug))
}

/// The `handle_exception` branches (`api/views/base.py:133-170`), in the
/// order Python checks them. Handlers map a failure to one variant and
/// render [`exception_envelope`]; anything unlisted is `Other` (the 500
/// branch, after `log_exception`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionKind {
    /// `IntegrityError` → 400 "The payload is not valid".
    IntegrityError,
    /// Django `ValidationError` → 400 "Please provide valid detail".
    ValidationError,
    /// `ObjectDoesNotExist` → 404 "The requested resource does not exist.".
    ObjectDoesNotExist,
    /// `KeyError` → 400 "The required key does not exist.".
    KeyError,
    /// Anything else → 500 "Something went wrong please try again later".
    Other,
}

/// Render one `handle_exception` branch as `(status, body)`, byte-exact.
pub fn exception_envelope(kind: ExceptionKind) -> (u16, &'static str) {
    match kind {
        ExceptionKind::IntegrityError => (400, INVALID_PAYLOAD_BODY),
        ExceptionKind::ValidationError => (400, INVALID_DETAIL_BODY),
        ExceptionKind::ObjectDoesNotExist => (404, NOT_FOUND_BODY),
        ExceptionKind::KeyError => (400, MISSING_KEY_BODY),
        ExceptionKind::Other => (500, SERVER_ERROR_BODY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_types::ProjectId;
    use serde_json::Value;

    const CYCLES: &str = "workspaces/<slug>/projects/<id>/cycles/";
    const CYCLE: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/";
    const CYCLE_ISSUES: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/";
    const CYCLE_ISSUE: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/cycle-issues/<uuid>/";
    const TRANSFER: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/transfer-issues/";
    const CYCLE_ARCHIVE: &str = "workspaces/<slug>/projects/<id>/cycles/<uuid>/archive/";
    const ARCHIVED_CYCLES: &str = "workspaces/<slug>/projects/<id>/archived-cycles/";
    const CYCLE_UNARCHIVE: &str =
        "workspaces/<slug>/projects/<id>/archived-cycles/<uuid>/unarchive/";
    const MODULES: &str = "workspaces/<slug>/projects/<id>/modules/";
    const MODULE: &str = "workspaces/<slug>/projects/<id>/modules/<uuid>/";
    const MODULE_ISSUES: &str = "workspaces/<slug>/projects/<id>/modules/<uuid>/module-issues/";
    const MODULE_ISSUE: &str =
        "workspaces/<slug>/projects/<id>/modules/<uuid>/module-issues/<uuid>/";
    const MODULE_ARCHIVE: &str = "workspaces/<slug>/projects/<id>/modules/<uuid>/archive/";
    const ARCHIVED_MODULES: &str = "workspaces/<slug>/projects/<id>/archived-modules/";
    const MODULE_UNARCHIVE: &str =
        "workspaces/<slug>/projects/<id>/archived-modules/<uuid>/unarchive/";

    fn scope() -> TenantScope {
        tenant_context("acme")
    }

    /// D-20 fact shape: `has_project_identifier` is always false (no view
    /// defines the attribute), so only the membership/role facts vary.
    fn facts(authenticated: bool, member: bool, admin_or_member: bool) -> project::ProjectFacts {
        project::ProjectFacts {
            workspace: WorkspaceId::from("acme"),
            project_id: ProjectId::from("p-1"),
            authenticated,
            is_workspace_member: authenticated,
            has_workspace_admin_or_member: false,
            is_workspace_admin: false,
            is_project_member: member,
            is_project_admin: false,
            has_project_admin_or_member: admin_or_member,
            has_identifier_membership: false,
            has_project_identifier: false,
        }
    }

    fn decide_path(method: &str, path: &str, facts: &project::ProjectFacts) -> bool {
        let row = gate_for(method, path).expect("fixture route must have a gate");
        decide(&row.gate, method, &scope(), facts)
    }

    #[test]
    fn table_covers_every_registered_method() {
        // 13 cycle rows + 11 module rows, in api/urls/ order.
        assert_eq!(GATES.len(), 24, "15 routes, one row per registered method");
        for row in GATES {
            assert!(
                gate_for(row.method, row.path).is_some(),
                "row must round-trip: {} {}",
                row.method,
                row.path
            );
            assert_eq!(row.gate, Gate::ProjectEntity);
            assert!(!row.source.is_empty());
        }
        assert!(gate_for("GET", "nope/").is_none());
        assert!(gate_for("PUT", CYCLES).is_none());
        // Cycle-issue detail serves GET+DELETE ...
        assert!(gate_for("GET", CYCLE_ISSUE).is_some());
        assert!(gate_for("DELETE", CYCLE_ISSUE).is_some());
        // ... while the module twin is DELETE-only (its view-level `get`
        // at module.py:800 is unrouted and 405s after auth).
        assert!(gate_for("GET", MODULE_ISSUE).is_none());
        assert!(gate_for("DELETE", MODULE_ISSUE).is_some());
        // Transfer is POST-only; archives are single-method per pattern.
        assert!(gate_for("GET", TRANSFER).is_none());
        assert!(gate_for("POST", TRANSFER).is_some());
        assert!(gate_for("GET", CYCLE_ARCHIVE).is_none());
        assert!(gate_for("POST", ARCHIVED_CYCLES).is_none());
        assert!(gate_for("POST", MODULE_ARCHIVE).is_some());
        assert!(gate_for("GET", ARCHIVED_MODULES).is_some());
        assert!(gate_for("DELETE", MODULE_UNARCHIVE).is_some());
    }

    /// Full FX-CYCMOD-06 matrix: every registered (method, path) under an
    /// admin/member caller (any active project row with role 20/15), a
    /// guest caller (active row, role 5), and an outsider (no row).
    /// `true` = the handler body runs.
    const MATRIX: &[(&str, &str, bool, bool, bool)] = &[
        ("GET", CYCLES, true, true, false),
        ("POST", CYCLES, true, false, false),
        ("GET", CYCLE, true, true, false),
        ("PATCH", CYCLE, true, false, false),
        ("DELETE", CYCLE, true, false, false),
        ("GET", CYCLE_ISSUES, true, true, false),
        ("POST", CYCLE_ISSUES, true, false, false),
        ("GET", CYCLE_ISSUE, true, true, false),
        ("DELETE", CYCLE_ISSUE, true, false, false),
        ("POST", TRANSFER, true, false, false),
        ("POST", CYCLE_ARCHIVE, true, false, false),
        ("GET", ARCHIVED_CYCLES, true, true, false),
        ("DELETE", CYCLE_UNARCHIVE, true, false, false),
        ("GET", MODULES, true, true, false),
        ("POST", MODULES, true, false, false),
        ("GET", MODULE, true, true, false),
        ("PATCH", MODULE, true, false, false),
        ("DELETE", MODULE, true, false, false),
        ("GET", MODULE_ISSUES, true, true, false),
        ("POST", MODULE_ISSUES, true, false, false),
        ("DELETE", MODULE_ISSUE, true, false, false),
        ("POST", MODULE_ARCHIVE, true, false, false),
        ("GET", ARCHIVED_MODULES, true, true, false),
        ("DELETE", MODULE_UNARCHIVE, true, false, false),
    ];

    #[test]
    fn decision_matrix_matches_entity_permission() {
        assert_eq!(MATRIX.len(), GATES.len(), "matrix must cover every row");
        let privileged = facts(true, true, true);
        let guest = facts(true, true, false);
        let outsider = facts(true, false, false);
        for (method, path, want_priv, want_guest, want_out) in MATRIX {
            // The matrix must pin table rows that exist.
            assert!(
                gate_for(method, path).is_some(),
                "matrix row must be a table row: {method} {path}"
            );
            assert_eq!(
                decide_path(method, path, &privileged),
                *want_priv,
                "admin/member {method} {path}"
            );
            assert_eq!(
                decide_path(method, path, &guest),
                *want_guest,
                "guest {method} {path}"
            );
            assert_eq!(
                decide_path(method, path, &outsider),
                *want_out,
                "outsider {method} {path}"
            );
        }
    }

    #[test]
    fn anonymous_and_cross_workspace_facts_deny() {
        // Anonymous callers never reach a gate (the auth layer 401s), but
        // facts fetched without a user deny all the same.
        let anon = facts(false, true, true);
        assert!(!decide_path("GET", CYCLES, &anon));
        assert!(!decide_path("POST", CYCLES, &anon));
        assert!(!decide_path("GET", MODULES, &anon));
        assert!(!decide_path("DELETE", MODULE, &anon));
        // Facts fetched for a different workspace deny even when the rows
        // exist (the tenant_context half of the pattern).
        let mut foreign = facts(true, true, true);
        foreign.workspace = WorkspaceId::from("other");
        assert!(!decide_path("GET", CYCLES, &foreign));
        assert!(!decide_path("POST", MODULES, &foreign));
    }

    #[test]
    fn identifier_branch_is_dead_for_d20_but_kernel_pinned() {
        // Every D-20 fact set carries has_project_identifier: false.
        let no_ident = facts(true, true, true);
        assert!(!no_ident.has_project_identifier);
        // The kernel branch itself still behaves (project.py:91-98): safe
        // methods read the identifier membership, writes ignore it.
        let mut ident = facts(true, false, false);
        ident.has_project_identifier = true;
        ident.has_identifier_membership = true;
        assert!(decide_path("GET", CYCLES, &ident));
        assert!(!decide_path("POST", CYCLES, &ident));
    }

    #[test]
    fn denial_bodies_are_byte_exact() {
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        // DRF PermissionDenied.default_detail, compact separators.
        assert_eq!(
            CLASS_DENIAL_BODY,
            r#"{"detail":"You do not have permission to perform this action."}"#
        );
        assert_eq!(CLASS_DENIAL_BODY, crate::permissions::DEFAULT_DENIED_BODY);
        assert_eq!(deny_body(&Gate::ProjectEntity), CLASS_DENIAL_BODY);
        // BaseAPIView spellings (base.py:133-170), not the BaseViewSet ones.
        assert_eq!(
            NOT_FOUND_BODY,
            r#"{"error":"The requested resource does not exist."}"#
        );
        assert_ne!(
            NOT_FOUND_BODY, r#"{"error":"The required object does not exist."}"#,
            "that spelling is the BaseViewSet variant other planes pin"
        );
        assert_eq!(
            INVALID_PAYLOAD_BODY,
            r#"{"error":"The payload is not valid"}"#
        );
        assert_eq!(
            INVALID_DETAIL_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
        assert_eq!(
            MISSING_KEY_BODY,
            r#"{"error":"The required key does not exist."}"#
        );
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
    }

    #[test]
    fn exception_envelopes_follow_handle_exception_order() {
        assert_eq!(
            exception_envelope(ExceptionKind::IntegrityError),
            (400, INVALID_PAYLOAD_BODY)
        );
        assert_eq!(
            exception_envelope(ExceptionKind::ValidationError),
            (400, INVALID_DETAIL_BODY)
        );
        assert_eq!(
            exception_envelope(ExceptionKind::ObjectDoesNotExist),
            (404, NOT_FOUND_BODY)
        );
        assert_eq!(
            exception_envelope(ExceptionKind::KeyError),
            (400, MISSING_KEY_BODY)
        );
        assert_eq!(
            exception_envelope(ExceptionKind::Other),
            (500, SERVER_ERROR_BODY)
        );
    }

    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/v1_cycles_modules/guards/permissions.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("FX-CYCMOD-06 golden exists"))
            .expect("golden parses")
    }

    /// The FX-CYCMOD-06 vectors: the port's matrix and envelopes match what
    /// the fixture recorded from the Python sources.
    #[test]
    fn vectors_match_fx_cycmod_06_golden() {
        let gold = golden();
        assert_eq!(gold["fixture"], Value::String("FX-CYCMOD-06".to_owned()));
        // Trace names the ported sources.
        let trace = gold["trace"].as_array().expect("trace is a list");
        let trace_text = serde_json::to_string(trace).expect("trace serializes");
        for needle in [
            "app/permissions/project.py:85-116",
            "api/views/base.py:133-182",
            "api/urls/cycle.py:16-57",
            "api/urls/module.py:15-51",
        ] {
            assert!(trace_text.contains(needle), "trace names {needle}");
        }
        // Matrix: safe reads need any active project row; writes need
        // roles 20/15; guests 403 on writes; anonymous 401s everywhere.
        let matrix = &gold["matrix"];
        let safe = matrix["SAFE_METHODS"].as_str().expect("SAFE prose");
        assert!(
            safe.contains("project_id"),
            "safe methods are project-scoped"
        );
        let writes = matrix["POST_PATCH_DELETE"].as_str().expect("write prose");
        assert!(writes.contains("20 admin") && writes.contains("15 member"));
        assert!(writes.contains("GUEST(5)") && writes.contains("403"));
        let anon = matrix["anonymous"].as_str().expect("anon prose");
        assert!(anon.contains("401"));
        // Envelopes: every shared handle_exception literal the fixture
        // lists renders from this module's consts.
        let envelopes = &gold["error_envelopes"];
        let all = serde_json::to_string(envelopes).expect("envelopes serialize");
        for body in [
            NOT_FOUND_BODY,
            INVALID_PAYLOAD_BODY,
            INVALID_DETAIL_BODY,
            MISSING_KEY_BODY,
            SERVER_ERROR_BODY,
        ] {
            // The fixture quotes the inner literal; the const carries the
            // full compact body — both must contain the same message text.
            let message: &str = body
                .strip_prefix(r#"{"error":""#)
                .and_then(|rest| rest.strip_suffix(r#""}"#))
                .expect("error const shape");
            assert!(all.contains(message), "golden lists {message}");
        }
        // The 404 the fixture lists is the BaseAPIView spelling.
        let not_found = envelopes["404"].as_array().expect("404 list");
        let not_found_text = serde_json::to_string(not_found).expect("404 serializes");
        assert!(not_found_text.contains("The requested resource does not exist."));
        // The gate span the fixture records is has_permission's body.
        let spans = gold["permission_spans"].as_array().expect("spans list");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0]["name"], Value::String("has_permission".to_owned()));
        assert_eq!(spans[0]["span"], serde_json::json!([86, 116]));
    }
}
