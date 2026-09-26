#![forbid(unsafe_code)]

//! Per-domain business logic.
//!
//! Each domain from the inventory gets a module here (`src/<domain>/`),
//! depending only on `types`, `db` and `auth`. The HTTP layer in
//! `pidash-api` calls into these functions; it holds no logic of its own.

pub mod extensions;
pub mod health;
pub mod user_settings;

pub use health::{db_summary, health_report};
