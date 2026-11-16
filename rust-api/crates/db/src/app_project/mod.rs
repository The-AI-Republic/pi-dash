#![forbid(unsafe_code)]

//! App project/states/estimates domain surface (D-25, stage 5).
//!
//! Ports `apps/api/pi_dash/db/models/{project,state,estimate}.py` and
//! `db/models/deploy_board.py` for the db layer.
//!
//! * [`models`] — column lists, row structs, enums, defaults, constraints,
//!   manager scopes, and save-rule helpers (PIDASHCONV-567).
//!
//! Wiring note: the crate root declares `pub mod app_project;` (seam added
//! by PIDASHCONV-567); every file under this module is new. Sibling issues
//! add their own layers under sibling modules (e.g. PIDASHCONV-568
//! `queries`); on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod models;
