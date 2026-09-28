//! D-01 license / instance-console handlers (stage 3).
//!
//! Ports `apps/api/pi_dash/license/api/views/` onto the merged D-01
//! foundation. Sibling handler issues own their files and consume shared
//! pieces read-only:
//!
//! * [`handlers_base`] — `views/base.py` (TimezoneMixin, `BaseAPIView`
//!   defaults, the exception matrix, `fields`/`expand`; PIDASHCONV-120).
//! * [`handlers_instance`] — `views/instance.py` (`InstanceEndpoint`
//!   GET/PATCH, `SignUpScreenVisitedEndpoint` POST; PIDASHCONV-120).
//! * [`handlers_admin`] — admins CRUD + me/session/sign-out
//!   (PIDASHCONV-121; `admin.py:44-86` and `admin.py:361-398`).
//! * [`handlers_auth_forms`] — admin sign-up + sign-in form-POST redirect
//!   flows (PIDASHCONV-122; `admin.py:89-358`).
//!
//! [`routes`] registers the owned instance paths; sibling handler routes
//! stay unregistered (proxying to Django) until their own issues cut over.
//! Every other method on the owned paths proxies to Django through the
//! edge fallback.

pub mod handlers_admin;
pub mod handlers_auth_forms;
pub mod handlers_base;
pub mod handlers_instance;

use axum::Router;

use crate::state::AppState;

/// Owned D-01 instance-console routes (cutover granularity: registered
/// paths serve from Rust, everything else keeps proxying).
pub fn routes() -> Router<AppState> {
    handlers_instance::routes()
}
