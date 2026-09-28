//! Loop auto-PM domain surface (D-03, stage 4).
//!
//! Ports `apps/api/pi_dash/db/models/loop.py` for the db layer:
//!
//! * [`models`] — `SkipReason`, `LoopJob`, `LoopTarget`,
//!   `LoopUserPreference` (PIDASHCONV-153, struct + column/constraint
//!   mapping only). Eligibility reads, settings payloads, and admin
//!   queries belong to PIDASHCONV-154; serializers live in
//!   `pidash-services::loop` (PIDASHCONV-152).

pub mod models;

pub use models::{
    loop_job::LoopJob, loop_target::LoopTarget, loop_user_preference::LoopUserPreference, OnDelete,
    SkipReason,
};
