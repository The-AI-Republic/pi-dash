//! api-v1 cycles + modules domain surface, services layer (D-20, stage 5).
//!
//! Ports `apps/api/pi_dash/api/serializers/` for the services layer:
//!
//! * [`cycle_shapes`] — the `CycleCreateSerializer.validate` flow
//!   (`cycle.py:61-106`, PIDASHCONV-283) composed over caller-supplied
//!   facts: project-id resolution, project gates, date ordering, the
//!   `convert_to_utc` rewrite and the `owned_by` default. Pure shapes and
//!   rules live in the types sibling
//!   (`pidash_types::v1_cycles_modules::cycle_shapes`); this module owns
//!   the orchestration the queries layer calls with DB-loaded facts.
//! * `module_shapes` — owned by the sibling issue PIDASHCONV-285; declared
//!   there.
//!
//! Wiring note: the crate root declares `pub mod v1_cycles_modules;` (seam
//! for this issue's new files); every file under this module is new.
//! Sibling issues add their own `*_shapes` siblings to this `mod.rs`; on
//! rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod cycle_shapes;
pub mod module_shapes;
