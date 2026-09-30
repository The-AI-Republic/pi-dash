//! App asset surface (D-31): S3/MinIO presigned flows.
//!
//! [`guards`] ports the permission gates and the asset throttle key
//! (`apps/api/pi_dash/app/views/asset/v2.py` gate lines,
//! `apps/api/pi_dash/app/views/asset/base.py` default auth,
//! `apps/api/pi_dash/throttles/asset.py`). [`handlers_v1`] serves the
//! legacy v1 routes (PIDASHCONV-394); sibling handler issues 400/412
//! extend [`routes`] with their own routers, keeping both sides.

pub mod guards;
pub mod handlers_v1;

use axum::Router;

use crate::state::AppState;

/// Owned D-31 app-asset routes (cutover granularity: registered paths
/// serve from Rust, everything else keeps proxying).
pub fn routes() -> Router<AppState> {
    handlers_v1::routes()
}
