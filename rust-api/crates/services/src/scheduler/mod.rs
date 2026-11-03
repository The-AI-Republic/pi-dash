//! Scheduler seeding surface (D-04, stage 4).
//!
//! Ports `apps/api/pi_dash/scheduler/builtins/__init__.py` and
//! `apps/api/pi_dash/scheduler/signals.py` for the services layer:
//!
//! * [`builtins`] — builtin catalog + per-workspace upsert
//!   (`ensure_builtin_schedulers`) + creation-signal gate.
//!
//! Wiring note: the crate root declares `pub mod scheduler;` (seam for
//! this issue's new files); every file under this module is new.
pub mod builtins;
