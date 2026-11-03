//! License / instance-console job units (D-01, stage 3).
//!
//! Ports the task plane for `apps/api/pi_dash/license/`:
//!
//! * [`tasks`] — `bgtasks/tracer.py:26-105` (`instance_traces`).
//! * [`commands`] — `management/commands/configure_instance.py:20-170` and
//!   `management/commands/register_instance.py:21-92`.
//!
//! The logic is pure over small traits so the fixture vectors replay without
//! a database; Postgres-backed implementations of those traits live next to
//! the logic for the worker to call once this module is wired in.
//!
//! Wiring note: the crate root declares `pub mod license;` (foundation
//! change, tracked separately); these files are new-files-only for this
//! issue.

pub mod commands;
pub mod tasks;
