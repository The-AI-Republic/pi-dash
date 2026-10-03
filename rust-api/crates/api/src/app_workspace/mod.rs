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
//! [`handlers_profile`] ports the user-profile handler family —
//! profile, user issues, stats, user activity, me activities
//! (PIDASHCONV-618); [`routes`] merges its routes — sibling handler
//! issues extend the merge; merges keep both sides.

pub mod gates;
pub mod handlers_prefs;
pub mod handlers_profile;
pub mod handlers_user_extras;

use axum::Router;

use crate::state::AppState;

/// App workspace routes: each handler file exposes its own `routes()`
/// (sibling D-24 handler issues extend this merge; merges keep both
/// sides), merged here for the F-10 overlay seam.
pub fn routes() -> Router<AppState> {
    handlers_prefs::routes()
        .merge(handlers_user_extras::routes())
        .merge(handlers_profile::routes())
}
