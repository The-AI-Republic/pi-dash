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
use crate::v1_cycles_modules::body::BodySpec;

/// The loop write bodies are flat scalar maps. DRF hands the views a
/// `QueryDict` for form input (last value per key, blank values kept) and
/// the handlers treat any non-dict as `{}`; no key is a list and no scalar
/// is skipped when present-but-blank (`_validate_writes` / `_read_enabled`
/// inspect the keys themselves).
pub const LOOP_BODY_SPEC: BodySpec = BodySpec {
    list_fields: &[],
    skip_blank_fields: &[],
};

/// Owned D-03 loop routes (cutover granularity: registered paths serve
/// from Rust, everything else keeps proxying).
pub fn routes() -> Router<AppState> {
    admin::routes().merge(user::routes())
}
