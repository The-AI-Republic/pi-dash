//! Runner enrollment + auth + machines domain surface (D-13, stage 5).
//!
//! Ports `apps/api/pi_dash/runner/services/` for the services layer:
//!
//! * [`tokens`] — token minting + key ring + access-JWT mint (`tokens.py`).
//! * [`pod_naming`] — pod-name validation (`pod_naming.py`).
//!
//! Wiring note: the crate root declares `pub mod runner_enroll;` (seam for
//! this issue's new files); sibling D-13 issues add their own files here.
pub mod pod_naming;
pub mod tokens;
