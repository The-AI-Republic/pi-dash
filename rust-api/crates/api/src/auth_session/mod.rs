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
//!
//! Plus the guard wiring named by PIDASHCONV-393:
//!
//! * [`middleware`] — D-16 session-cookie semantics + tower wiring over
//!   the F-08 session layer (reused read-only).
pub mod email;
// Plus the magic-link handler port, named by PIDASHCONV-431: [`magic`]
// (six routes: generate/sign-in/sign-up, app + space) wired through
// [`routes`]; merges keep both sides.
pub mod magic;
pub mod middleware;
pub mod multipart;
pub mod password;
pub mod render;

pub use magic::{
    routes as magic_routes, APP_GENERATE_PATH, APP_SIGN_IN_PATH, APP_SIGN_UP_PATH,
    SPACE_GENERATE_PATH, SPACE_SIGN_IN_PATH, SPACE_SIGN_UP_PATH,
};
pub use middleware::{
    auth_cookie_name_for_path, auth_session_layer, ADMIN_SESSION_COOKIE_NAME, SESSION_COOKIE_NAME,
};
pub use render::{
    disallowed_redirect, html_escape, json_error_body, json_error_string, redirect_location,
    render_csrf_failure, CSRF_FAILURE_TEMPLATE, JSON_400_STATUS, REDIRECT_302_STATUS,
    THROTTLE_429_STATUS, UNAUTHENTICATED_401_STATUS,
};

/// DRF `AnonRateThrottle` ident (`throttling.py` `get_ident`,
/// `NUM_PROXIES` unset): the `X-Forwarded-For` value with all
/// whitespace stripped when present, else `REMOTE_ADDR` (which may
/// itself be absent → the `"None"` ident Python's `%`-formatting
/// produces). Shared by the magic and email-check throttles; the
/// first-entry `client_ip` helper stays reserved for the stamp
/// columns (`utils/ip_address.py:get_client_ip`).
pub(crate) fn throttle_ident(headers: &axum::http::HeaderMap, peer: Option<String>) -> String {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if !xff.is_empty() {
            return xff.split_whitespace().collect();
        }
    }
    peer.unwrap_or_else(|| "None".to_owned())
}

/// Owned D-16 auth routes (cutover granularity: registered paths
/// serve Rust, sibling paths keep proxying through the fallback).
/// Email sessions (PIDASHCONV-422) plus the magic-link family
/// (PIDASHCONV-431) plus the password/CSRF closure (PIDASHCONV-434);
/// merges keep both sides.
pub fn routes() -> axum::Router<crate::state::AppState> {
    email::routes()
        .merge(magic_routes())
        .merge(password::password_routes())
}
