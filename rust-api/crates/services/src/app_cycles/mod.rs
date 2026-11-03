//! App cycles domain surface (D-27, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/cycle.py` for the services
//! layer, bottom-up:
//!
//! * [`shape`] — field-name consts in DRF wire order, the
//!   `CycleWriteSerializer.validate` date-ordering rule, the
//!   `convert_to_utc` rewrite boundary, DRF datetime rendering, and the
//!   byte-identical `ValidationError` body.
//!
//! Wiring note: the crate root declares `pub mod app_cycles;` (seam for
//! this issue's new files); every file under this module is new.
//!
//! Fixture input: F-C27-01 (`rust-api/fixtures/app_cycles/`
//! `serializers.golden.json` + `TRACE.md`); the golden is the Done-when
//! oracle for this layer.
//!
//! Pages read: Porting guide `4496e321-dd24-40f7-bfdf-f771e45fac0c`
//! (updated_at 2026-09-28T03:51:35.921141Z); PIDASHCONV-1 rulebook
//! (updated_at 2026-09-29T18:38:16.331901Z); PIDASHCONV-85 oracle Done.

pub mod queries;
pub mod shape;
