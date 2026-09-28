//! Space public-API domain surface (D-02, stage 4).
//!
//! Ports `apps/api/pi_dash/space/` for the services layer, bottom-up:
//!
//! * [`serializers`] — output shapes (`serializer/`).
//!
//! Wiring note: the crate root declares `pub mod space;` (seam for this
//! issue's new files); every file under this module is new.
pub mod serializers;
