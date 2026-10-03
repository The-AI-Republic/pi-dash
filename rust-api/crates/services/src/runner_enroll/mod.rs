//! Runner enrollment + auth + machines domain surface (D-13, stage 5).
//!
//! Ports `apps/api/pi_dash/runner/` for the services layer, bottom-up:
//!
//! * [`tokens`] — token minting + key ring + access-JWT mint (`services/tokens.py`).
//! * [`pod_naming`] — pod-name validation (`services/pod_naming.py`).
//! * [`serializers`] — output shapes + request validation
//!   (`runner/serializers.py`, PIDASHCONV-579).
//! * [`queries`] — daemon-enrollment read/write statement sets
//!   (`runner/views/enrollment.py`, PIDASHCONV-582) plus pods +
//!   projects + desktop reads (`runner/views/pods.py`,
//!   `projects.py`, `desktop.py`, PIDASHCONV-584).
//!
//! Wiring note: the crate root declares `pub mod runner_enroll;` (seam for
//! this issue's new files); sibling D-13 issues add their own files here.
//! `mod.rs` edits stay additive-only.
pub mod pod_naming;
pub mod queries;
pub mod serializers;
pub mod tokens;
