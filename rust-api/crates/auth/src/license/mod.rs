//! License / instance-console domain surface (D-01, stage 3).
//!
//! Ports `apps/api/pi_dash/license/api/permissions/` for the auth layer:
//!
//! * [`permissions`] — `instance.py:12-18` (`InstanceAdminPermission`).
//!
//! The rule itself lives in the F-06 kernel
//! (`crate::permissions::instance`); this module is the domain's calling
//! convention over that kernel, so downstream D-01 layers (`handlers`)
//! import from one domain path. No behavior is forked here: every semantic,
//! including the ported quirks, is inherited from the kernel and documented
//! at the function.
//!
//! Wiring note: the crate root declares `pub mod license;` (foundation
//! change, tracked separately); these files are new-files-only for this
//! issue.

pub mod permissions;
