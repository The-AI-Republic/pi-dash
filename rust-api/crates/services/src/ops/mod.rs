#![forbid(unsafe_code)]

//! Ops tail (D-37, stage 7): management-command backings, seed data +
//! loader, replica routing. One module per issue; command groups stay in
//! the order boot, users, repair, instance, prompting so sibling rebases
//! stay mechanical.

pub mod repair;
pub mod seeds;
