//! D-35 app-analytics handlers (stage 5).
//!
//! [`gates`] ports the `@allow_permission` gates on all 14 D-35 routes
//! plus the `WorkSpaceAdminPermission` class on the analytic-view viewset
//! (PIDASHCONV-358). The module is pure: handlers fetch membership rows
//! through the workspace-scoped handle and decide here.
//!
//! [`render`] ports the workspace advance-analytics HTTP shell
//! (PIDASHCONV-414): the three `advance-analytics*/` GET routes with their
//! `get_analytics_filters` scoping, counts, stats, charts and the
//! `build_analytics_chart` equivalent — plus the handlers-B shell
//! (PIDASHCONV-399): the four owned routes `saved-analytic-view`,
//! `export-analytics`, `default-analytics` and `project-stats`.
//! Sibling handler issues extend [`routes`]; merges keep both sides.

pub mod gates;
pub mod render;

use axum::Router;

use crate::state::AppState;

/// Merge the app-analytics route groups (workspace advance first,
/// PIDASHCONV-414, then handlers-B, PIDASHCONV-399; sibling handler
/// issues extend the merge; merges keep both sides). Cutover into the
/// serving router stays with the domain gate (PIDASHCONV-440), so this
/// is additive only.
pub fn routes() -> Router<AppState> {
    render::routes()
}
