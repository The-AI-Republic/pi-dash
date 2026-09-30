//! api-v1 projects/members/states/estimates permission guards (D-19, stage 5).
//!
//! Ports `apps/api/pi_dash/api/views/{project,member,invite,user,state,
//! estimate}.py` permission wiring for the api layer:
//!
//! * [`perms`] — the 7 guard classes as a route→gate table plus thin
//!   decision wrappers over the read-only F-06 kernel (PIDASHCONV-367).
//!
//! Wiring note: the crate root declares `pub mod v1_projects;` (seam for
//! this issue's new files); every file under this module is new. Sibling
//! issues add their own handler modules here and merge their routers into
//! [`routes`]; on rebase keep both sides.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod handlers_members;
pub mod handlers_state_estimate;
pub mod perms;

use axum::Router;

use crate::state::AppState;

/// Domain router: member / invite / user routes (PIDASHCONV-371) plus
/// state / estimate routes (PIDASHCONV-372). Owned D-19 api-v1 routes serve
/// from Rust (cutover granularity); everything else keeps proxying to Django
/// through the edge fallback. Sibling D-19 handler issues merge their routers
/// here; merges keep both sides.
pub fn routes() -> Router<AppState> {
    handlers_members::routes().merge(handlers_state_estimate::routes())
}
}
