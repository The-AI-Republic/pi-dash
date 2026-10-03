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
//! [`handlers_lists`] ports the read-only workspace list family —
//! labels, states, estimates, modules, cycles (PIDASHCONV-621);
//! [`routes`] merges its routes — sibling handler issues extend the
//! merge; merges keep both sides.

pub mod gates;
pub mod handlers_lists;

use axum::Router;

use crate::state::AppState;

/// Merge the app-workspace route groups (lists family first,
/// PIDASHCONV-621; sibling handler issues extend the merge; merges keep
/// both sides). Cutover into the serving router stays with the domain
/// gate (PIDASHCONV-625), so this is additive only.
pub fn routes() -> Router<AppState> {
    handlers_lists::routes()
}
