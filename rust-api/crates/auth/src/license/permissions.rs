#![forbid(unsafe_code)]

//! License console gate, D-01 domain surface
//! (`apps/api/pi_dash/license/api/permissions/instance.py:12-18`).
//!
//! `InstanceAdminPermission.has_permission`: anonymous denies without a DB
//! hit (`instance.py:14-15`); otherwise the first `Instance` row
//! (`Meta.ordering = ("-created_at",)`, `models/instance.py:46-50`) must
//! have an `InstanceAdmin` row for the user with `role__gte=15`
//! (`instance.py:17-18`).
//!
//! The decision itself is inherited from the F-06 kernel
//! ([`crate::permissions::instance`]) — this module only fixes the D-01
//! calling convention, so handler layers import one domain path. The
//! threshold is re-exported, never redefined, so the gate cannot drift from
//! the kernel.
//!
//! Ported quirks (kept as written, listed for follow-up):
//!
//! * The gate is `role__gte=15` while `ROLE_CHOICES`
//!   (`models/instance.py:15`) only defines `20` ("Admin") — any stored
//!   value `>= 15` passes. The fixture pins the boundary (`15` allows,
//!   `10` denies).
//! * There is no active/verified filter on the `InstanceAdmin` row: an
//!   inactive or unverified row still passes.
//!
//! Throttles: none exist in this domain. No `throttle_classes` appear
//! anywhere under `license/`, and no `DEFAULT_THROTTLE_CLASSES` single out
//! these routes, so there is no throttle code to port. The contract suite's
//! permission-removal probe (`contract-tests/license/test_permissions.py`)
//! is the only guard test at the HTTP layer.
//!
//! Per-endpoint overrides (for the handler layers; verified against the
//! views — the guard module itself takes no position on them):
//!
//! * `InstanceEndpoint` (`views/instance.py:28-32`): `PATCH` requires this
//!   guard (`get_permissions`), every other method is `AllowAny`.
//! * `SignUpScreenVisitedEndpoint` (`views/instance.py:186-187`) and
//!   `InstanceAdminUserSessionEndpoint` (`views/admin.py:369-370`):
//!   `AllowAny`.
//! * `InstanceAdminSignUpEndpoint` (`views/admin.py:89`) and
//!   `InstanceAdminSignInEndpoint` (`views/admin.py:242`) are plain Django
//!   `View`s — the `permission_classes = [AllowAny]` attributes on them are
//!   dead; no DRF permission layer runs at all.
//! * `InstanceAdminSignOutEndpoint` (`views/admin.py:382-383`) is likewise a
//!   plain Django `View`, so its `permission_classes =
//!   [InstanceAdminPermission]` attribute is dead too — DRF never enforces
//!   it. Handler behavior there belongs to PIDASHCONV-121.
//! * Every other license `BaseAPIView` inherits the default
//!   `permission_classes = [InstanceAdminPermission]`
//!   (`views/base.py:43`): deny-by-default.

use crate::permissions::instance::{decide_instance_admin, INSTANCE_ADMIN_MIN_ROLE};

/// The minimum `InstanceAdmin.role` that passes, re-exported from the F-06
/// kernel (`role__gte=15`, `permissions/instance.py:18`).
pub use crate::permissions::instance::INSTANCE_ADMIN_MIN_ROLE as D01_ADMIN_MIN_ROLE;

/// Facts for one `InstanceAdminPermission.has_permission` evaluation.
///
/// * `authenticated` — `request.user.is_anonymous` is false. Anonymous
///   denies before any DB access (`instance.py:14-15`).
/// * `instance_present` — `Instance.objects.first()` returned a row. When it
///   is `None`, the filter runs with `instance=None`, which matches nothing
///   (the FK is non-nullable), so the result is `False`.
/// * `has_admin_row` — `InstanceAdmin.objects.filter(role__gte=15,
///   instance=<first>, user=request.user).exists()`: one boolean combining
///   the role threshold, the first-instance scoping (a row for another
///   instance does not match), and row presence.
pub struct InstanceAdminFacts {
    pub authenticated: bool,
    pub instance_present: bool,
    pub has_admin_row: bool,
}

