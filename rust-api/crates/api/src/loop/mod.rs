#![forbid(unsafe_code)]

//! Loop auto-pm domain module (D-03).
//!
//! [`guards`] ports the toggle-only body guard, the admin write validator,
//! the slug-taken predicates, and the `InstanceAdminPermission` wiring
//! reference (`pi_dash/loop/views.py`, `pi_dash/loop/admin_views.py`,
//! `pi_dash/license/api/permissions/instance.py`). [`admin`] owns the
//! instance-admin HTTP shell: the three `/api/instances/loop/` routes,
//! the session auth + permission gate, the write SQL, and the response
//! rendering (PIDASHCONV-161). The user-surface handlers land with
//! PIDASHCONV-159 and merge into [`routes`] beside it.

pub mod admin;
pub mod guards;

use axum::Router;

use crate::state::AppState;

/// Owned D-03 loop routes (cutover granularity: registered paths serve
/// from Rust, everything else keeps proxying).
pub fn routes() -> Router<AppState> {
    admin::routes()
}
