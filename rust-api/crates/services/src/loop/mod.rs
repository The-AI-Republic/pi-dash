#![forbid(unsafe_code)]

//! Loop auto-pm serializer shapes (D-03).
//!
//! Port of the Django-free payload builders:
//!
//! * `loop/serializers.py:16-32` ([`shape::interval_label`])
//! * `loop/serializers.py:35-43` ([`shape::public_job_payload`])
//! * `loop/admin_views.py:43-59` ([`shape::job_payload`])
//! * `loop/builtins.py:17-59` ([`builtins`])
//!
//! These are pure output shapes: each builder takes a row borrowed from the
//! caller and returns a [`serde_json::Value`]. Datetimes cross this boundary
//! already rendered as DRF `isoformat` strings (`+00:00` offsets preserved,
//! microseconds when nonzero) — rendering owns to the DB edge at the future
//! handlers layer, so formatting here is a byte-exact passthrough and
//! `None` renders `null` exactly as `x.isoformat() if x else None` does.

pub mod builtins;
pub mod shape;

pub use builtins::{BuiltinLoopJob, AUTO_CLOSE_MERGED_PROMPT, BUILTIN_LOOP_JOBS};
pub use shape::{interval_label, job_payload, public_job_payload, AdminJobRow, PublicJobRow};
