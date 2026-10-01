//! D-35 app-analytics handlers (stage 5).
//!
//! [`gates`] ports the `@allow_permission` gates on all 14 D-35 routes
//! plus the `WorkSpaceAdminPermission` class on the analytic-view viewset
//! (PIDASHCONV-358). The module is pure: handlers fetch membership rows
//! through the workspace-scoped handle and decide here.
//!
//! [`handlers_a`] serves routes 1-3 (`AnalyticsEndpoint.get` plus the
//! analytic-view viewset list/create/retrieve/partial_update/destroy,
//! PIDASHCONV-389); [`render`] holds the DRF plot/extras parity kernels
//! plus the workspace advance-analytics HTTP shell (PIDASHCONV-414), the
//! handlers-B shell (PIDASHCONV-399): the four owned routes
//! `saved-analytic-view`, `export-analytics`, `default-analytics` and
//! `project-stats` — and the handlers-D shell (PIDASHCONV-424): the three
//! project `advance-analytics*/` GET routes.
//! Sibling handler issues extend [`routes`]; merges keep both sides.

pub mod gates;
pub mod handlers_a;
pub mod render;

use axum::Router;

use crate::state::AppState;

/// Merge the app-analytics route groups (handlers-A, PIDASHCONV-389, plus
/// workspace advance, PIDASHCONV-414, then handlers-B, PIDASHCONV-399,
/// then project advance, PIDASHCONV-424; sibling handler issues extend
/// the merge; merges keep both sides). Cutover into the serving router
/// stays with the domain gate (PIDASHCONV-440), so this is additive only.
pub fn routes() -> Router<AppState> {
    handlers_a::routes().merge(render::routes())
}
