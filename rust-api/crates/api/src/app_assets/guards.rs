//! Asset permission gates + asset throttle key (D-31, stage 5).
//!
//! Ports the `@allow_permission` / `permission_classes` / `throttle_classes`
//! lines of `apps/api/pi_dash/app/views/asset/v2.py` plus the default-auth
//! statement for `apps/api/pi_dash/app/views/asset/base.py` (v1 endpoints
//! inherit `BaseAPIView` / `BaseViewSet` auth unchanged). Fixture:
//! `rust-api/fixtures/app_assets/guards/permissions.golden.json`
//! (trace: `rust-api/fixtures/app_assets/TRACE.md`).
//!
//! Shape of the port: the decision kernel (`decide_allow` over
//! caller-fetched membership facts) lives in the read-only F-06 foundation
//! (`pidash_auth::permissions::allow`); this module only pins which gate
//! each endpoint carries and the throttle key/rate the duplicate endpoint
//! enforces. Row fetching stays with the handler layer (issues 394/400/412).
//!
//! Gate order (preserved, not redesigned): DRF `initial()` runs
//! authentication, then `check_permissions`, then `check_throttles`, so on
//! the duplicate endpoint a denied outsider answers 403 even when the
//! per-asset quota is also exhausted — 429 fires only for callers that
//! survive the gate.
//!
//! Response shapes (handlers render these; recorded here so the matrix has
//! one home):
//!
//! * Anonymous on any gated or auth-only route: 401
//!   `{"detail":"Authentication credentials were not provided."}` — the
//!   session layer answers before any gate runs. `StaticFileAssetEndpoint`
//!   (`AllowAny`) is the one route with no auth at all.
//! * Denied member: 403 [`crate::permissions::PERMISSION_DENIED_BODY`].
//! * Throttled duplicate caller: 429 [`ASSET_RATE_LIMIT_BODY`].
//!
//! Ported quirks (translate, don't redesign):
//!
//! * Entity-level gates (`ProjectAssetEndpoint`, `ProjectBulkAssetEndpoint`)
//!   pass no `level`, so they run the default `"PROJECT"` branch — including
//!   its workspace-admin bypass (an admin with any project row passes
//!   without a project role). A workspace member with no project row gets
//!   403 even with the highest workspace role.
//! * Ungated workspace/user routes (`WorkspaceFileAssetEndpoint`,
//!   `UserAssetsV2Endpoint`) enforce authentication only: any signed-in user
//!   may act on any workspace's rows. That gap is shipped Django behavior,
//!   pinned by the tenant contract suite — reproduced here, not fixed.
//! * `AssetRateThrottle.get_cache_key` returns `None` for a missing or
//!   falsy `asset_id` kwarg, and DRF skips throttles with a `None` key
//!   (no throttling for that request). The duplicate route always captures
//!   `<uuid:asset_id>`, so the `None` path is defensive only.
//! * The throttle key is per source asset, not per caller: no user or IP
//!   fragment enters it.

use pidash_auth::permissions::allow::{
    decide_allow, AllowFacts, AllowLevel, AllowSpec, CreatorGate,
};
use pidash_auth::scope::TenantScope;

/// The gate one asset endpoint carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetGate {
    /// `permission_classes = [AllowAny]`: no auth, no membership check
    /// (`StaticFileAssetEndpoint`, `v2.py:435`).
    AllowAny,
    /// No decorator: `BaseAPIView` default `IsAuthenticated` only. Any
    /// signed-in user passes; anonymous callers 401 before the gate.
    AuthOnly,
    /// `@allow_permission([ADMIN, MEMBER, GUEST], level="WORKSPACE")`:
    /// active workspace membership with an allowed role.
    Workspace,
    /// `@allow_permission([ADMIN, MEMBER, GUEST])` with the default level
    /// (`"PROJECT"`), or an explicit `level="PROJECT"`: active project
    /// membership with an allowed role, plus the workspace-admin bypass.
    Project,
}

