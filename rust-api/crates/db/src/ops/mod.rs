//! D-37 ops command groups (stage 7, PIDASHCONV-74).
//!
//! One module per command issue; groups stay in the order boot, users,
//! repair, instance, prompting so sibling rebases stay mechanical.

pub mod repair;
