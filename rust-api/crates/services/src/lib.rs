#![forbid(unsafe_code)]

//! Per-domain business logic.
//!
//! Each domain from the inventory gets a module here (`src/<domain>/`),
//! depending only on `types`, `db` and `auth`. The HTTP layer in
//! `pidash-api` calls into these functions; it holds no logic of its own.

pub mod app_analytics;
pub mod app_assets;
pub mod app_cycles;
pub mod app_intake;
pub mod app_integrations;
pub mod app_issues;
pub mod app_modules;
pub mod app_notifications;
pub mod app_pages;
pub mod app_views_search;
pub mod assistant;
pub mod auth_oauth;
pub mod auth_session;
pub mod dispatch;
pub mod extensions;
pub mod health;
pub mod integrations;
pub mod license;
pub mod r#loop;
pub mod prompting;
pub mod scheduler;
pub mod space;
pub mod tasks_cleanup;
pub mod tasks_webhooks;
pub mod user_settings;
pub mod v1_cycles_modules;
pub mod v1_openapi;
pub mod v1_projects;

pub use health::{db_summary, health_report};
