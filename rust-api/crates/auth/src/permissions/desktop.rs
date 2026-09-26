#![forbid(unsafe_code)]

//! Desktop-session gate (`managed_runner/permissions.py::IsDesktopSession`
//! over `ee/authentication/desktop.py::request_is_desktop`).
//!
//! Three provisioning endpoints are desktop-only (machine enrollment, the
//! agent model profile, the agent model token): "is authenticated" is not a
//! strong enough gate for handing a browser tab those secrets. CE marks
//! desktop sessions with the session key `pidash_client = "desktop"` at
//! sign-in; the cloud overlay replaces the check with the OIDC `client`
//! claim (an F-10 seam). A missing session, a missing key, or a broken
//! session store is "not desktop" — the broad `except` in Python fails
//! closed here too.

/// Session key CE uses to remember a desktop session.
pub const DESKTOP_SESSION_KEY: &str = "pidash_client";
/// Session value marking the desktop app.
pub const DESKTOP_CLIENT: &str = "desktop";

/// Mirror of `request_is_desktop`: `session_value` is the stored
/// `DESKTOP_SESSION_KEY` value (`None` when the session is missing, the key
/// absent, or the store broken).
pub fn is_desktop_session(authenticated: bool, session_value: Option<&str>) -> bool {
    if !authenticated {
        return false;
    }
    matches!(session_value, Some(value) if value == DESKTOP_CLIENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_desktop_value_passes() {
        assert!(is_desktop_session(true, Some("desktop")));
        assert!(!is_desktop_session(true, Some("browser")));
        assert!(!is_desktop_session(true, Some("")));
        assert!(!is_desktop_session(true, None));
    }

    #[test]
    fn anonymous_never_desktop() {
        assert!(!is_desktop_session(false, Some("desktop")));
    }
}