/// Mirror of `InstanceAdminPermission.has_permission` for the D-01 domain.
///
/// Delegates to the F-06 kernel without adding semantics: anonymous denies,
/// a missing first instance denies, otherwise the pre-fetched
/// `has_admin_row` decides.
pub fn instance_admin_allows(facts: &InstanceAdminFacts) -> bool {
    decide_instance_admin(
        facts.authenticated,
        facts.instance_present,
        facts.has_admin_row,
    )
}

/// The role threshold, exposed for completeness so callers decomposing a row
/// (rather than a pre-evaluated `.exists()`) apply the same gate Django
/// does. Prefers the kernel constant; kept here only as documentation of
/// the decomposition the fixture rows use.
pub fn role_passes(role: i32) -> bool {
    role >= INSTANCE_ADMIN_MIN_ROLE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn golden_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/license/guards/instance_admin_permission.golden.json")
    }

    fn golden() -> serde_json::Value {
        let path = golden_path();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        serde_json::from_str(&text).expect("golden fixture is valid JSON")
    }

    /// Map one matrix row to the `.exists()` boolean the Python filter
    /// yields: the row must belong to the first instance and carry a role
    /// `>= 15`. A `null` role (authenticated non-admin) means no row. The
    /// fixture encodes the other-instance case as the
    /// `"admin-of-OTHER-instance"` user label (with a `why` note); every
    /// other labelled user owns a row on the first instance.
    fn row_exists(row: &serde_json::Value) -> bool {
        let same_instance = row
            .get("user")
            .and_then(|u| u.as_str())
            .is_some_and(|u| u != "admin-of-OTHER-instance");
        let role_ok = row
            .get("role")
            .and_then(|r| r.as_i64())
            .is_some_and(|r| role_passes(r as i32));
        same_instance && role_ok
    }

    #[test]
    fn golden_matrix_replays_exactly() {
        let g = golden();
        let matrix = g.get("matrix").expect("matrix").as_array().expect("array");
        assert!(!matrix.is_empty(), "matrix must not be empty");
        for row in matrix {
            let expected = row.get("result").expect("result").as_bool().expect("bool");
            let facts = InstanceAdminFacts {
                authenticated: true,
                instance_present: row
                    .get("instance_present")
                    .expect("instance_present")
                    .as_bool()
                    .expect("bool"),
                has_admin_row: row_exists(row),
            };
            assert_eq!(
                instance_admin_allows(&facts),
                expected,
                "matrix row replay mismatch: {row}"
            );
        }
    }

    #[test]
    fn anonymous_denies_without_db() {
        let g = golden();
        assert_eq!(
            g.get("anonymous")
                .expect("anonymous")
                .get("result")
                .expect("result"),
            &serde_json::Value::Bool(false)
        );
        // Anonymous denies even if rows would otherwise allow.
        let facts = InstanceAdminFacts {
            authenticated: false,
            instance_present: true,
            has_admin_row: true,
        };
        assert!(!instance_admin_allows(&facts));
    }

    #[test]
    fn missing_instance_denies() {
        let g = golden();
        assert_eq!(
            g.get("no_instance_row")
                .expect("no_instance_row")
                .get("result")
                .expect("result"),
            &serde_json::Value::Bool(false)
        );
        // No first instance denies even for an authenticated admin row.
        let facts = InstanceAdminFacts {
            authenticated: true,
            instance_present: false,
            has_admin_row: true,
        };
        assert!(!instance_admin_allows(&facts));
    }

    #[test]
    fn threshold_boundary_matches_fixture() {
        // `role__gte=15`: 15 passes, 14 does not. ROLE_CHOICES only defines
        // 20, so the boundary is the latent wider gate, ported exactly.
        assert!(role_passes(15));
        assert!(!role_passes(14));
        assert!(role_passes(20));
        assert!(!role_passes(10));
    }
}
