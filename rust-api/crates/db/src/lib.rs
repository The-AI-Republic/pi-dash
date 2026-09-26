#![forbid(unsafe_code)]

//! Database kernel for the Pi Dash Rust backend.
//!
//! The data layer every port builds on (F-04):
//!
//! - [`config`]: connection configuration (F-01 scaffold).
//! - [`pool`]: primary + replica sqlx pools with Django-equivalent routing.
//! - [`context`]: explicit per-request audit/tenant context.
//! - [`filter`]: dynamic JSON filters compiled to sea-query conditions.
//! - [`soft_delete`]: soft-delete read scope, write statements, view DDL.
//! - [`tx`]: transaction wrapper with post-commit actions.
//!
//! Write paths take a [`context::RequestContext`]; there is no unscoped
//! handle for writes.

pub mod config;
pub mod context;
pub mod filter;
pub mod pool;
pub mod soft_delete;
pub mod tx;

pub use config::DbConfig;
pub use context::RequestContext;
pub use pool::{is_write_method, replica_scope, route_for, should_use_read_replica, Pools, Route};
