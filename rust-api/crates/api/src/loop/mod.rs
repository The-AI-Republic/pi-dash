#![forbid(unsafe_code)]

//! Loop auto-pm domain module (D-03).
//!
//! [`guards`] ports the toggle-only body guard, the admin write validator,
//! the slug-taken predicates, and the `InstanceAdminPermission` wiring
//! reference (`pi_dash/loop/views.py`, `pi_dash/loop/admin_views.py`,
//! `pi_dash/license/api/permissions/instance.py`). [`admin`] owns the
//! instance-admin HTTP shell: the `/api/instances/loop/` routes with
//! the session auth + permission gate, the write SQL, and the response
//! rendering (PIDASHCONV-161). [`user`] owns the user-surface handlers
//! (PIDASHCONV-159): the settings GET + PATCH and the per-job PATCH with
//! their upsert SQL and HTTP mapping. [`routes`] merges both surfaces;
//! every other method on those paths proxies to Django through the edge
//! fallback.

pub mod admin;
pub mod guards;
pub mod user;

use axum::Router;

use crate::state::AppState;

/// Owned D-03 loop routes (cutover granularity: registered paths serve
/// from Rust, everything else keeps proxying).
pub fn routes() -> Router<AppState> {
    admin::routes().merge(user::routes())
}
