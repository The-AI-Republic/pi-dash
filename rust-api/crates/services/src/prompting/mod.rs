//! Prompting domain surface (D-04, stage 4).
//!
//! Ports `apps/api/pi_dash/prompting/` for the services layer, bottom-up:
//!
//! * [`shape`] — serializer output shapes (`serializers.py`).
//!
//! Wiring note: the crate root declares `pub mod prompting;` (seam for
//! this issue's new files); every file under this module is new.
pub mod shape;
