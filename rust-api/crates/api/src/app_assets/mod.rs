//! App asset surface (D-31): S3/MinIO presigned flows.
//!
//! [`guards`] ports the permission gates and the asset throttle key
//! (`apps/api/pi_dash/app/views/asset/v2.py` gate lines,
//! `apps/api/pi_dash/app/views/asset/base.py` default auth,
//! `apps/api/pi_dash/throttles/asset.py`). [`handlers_v1`] serves the
//! legacy v1 routes (PIDASHCONV-394);
//! [`handlers_v2_user_workspace`] (PIDASHCONV-400) serves the v2
//! user/workspace/static/restore family; sibling handler issues 412
//! extend [`routes`] with their own routers, keeping both sides.

pub mod guards;
pub mod handlers_v1;
pub mod handlers_v2_user_workspace;

use axum::Router;

use crate::state::AppState;

/// Merge the app-asset route groups (v1 first, then v2
/// user/workspace/static/restore; sibling handler issues extend the
/// merge; merges keep both sides).
pub fn routes() -> Router<AppState> {
    handlers_v1::routes().merge(handlers_v2_user_workspace::routes())
}
