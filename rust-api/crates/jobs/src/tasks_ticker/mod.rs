//! Agent ticker + scheduler task helpers (`tasks_ticker` domain layer, D-10).
//!
//! This module hosts the pure helpers every ticker/scheduler task calls
//! into. It is the single owner of the `_rrule` port (PIDASHCONV-205):
//! D-10 task layers (`scan`, `fire_tick`, `fire_binding`) and the D-03
//! loop workers all call [`rrule::next_fire_from_rrule`] here rather than
//! re-porting expansion.

pub mod fire_tick;
pub mod rrule;
pub mod scan;
