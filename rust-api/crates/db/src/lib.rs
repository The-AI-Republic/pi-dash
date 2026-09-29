#![forbid(unsafe_code)]

//! Database kernel for the Pi Dash Rust backend.
//!
//! The data layer every port builds on:
//!
//! - [`config`]: connection configuration (F-01 scaffold) plus the
//!   centralized config mechanism (F-03: `registry`, `accessor`,
//!   `encryption`, `legacy`, `settings`).
//! - [`pool`]: primary + replica sqlx pools with Django-equivalent routing.
//! - [`context`]: explicit per-request audit/tenant context.
//! - [`app_assets`]: file-asset columns + helpers (D-31).
//! - [`filter`]: dynamic JSON filters compiled to sea-query conditions.
//! - [`filterset`]: the `IssueFilterSet` declaration and leaf compiler (F-07).
//! - [`issue_filters`]: the legacy `issue_filters` query-param compiler (F-07).
//! - [`soft_delete`]: soft-delete read scope, write statements, view DDL.
//! - [`space`]: space public API (D-02) read columns + manager scopes.
//! - [`tx`]: transaction wrapper with post-commit actions.
//! - [`migrations`]: private migration directory convention (F-10).
//!
//! Write paths take a [`context::RequestContext`]; there is no unscoped
//! handle for writes.

pub mod app_assets;
pub mod app_cycles;
pub mod app_intake;
pub mod app_integrations;
pub mod app_views_search;
pub mod assistant;
pub mod config;
pub mod context;
pub mod filter;
pub mod filterset;
pub mod integrations;
pub mod issue_filters;
pub mod license;
pub mod r#loop;
pub mod migrations;
pub mod pool;
pub mod prompting;
pub mod redis;
pub mod soft_delete;
pub mod space;
pub mod tasks_cleanup;
pub mod tasks_ticker;
pub mod tx;

pub use config::DbConfig;
pub use context::RequestContext;
pub use pool::{is_write_method, replica_scope, route_for, should_use_read_replica, Pools, Route};
