//! api-v1 assets / stickies / intake domain surface (D-21, stage 5).
//!
//! Ports `apps/api/pi_dash/api/serializers/` for the types layer:
//!
//! * [`intake`] — `intake.py:12-170` (PIDASHCONV-401; fixture `fx-ser-intake`).
//! * `asset.rs`, `sticky.rs` — owned by the sibling issue PIDASHCONV-392
//!   (fixtures `fx-ser-asset`, `fx-ser-sticky`); declared there.

pub mod intake;