/// Every v2 asset endpoint with its gate and Python source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetEndpoint {
    /// `UserAssetsV2Endpoint.post` (`v2.py:109`): no gate.
    UserPost,
    /// `UserAssetsV2Endpoint.patch` (`v2.py:170`): no gate.
    UserPatch,
    /// `UserAssetsV2Endpoint.delete` (`v2.py:191`): no gate.
    UserDelete,
    /// `WorkspaceFileAssetEndpoint.post` (`v2.py:314`): no gate.
    WorkspacePost,
    /// `WorkspaceFileAssetEndpoint.patch` (`v2.py:379`): no gate.
    WorkspacePatch,
    /// `WorkspaceFileAssetEndpoint.delete` (`v2.py:400`): no gate.
    WorkspaceDelete,
    /// `WorkspaceFileAssetEndpoint.get` (`v2.py:409`): no gate.
    WorkspaceGet,
    /// `StaticFileAssetEndpoint.get` (`v2.py:437`): `AllowAny`.
    StaticGet,
    /// `AssetRestoreEndpoint.post` (`v2.py:471`): WORKSPACE.
    RestorePost,
    /// `ProjectAssetEndpoint.post` (`v2.py:512`): default PROJECT.
    ProjectPost,
    /// `ProjectAssetEndpoint.patch` (`v2.py:579`): default PROJECT.
    ProjectPatch,
    /// `ProjectAssetEndpoint.delete` (`v2.py:595`): default PROJECT.
    ProjectDelete,
    /// `ProjectAssetEndpoint.get` (`v2.py:606`): default PROJECT.
    ProjectGet,
    /// `ProjectBulkAssetEndpoint.post` (`v2.py:636`): default PROJECT.
    BulkPost,
    /// `AssetCheckEndpoint.get` (`v2.py:694`): WORKSPACE.
    CheckGet,
    /// `DuplicateAssetEndpoint.post` (`v2.py:736`): WORKSPACE + throttle.
    DuplicatePost,
    /// `WorkspaceAssetDownloadEndpoint.get` (`v2.py:786`): WORKSPACE.
    WorkspaceDownloadGet,
    /// `ProjectAssetDownloadEndpoint.get` (`v2.py:813`): PROJECT.
    ProjectDownloadGet,
}

/// The v1 legacy surface (`app/views/asset/base.py`): every method inherits
/// the base-class default auth with no per-view gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyAssetEndpoint {
    /// `FileAssetEndpoint.get` (`base.py:23`).
    FileGet,
    /// `FileAssetEndpoint.post` (`base.py:35`).
    FilePost,
    /// `FileAssetEndpoint.delete` (`base.py:44`).
    FileDelete,
    /// `FileAssetViewSet.restore` (`base.py:53`).
    FileRestore,
    /// `UserAssetsEndpoint.get` (`base.py:64`).
    UserGet,
    /// `UserAssetsEndpoint.post` (`base.py:75`).
    UserPost,
    /// `UserAssetsEndpoint.delete` (`base.py:82`).
    UserDelete,
}

/// Map a v2 endpoint to its gate.
pub fn gate_for(endpoint: AssetEndpoint) -> AssetGate {
    match endpoint {
        AssetEndpoint::UserPost
        | AssetEndpoint::UserPatch
        | AssetEndpoint::UserDelete
        | AssetEndpoint::WorkspacePost
        | AssetEndpoint::WorkspacePatch
        | AssetEndpoint::WorkspaceDelete
        | AssetEndpoint::WorkspaceGet => AssetGate::AuthOnly,
        AssetEndpoint::StaticGet => AssetGate::AllowAny,
        AssetEndpoint::RestorePost
        | AssetEndpoint::CheckGet
        | AssetEndpoint::DuplicatePost
        | AssetEndpoint::WorkspaceDownloadGet => AssetGate::Workspace,
        AssetEndpoint::ProjectPost
        | AssetEndpoint::ProjectPatch
        | AssetEndpoint::ProjectDelete
        | AssetEndpoint::ProjectGet
        | AssetEndpoint::BulkPost
        | AssetEndpoint::ProjectDownloadGet => AssetGate::Project,
    }
}

/// The v1 surface is uniformly auth-only; kept as a function (rather than a
/// constant) so handlers call the same shape as [`gate_for`].
pub fn legacy_gate_for(_endpoint: LegacyAssetEndpoint) -> AssetGate {
    AssetGate::AuthOnly
}

