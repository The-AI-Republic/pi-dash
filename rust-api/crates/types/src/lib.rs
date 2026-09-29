#![forbid(unsafe_code)]

//! Shared value types for the Pi Dash Rust backend.
//!
//! This crate holds identifiers, enums, DTOs and the error type. It performs
//! no I/O: no database, no network, no filesystem.

pub mod assistant;
pub mod error;
pub mod health;
pub mod ids;
pub mod integrations;
pub mod license;
pub mod tasks_cleanup;

pub use error::Error;
pub use health::HealthStatus;
pub use ids::{IssueId, ProjectId, UserId, WorkspaceId};
