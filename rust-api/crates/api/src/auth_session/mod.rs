//! D-16 authentication HTTP rendering (stage 5, PIDASHCONV-340).
//!
//! * [`render`] — JSON 400 bodies, 302 redirect locations, and the
//!   `csrf_failure` page over the `pidash_services::auth_session`
//!   kernel.
//!
//! Wiring note: the crate root declares `pub mod auth_session;`
//! (seam for this issue's new files); every file under this module
//! is new. Sibling D-16 handler issues register routes here and call
//! [`render`] from them.
pub mod render;

pub use render::{
    html_escape, json_error_body, json_error_string, redirect_location, render_csrf_failure,
    CSRF_FAILURE_TEMPLATE, JSON_400_STATUS, REDIRECT_302_STATUS, THROTTLE_429_STATUS,
    UNAUTHENTICATED_401_STATUS,
};