/// The kernel spec for a gate: `None` for [`AssetGate::AllowAny`] and
/// [`AssetGate::AuthOnly`] (no `@allow_permission` runs there).
///
/// No D-31 endpoint uses the creator bypass (`creator=False` everywhere),
/// so `creator_bypass` is `false` for both levels; `creator_gate` stays the
/// `App` copy (`app/permissions/base.py`), the only tree these views import.
pub fn allow_spec(gate: AssetGate) -> Option<AllowSpec> {
    match gate {
        AssetGate::AllowAny | AssetGate::AuthOnly => None,
        AssetGate::Workspace => Some(AllowSpec {
            level: AllowLevel::Workspace,
            creator_gate: CreatorGate::App,
            creator_bypass: false,
        }),
        AssetGate::Project => Some(AllowSpec {
            level: AllowLevel::Project,
            creator_gate: CreatorGate::App,
            creator_bypass: false,
        }),
    }
}

/// Decide a v2 endpoint: `AllowAny` passes without inspecting facts
/// (anonymous callers included); every other gate delegates to the F-06
/// kernel. Anonymous callers match no membership row, so the kernel denies
/// them — the handler layer maps that denial to 401 (never reached the
/// gate) versus 403.
pub fn check_asset_gate(endpoint: AssetEndpoint, scope: &TenantScope, facts: &AllowFacts) -> bool {
    match gate_for(endpoint) {
        AssetGate::AllowAny => true,
        AssetGate::AuthOnly => facts.authenticated,
        gate => match allow_spec(gate) {
            Some(spec) => decide_allow(&spec, scope, facts),
            // `allow_spec` returns `Some` for both remaining gates; the
            // fallback keeps the match exhaustive if that ever changes.
            None => false,
        },
    }
}

// ---------------------------------------------------------------------------
// AssetRateThrottle (`throttles/asset.py:8-15`, `v2.py:701`)
// ---------------------------------------------------------------------------

/// DRF `scope` class attribute, also the `DEFAULT_THROTTLE_RATES` key
/// (`settings/common.py:95`).
pub const ASSET_THROTTLE_SCOPE: &str = "asset_id";
/// `5/minute`: five duplicate attempts per source asset per minute; the
/// sixth trips (`test_duplicate_throttle`: five 404s, then 429).
pub const ASSET_THROTTLE_REQUESTS: u32 = 5;
/// Rate window in seconds.
pub const ASSET_THROTTLE_WINDOW_SECS: u64 = 60;

/// Exact bytes of a throttled duplicate response: the product rate-limit
/// envelope (`AUTHENTICATION_ERROR_CODES["RATE_LIMIT_EXCEEDED"] = 5900` in
/// `authentication/adapter/error.py:71`, raised via
/// `authentication/rate_limit.py:24,43`), not the DRF `{"detail"}` shape.
pub const ASSET_RATE_LIMIT_BODY: &str =
    r#"{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}"#;

/// `True` only for the duplicate endpoint: the sole throttled route in the
/// domain (`throttle_classes = [AssetRateThrottle]`, `v2.py:701`).
pub fn is_throttled(endpoint: AssetEndpoint) -> bool {
    matches!(endpoint, AssetEndpoint::DuplicatePost)
}

