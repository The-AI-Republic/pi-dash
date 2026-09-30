//! D-35 app-analytics permission layer (stage 5, PIDASHCONV-358).
//!
//! [`gates`] ports the `@allow_permission` gates on all 14 D-35 routes
//! plus the `WorkSpaceAdminPermission` class on the analytic-view viewset.
//! The module is pure: handlers fetch membership rows through the
//! workspace-scoped handle and decide here. Sibling handler issues own
//! the HTTP shell; merges keep both sides.
//!
//! [`export`] serves the exporter route (`GET` + `POST`
//! `workspaces/<slug>/export-issues/`, PIDASHCONV-430); sibling handler
//! issues extend the merge below, keeping both sides.

pub mod export;
pub mod gates;

/// D-35 routes owned by Rust. Registration is the cutover granularity:
/// sibling paths have no Rust route and keep proxying to Django through
/// the fallback, so no per-path flag is needed.
pub fn routes() -> axum::Router<crate::state::AppState> {
    export::routes()
}
