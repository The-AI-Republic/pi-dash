//! api-v1 cycles + modules permission guards (D-20, stage 5).
//!
//! Ports the permission wiring of
//! `apps/api/pi_dash/api/views/{cycle,module}.py` for the api layer:
//!
//! * [`gates`] — `ProjectEntityPermission` as a route→gate table plus thin
//!   decision wrappers over the read-only F-06 kernel, and the shared
//!   `BaseAPIView.handle_exception` error envelopes (PIDASHCONV-309).
//!
//! Wiring note: the crate root declares `pub mod v1_cycles_modules;` (seam
//! for this issue's new files); every file under this module is new.
//! Sibling issues add their own handler modules here (PIDASHCONV-362 cycle
//! endpoints, PIDASHCONV-406 module endpoints) and own the router plus the
//! overlay wiring; on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod gates;
pub mod module;

use axum::Router;

use crate::state::AppState;

/// Domain router: module routes in [`module::routes`] (PIDASHCONV-406);
/// PIDASHCONV-362 merges `cycle::routes()` here for the cycle endpoints.
/// Owned D-20 api-v1 routes serve from Rust (cutover granularity);
/// everything else keeps proxying to Django through the edge fallback.
/// Sibling D-20 handler issues merge their routers here; merges keep both
/// sides.
pub fn routes() -> Router<AppState> {
    module::routes()
}
