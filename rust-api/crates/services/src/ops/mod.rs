#![forbid(unsafe_code)]

//! Ops tail (D-37, stage 7): management-command backings, seed data +
//! loader, replica routing. One module per issue; command groups stay in
//! the order boot, users, repair, instance, prompting so sibling rebases
//! stay mechanical.
//!
//! * [`users`] — users + membership commands (PIDASHCONV-807,
//!   fixtures F37-03/F37-04).
//! * [`repair`] — data-repair command decisions (PIDASHCONV-808).
//! * [`seeds`] — the workspace seed data + loader (PIDASHCONV-811,
//!   fixture F37-10).
//! * [`prompting`] — reseed + revalidate decisions over the prompting
//!   kernels (PIDASHCONV-810, fixture F37-09).

pub mod prompting;
pub mod repair;
pub mod seeds;
pub mod users;
