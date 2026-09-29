//! App intake domain surface (D-32, stage 5).
//!
//! Ports `apps/api/pi_dash/app/views/intake/base.py` for the services
//! layer, bottom-up:
//!
//! * [`permissions`] — role matrices, guest scoping, creator gates,
//!   default-intake delete guard and destroy cascade (the guard units).
//!
//! Wiring note: the crate root declares `pub mod app_intake;` (seam for
//! this issue's new files); every file under this module is new.
pub mod permissions;
