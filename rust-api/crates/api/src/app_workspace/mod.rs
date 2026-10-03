#![forbid(unsafe_code)]

//! App workspace guard seam (D-24, stage 5).
//!
//! [`gates`] ports the `@allow_permission` role matrix, the workspace
//! permission classes, the throttle wiring, and the cache/header
//! decorators (F-W24-13, PIDASHCONV-613) over the F-06 kernel for the
//! D-24 handler issues (PIDASHCONV-615…624) to decide through.
//!
//! Route registration and handlers belong to those handler issues:
//! each owns one `handlers_*.rs` module exposing `routes()`, merged
//! below (sibling issues extend the merge; merges keep both sides).

pub mod gates;
pub mod handlers_prefs;
pub mod handlers_tokens;
pub mod handlers_user_extras;
pub mod handlers_workspace;

use axum::Router;

use crate::state::AppState;

/// App workspace routes: each handler file exposes its own `routes()`
/// (sibling D-24 handler issues extend this merge; merges keep both
/// sides), merged here for the F-10 overlay seam.
pub fn routes() -> Router<AppState> {
    handlers_prefs::routes()
        .merge(handlers_tokens::routes())
        .merge(handlers_user_extras::routes())
        .merge(handlers_workspace::routes())
}
