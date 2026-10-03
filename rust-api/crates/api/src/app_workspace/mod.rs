#![forbid(unsafe_code)]

//! App workspace guard seam (D-24, stage 5).
//!
//! [`gates`] ports the `@allow_permission` role matrix, the workspace
//! permission classes, the throttle wiring, and the cache/header
//! decorators (F-W24-13, PIDASHCONV-613) over the F-06 kernel for the
//! D-24 handler issues (PIDASHCONV-615…624) to decide through.
//!
//! Route registration and handlers belong to those handler issues;
//! this module only declares the guard layer they wire through, not a
//! stub — sibling issues extend it; merges keep both sides.
//!
//! [`handlers_members`] serves the member family (list/retrieve,
//! partial_update/destroy/leave, views-post, me-get, project-members,
//! last-visited — PIDASHCONV-616).

pub mod gates;
pub mod handlers_members;

use axum::Router;

use crate::state::AppState;

/// Merge the app-workspace route groups (members, PIDASHCONV-616;
/// sibling handler issues extend the merge; merges keep both sides).
/// Cutover into the serving router stays with the domain gate
/// (PIDASHCONV-625), so this is additive only.
pub fn routes() -> Router<AppState> {
    handlers_members::routes()
}