/// `AssetRateThrottle.get_cache_key`: `f"throttle_asset_{asset_id}"` from
/// `view.kwargs`, `None` when the kwarg is missing or falsy
/// (`throttles/asset.py:11-15`) — and DRF skips throttles with a `None` key.
/// Only the empty string is falsy among present values (whitespace is
/// truthy in Python), so `Some("")` maps to `None` while any other
/// `Some(..)` — including whitespace — produces a key.
pub fn asset_throttle_key(asset_id: Option<&str>) -> Option<String> {
    match asset_id {
        None | Some("") => None,
        Some(id) => Some(format!("throttle_asset_{id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_auth::permissions::{ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER};
    use pidash_types::WorkspaceId;

    fn scope() -> TenantScope {
        TenantScope::new(WorkspaceId::from("acme"))
    }

    fn authed() -> AllowFacts {
        AllowFacts {
            workspace: WorkspaceId::from("acme"),
            authenticated: true,
            is_workspace_member: false,
            has_allowed_workspace_role: false,
            is_creator: false,
            has_allowed_project_role: false,
            is_project_member: false,
            is_workspace_admin: false,
        }
    }

    fn anon() -> AllowFacts {
        AllowFacts {
            authenticated: false,
            ..authed()
        }
    }

    fn ws_member() -> AllowFacts {
        AllowFacts {
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            ..authed()
        }
    }

    fn project_member() -> AllowFacts {
        AllowFacts {
            is_workspace_member: true,
            has_allowed_workspace_role: true,
            has_allowed_project_role: true,
            is_project_member: true,
            ..authed()
        }
    }

    #[test]
    fn role_values_match_python() {
        // `app/permissions/base.py:13-16`.
        assert_eq!(ROLE_ADMIN, 20);
        assert_eq!(ROLE_MEMBER, 15);
        assert_eq!(ROLE_GUEST, 5);
    }

    #[test]
    fn allow_any_serves_anonymous() {
        assert_eq!(gate_for(AssetEndpoint::StaticGet), AssetGate::AllowAny);
        assert!(allow_spec(AssetGate::AllowAny).is_none());
        assert!(check_asset_gate(
            AssetEndpoint::StaticGet,
            &scope(),
            &anon()
        ));
        assert!(check_asset_gate(
            AssetEndpoint::StaticGet,
            &scope(),
            &authed()
        ));
    }

    #[test]
    fn auth_only_passes_any_signed_in_user() {
        // v2 user/workspace methods (v2.py:109,170,191 + :314,379,400,409):
        // no decorator, so even a cross-workspace caller passes — the
        // shipped gap the tenant suite pins.
        let endpoints = [
            AssetEndpoint::UserPost,
            AssetEndpoint::UserPatch,
            AssetEndpoint::UserDelete,
            AssetEndpoint::WorkspacePost,
            AssetEndpoint::WorkspacePatch,
            AssetEndpoint::WorkspaceDelete,
            AssetEndpoint::WorkspaceGet,
        ];
        for endpoint in endpoints {
            assert_eq!(gate_for(endpoint), AssetGate::AuthOnly, "{endpoint:?}");
            assert!(check_asset_gate(endpoint, &scope(), &authed()));
            assert!(!check_asset_gate(endpoint, &scope(), &anon()));
        }
    }

    #[test]
    fn legacy_surface_is_auth_only() {
        let endpoints = [
            LegacyAssetEndpoint::FileGet,
            LegacyAssetEndpoint::FilePost,
            LegacyAssetEndpoint::FileDelete,
            LegacyAssetEndpoint::FileRestore,
            LegacyAssetEndpoint::UserGet,
            LegacyAssetEndpoint::UserPost,
            LegacyAssetEndpoint::UserDelete,
        ];
        for endpoint in endpoints {
            assert_eq!(
                legacy_gate_for(endpoint),
                AssetGate::AuthOnly,
                "{endpoint:?}"
            );
        }
    }

    #[test]
    fn workspace_gate_needs_allowed_workspace_role() {
        let endpoints = [
            AssetEndpoint::RestorePost,
            AssetEndpoint::CheckGet,
            AssetEndpoint::DuplicatePost,
            AssetEndpoint::WorkspaceDownloadGet,
        ];
        for endpoint in endpoints {
            assert_eq!(gate_for(endpoint), AssetGate::Workspace, "{endpoint:?}");
            // Guests pass with workspace membership: every gate in this
            // domain lists [ADMIN, MEMBER, GUEST].
            assert!(
                check_asset_gate(endpoint, &scope(), &ws_member()),
                "{endpoint:?}"
            );
            // Outsider (no membership row) is denied...
            assert!(
                !check_asset_gate(endpoint, &scope(), &authed()),
                "{endpoint:?}"
            );
            // ...and anonymous never reaches the gate.
            assert!(
                !check_asset_gate(endpoint, &scope(), &anon()),
                "{endpoint:?}"
            );
        }
    }

    #[test]
    fn workspace_gate_denies_wrong_role() {
        let mut facts = ws_member();
        facts.has_allowed_workspace_role = false;
        assert!(!check_asset_gate(AssetEndpoint::CheckGet, &scope(), &facts));
    }

    #[test]
    fn project_gate_needs_project_row() {
        let endpoints = [
            AssetEndpoint::ProjectPost,
            AssetEndpoint::ProjectPatch,
            AssetEndpoint::ProjectDelete,
            AssetEndpoint::ProjectGet,
            AssetEndpoint::BulkPost,
            AssetEndpoint::ProjectDownloadGet,
        ];
        for endpoint in endpoints {
            assert_eq!(gate_for(endpoint), AssetGate::Project, "{endpoint:?}");
            assert!(
                check_asset_gate(endpoint, &scope(), &project_member()),
                "{endpoint:?}"
            );
            // Workspace membership alone (no project row) 403s.
            assert!(
                !check_asset_gate(endpoint, &scope(), &ws_member()),
                "{endpoint:?}"
            );
            assert!(
                !check_asset_gate(endpoint, &scope(), &authed()),
                "{endpoint:?}"
            );
            assert!(
                !check_asset_gate(endpoint, &scope(), &anon()),
                "{endpoint:?}"
            );
        }
    }

    #[test]
    fn project_gate_workspace_admin_bypass() {
        // The default-PROJECT branch: a workspace admin who holds any
        // project row passes without an allowed project role
        // (`app/permissions/base.py:64-78`).
        let mut facts = authed();
        facts.is_workspace_member = true;
        facts.is_project_member = true;
        facts.is_workspace_admin = true;
        assert!(check_asset_gate(
            AssetEndpoint::ProjectPost,
            &scope(),
            &facts
        ));
        // Either half alone still denies.
        let mut no_project = authed();
        no_project.is_workspace_admin = true;
        assert!(!check_asset_gate(
            AssetEndpoint::ProjectPost,
            &scope(),
            &no_project
        ));
        let mut no_admin = authed();
        no_admin.is_project_member = true;
        assert!(!check_asset_gate(
            AssetEndpoint::ProjectPost,
            &scope(),
            &no_admin
        ));
    }

    #[test]
    fn gates_deny_cross_workspace_facts() {
        let other = TenantScope::new(WorkspaceId::from("other"));
        assert!(!check_asset_gate(
            AssetEndpoint::CheckGet,
            &other,
            &ws_member()
        ));
        assert!(!check_asset_gate(
            AssetEndpoint::ProjectGet,
            &other,
            &project_member()
        ));
    }

    #[test]
    fn only_duplicate_is_throttled() {
        for endpoint in [
            AssetEndpoint::UserPost,
            AssetEndpoint::WorkspacePost,
            AssetEndpoint::StaticGet,
            AssetEndpoint::RestorePost,
            AssetEndpoint::ProjectPost,
            AssetEndpoint::BulkPost,
            AssetEndpoint::CheckGet,
            AssetEndpoint::WorkspaceDownloadGet,
            AssetEndpoint::ProjectDownloadGet,
        ] {
            assert!(!is_throttled(endpoint), "{endpoint:?}");
        }
        assert!(is_throttled(AssetEndpoint::DuplicatePost));
    }

    #[test]
    fn throttle_key_goldens() {
        // Fixture `throttle.goldens` in `guards/permissions.golden.json`.
        assert_eq!(
            asset_throttle_key(Some("3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f")),
            Some("throttle_asset_3f9a2c1e-9b1a-4d2e-8f3a-1a2b3c4d5e6f".to_owned())
        );
        assert_eq!(asset_throttle_key(None), None);
        assert_eq!(asset_throttle_key(Some("")), None);
    }

    #[test]
    fn throttle_rate_and_envelope() {
        assert_eq!(ASSET_THROTTLE_SCOPE, "asset_id");
        assert_eq!(ASSET_THROTTLE_REQUESTS, 5);
        assert_eq!(ASSET_THROTTLE_WINDOW_SECS, 60);
        assert_eq!(
            ASSET_RATE_LIMIT_BODY,
            r#"{"error_code":5900,"error_message":"RATE_LIMIT_EXCEEDED"}"#
        );
    }
}
