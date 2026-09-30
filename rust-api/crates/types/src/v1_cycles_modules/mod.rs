//! api-v1 cycles + modules domain surface, types layer (D-20, stage 5).
//!
//! Ports `apps/api/pi_dash/api/serializers/` for the types layer:
//!
//! * [`cycle_shapes`] — `cycle.py:1-206` (PIDASHCONV-283; fixture
//!   FX-CYCMOD-02).
//! * `module_shapes` — owned by the sibling issue PIDASHCONV-285 (fixture
//!   FX-CYCMOD-03); declared there.
//!
//! Wiring note: the crate root declares `pub mod v1_cycles_modules;` (seam
//! for this issue's new files); every file under this module is new.
//! Sibling issues add their own `*_shapes` siblings to this `mod.rs`; on
//! rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod cycle_shapes;
