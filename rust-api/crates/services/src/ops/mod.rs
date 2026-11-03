#![forbid(unsafe_code)]

//! Ops tail (D-37, stage 7): management-command backings, seed data
//! loader, replica routing. One module per issue; command groups stay in
//! the order boot, users, repair, instance, prompting so sibling rebases
//! stay mechanical.
//!
//! * [`boot`] — `wait_for_db`, `wait_for_migrations`, `clear_cache`
//!   (PIDASHCONV-806, fixtures F37-01/F37-02).
//! * [`storage`] — `create_bucket`, `update_bucket` S3 flows
//!   (PIDASHCONV-806).
//! * [`users`] — users + membership commands (PIDASHCONV-807,
//!   fixtures F37-03/F37-04).
//! * [`repair`] — data-repair command decisions (PIDASHCONV-808).
//! * [`instance`] — pure output shaping for the `instance` command group
//!   (PIDASHCONV-809); no I/O there, the binary wires stores, SMTP, AMQP
//!   and the GitHub probe around those shapes.
//! * [`seeds`] — the workspace seed data + loader (PIDASHCONV-811,
//!   fixture F37-10).
//! * [`prompting`] — reseed + revalidate decisions over the prompting
//!   kernels (PIDASHCONV-810, fixture F37-09).

pub mod boot;
pub mod instance;
pub mod prompting;
pub mod repair;
pub mod seeds;
pub mod storage;
pub mod users;
