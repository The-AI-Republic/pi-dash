#![forbid(unsafe_code)]

//! License console gate (`license/api/permissions/instance.py`).
//!
//! `InstanceAdminPermission.has_permission`: anonymous denies; otherwise the
//! first `Instance` row (`Meta.ordering = ("-created_at",)`) must have an
//! `InstanceAdmin` row for the user with `role__gte=15`. There is no
//! active/verified filter on the row — ported as written.

/// Minimum `InstanceAdmin.role` that passes (`role__gte=15`: Admin=20 and
/// Member=15; Guest=5 is excluded).
pub const INSTANCE_ADMIN_MIN_ROLE: i32 = 15;

/// Mirror of `has_permission`: `has_instance` is "an `Instance` row exists",
/// `has_admin_row` is "`InstanceAdmin` with `role >= 15` exists for
/// `(instance, user)`".
pub fn decide_instance_admin(authenticated: bool, has_instance: bool, has_admin_row: bool) -> bool {
    if !authenticated {
        return false;
    }
    if !has_instance {
        // `InstanceAdmin.objects.filter(instance=None, ...)` matches
        // nothing (the FK is non-nullable), so no instance denies.
        return false;
    }
    has_admin_row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_row_with_instance_passes() {
        assert!(decide_instance_admin(true, true, true));
    }

    #[test]
    fn everything_else_denies() {
        assert!(!decide_instance_admin(false, true, true));
        assert!(!decide_instance_admin(true, false, true));
        assert!(!decide_instance_admin(true, true, false));
        assert!(!decide_instance_admin(true, false, false));
    }
}
