//! D-36 scheduler permission gates + feature-flag guard (stage 5, PIDASHCONV-632).
//!
//! Ports the `@allow_permission` gates on the 5 scheduler routes
//! (`apps/api/pi_dash/app/urls/scheduler.py:16-46`) and the
//! `_feature_enabled` kill switch
//! (`apps/api/pi_dash/app/views/scheduler/views.py:32-40`). Fixture:
//! `rust-api/fixtures/app_scheduler/guards/permissions.golden.json`
//! (F36-09, trace: `rust-api/fixtures/app_scheduler/TRACE.md`).
//!
//! Shape of the port, following the [`crate::app_pages`] `gate.rs`
//! precedent: the decision kernel lives in the read-only F-06
//! foundation (`pidash_auth::permissions::allow::{decide_allow, ...}`);
//! [`gate`] pins which gate each route carries, adds the async tenant
//! row fetching (handlers call into it — no handler takes an unscoped
//! database handle for these routes), and owns the byte-exact denial
//! bodies.
//!
//! The handler shells (`handlers_sched.rs`, `handlers_bind.rs`,
//! `handlers_occ.rs`, PIDASHCONV-633…635) land in this module and call
//! [`gate::resolve_gate`], then [`gate::ensure_feature_enabled`] on the
//! 4 CRUD routes (never on occurrences — ported quirk, see
//! [`gate::DISABLED_BODY`).

pub mod gate;
pub mod handlers_sched;

use axum::Router;

use crate::state::AppState;

/// Scheduler routes: each handler file exposes its own `routes()`
/// (sibling D-36 handler issues extend this merge; merges keep both
/// sides), merged here for the F-10 overlay seam.
pub fn routes() -> Router<AppState> {
    handlers_sched::routes()
}
