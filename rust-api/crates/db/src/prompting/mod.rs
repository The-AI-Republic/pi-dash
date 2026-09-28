//! Prompting domain surface (D-04, stage 4).
//!
//! Ports `apps/api/pi_dash/prompting/` for the db layer, bottom-up:
//!
//! * [`models`] — table shapes (`models.py`: `PromptTemplate`,
//!   `PromptSectionOverride`).
//!
//! Wiring note: the crate root declares `pub mod prompting;` (seam for
//! this issue's new files); every file under this module is new.
pub mod models;
