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
//! - [`dispatch`]: AgentRun read shapes + status/trigger enums (D-11).
//! - [`app_analytics`]: analytic-view / exporter-history / importer columns (D-35).
//! - [`app_assets`]: file-asset columns + helpers (D-31).
//! - [`app_pages`]: page / page-log / page-label / project-page / page-version columns (D-30).
//! - [`filter`]: dynamic JSON filters compiled to sea-query conditions.
//! - [`filterset`]: the `IssueFilterSet` declaration and leaf compiler (F-07).
//! - [`issue_filters`]: the legacy `issue_filters` query-param compiler (F-07).
//! - [`soft_delete`]: soft-delete read scope, write statements, view DDL.
//! - [`runner_enroll`]: runner enrollment/auth/machine columns + pure model methods (D-13).
//! - [`space`]: space public API (D-02) read columns + manager scopes.
//! - [`tx`]: transaction wrapper with post-commit actions.
//! - [`migrations`]: private migration directory convention (F-10).
//! - [`v1_assets`]: api-v1 file-asset / sticky / intake models (D-21).
//! - [`v1_cli_auth`]: api-v1 CLI auth + runner v1 Runner read model (D-22).
//! - [`v1_cycles_modules`]: api-v1 cycles + modules models (D-20).
//! - [`v1_projects`]: api-v1 projects/members/states/estimates models (D-19).
//! - [`runner_runs`]: runner run/chat/dedupe/live-state models (D-15).
//!
//! Write paths take a [`context::RequestContext`]; there is no unscoped
//! handle for writes.

pub mod app_analytics;
pub mod app_assets;
pub mod app_cycles;
pub mod app_intake;
pub mod app_integrations;
pub mod app_issues;
pub mod app_pages;
pub mod app_project;
pub mod app_views_search;
pub mod assistant;
pub mod auth_oauth;
pub mod config;
pub mod context;
pub mod dispatch;
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
pub mod runner_enroll;
pub mod runner_runs;
pub mod soft_delete;
pub mod space;
pub mod tasks_cleanup;
pub mod tasks_ticker;
pub mod tx;
pub mod v1_assets;
pub mod v1_cli_auth;
pub mod v1_cycles_modules;
pub mod v1_projects;

pub use config::DbConfig;
pub use context::RequestContext;
pub use pool::{is_write_method, replica_scope, route_for, should_use_read_replica, Pools, Route};
