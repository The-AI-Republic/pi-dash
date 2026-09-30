//! D-28 app-modules permission layer (stage 5, PIDASHCONV-379).
//!
//! [`gates`] ports the guard closure on all 13 D-28 routes: the
//! `@allow_permission` role sets, the `ProjectEntityPermission` vs
//! `ProjectLitePermission` branch mapping, and the per-user scoping notes.
//! The module is pure: handlers fetch membership rows through the
//! workspace-scoped handle and decide here. Sibling handler issues own
//! the HTTP shell; merges keep both sides.
//!
//! [`handlers_modules`] ports `ModuleViewSet` CRUD (PIDASHCONV-391);
//! [`routes`] merges its routes — sibling handler issues (397, 407)
//! extend the merge; merges keep both sides.

pub mod gates;
pub mod handlers_modules;

use axum::Router;

use crate::state::AppState;

/// Merge the app-modules route groups (ModuleViewSet CRUD first,
/// PIDASHCONV-391; sibling handler issues extend the merge; merges keep
/// both sides). Cutover into the serving router stays with the domain
/// gate (PIDASHCONV-416), so this is additive only.
pub fn routes() -> Router<AppState> {
    handlers_modules::routes()
}
