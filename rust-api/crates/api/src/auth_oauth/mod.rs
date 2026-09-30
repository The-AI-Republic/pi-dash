//! D-17 authentication guards (api layer, stage 5).
//!
//! Ports the guard units of
//! `apps/api/pi_dash/authentication/views/cli/device.py` (flow constants,
//! `DeviceCodeStartThrottle`, `_verification_uri`, the six endpoint gate
//! triples) and the auth shape of the sixteen OAuth views
//! (`authentication/views/app+space/{google,github,gitlab,gitea}.py`).
//!
//! * [`guards`] — constants, throttle spec, URI builder, device matrix,
//!   OAuth view shape (PIDASHCONV-331, AUTHOAUTH-F9).
//! * [`device_flow`] — device start / approve / token-poll handlers +
//!   route registration (PIDASHCONV-342, AUTHOAUTH-F12).
//! * [`device_session`] — workspaces list, machine-token exchange, revoke
//!   handlers + routes (PIDASHCONV-343, AUTHOAUTH-F12).
//! * [`oauth_gitea`] — Gitea app + space initiate/callback handlers +
//!   routes (PIDASHCONV-341, AUTHOAUTH-F10/F11).
//! * [`oauth_github`] — GitHub app + space initiate/callback handlers +
//!   routes (PIDASHCONV-336, AUTHOAUTH-F10/F11 github rows).
//! * [`oauth_gitlab`] — GitLab app + space initiate/callback handlers +
//!   routes (PIDASHCONV-339, AUTHOAUTH-F10/F11). Sibling provider issue
//!   PIDASHCONV-335 adds its `oauth_*` module here; each provider mounts
//!   its own router under the Auth group arm.
//!
//! [`routes`] merges both ported device families; merges keep both sides.
//!
//! # Wiring note
//!
//! The crate root declares `pub mod auth_oauth;` (one-line wiring in the
//! port PR, following the PIDASHCONV-284 precedent).

pub mod device_flow;
pub mod device_session;
pub mod guards;
pub mod oauth_gitea;
pub mod oauth_github;
pub mod oauth_gitlab;

pub use guards::{
    device_endpoint_guard, device_start_cache_key, device_start_throttle_rate,
    initiate_session_keys, verification_uri, DeviceEndpointGuard, OauthProvider,
    DEVICE_CODE_MIN_POLL_GAP_SECS, DEVICE_CODE_POLL_INTERVAL_SECS, DEVICE_CODE_START_MAX_RETRIES,
    DEVICE_CODE_TTL_SECS, DEVICE_ENDPOINT_GUARDS, DEVICE_START_THROTTLE_RATE_STR,
    DEVICE_START_THROTTLE_SCOPE, OAUTH_APP_INITIATE_SESSION_KEYS, OAUTH_CALLBACK_SESSION_KEYS,
    OAUTH_SPACE_GITEA_INITIATE_SESSION_KEYS, OAUTH_SPACE_INITIATE_SESSION_KEYS, OAUTH_VIEW_BASE,
    OAUTH_VIEW_METHOD,
};

/// Routes for the ported device families (PIDASHCONV-342 + PIDASHCONV-343).
pub fn routes() -> axum::Router<crate::state::AppState> {
    device_flow::routes().merge(device_session::routes())
}
