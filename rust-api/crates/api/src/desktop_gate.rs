//! Injectable desktop-session verdict (PIDASHCONV-917).
//!
//! Both desktop-only endpoint families (`assistant::agent_profile`, the
//! `runner_enroll` desktop-enroll preamble) consult this instead of
//! hardcoding the CE session-key check, so the private overlay
//! substitutes its OIDC `client`-claim check without touching the
//! callers. The verdict routes through the admission closure form
//! ([`pidash_services::dispatch::is_desktop_session`], the DRF
//! `IsDesktopSession.has_permission` shape: the authentication fold,
//! then the EE seam verdict), and the closure consults the gate the
//! overlay installed on [`crate::state::AppState`].
//!
//! The gate reads request data, not the request: the caller extracts the
//! session dict plus the raw `Authorization` and `Cookie` headers into a
//! [`DesktopCredential`]. CE reads only the session dict (the
//! `pidash_client == "desktop"` marker from
//! `ee/authentication/desktop.py`); the cloud overlay reads only the
//! headers (the signed access token whose `client` claim the desktop
//! OIDC exchange stamped —
//! `ee-overlay/apps/api/pi_dash/ee/authentication/desktop.py`), because a
//! claim is fixed at sign-in while a session key can be written by a
//! later request. The raw header values travel unparsed so each build's
//! gate applies its own source's parsing (the `bearer ` prefix check, the
//! access-cookie name).
//!
//! A missing session, a missing header, or an unreadable value is simply
//! absent input; each gate fails closed over absence, matching the
//! `except`/`None` arms in its Python source.

use axum::http::HeaderMap;
use serde_json::Value;

/// Request data the desktop verdict may consult, extracted by the
/// caller. Every field is optional input: CE needs only [`DesktopCredential::session`],
/// the cloud overlay only [`DesktopCredential::authorization`] and
/// [`DesktopCredential::cookie`].
#[derive(Debug, Clone, Copy)]
pub struct DesktopCredential<'a> {
    /// The full session-dict snapshot (`None` when the request carries
    /// no session extension or the snapshot is missing).
    pub session: Option<&'a Value>,
    /// The raw `Authorization` header value (`None` when missing or not
    /// UTF-8 — an unreadable credential is not a desktop).
    pub authorization: Option<&'a str>,
    /// The raw `Cookie` header value (same absence rule). The gate
    /// parses out the cookie its build names.
    pub cookie: Option<&'a str>,
}

/// First header value as `str`: missing or non-UTF-8 reads as absent
/// (fail closed — an unreadable credential is not a desktop).
pub fn header_value(headers: &HeaderMap, name: axum::http::HeaderName) -> Option<&str> {
    headers.get(name)?.to_str().ok()
}

/// Overlay seam for `ee/authentication/desktop.py::request_is_desktop`,
/// over the full request credential: the CE session dict plus the raw
/// auth headers. Every build provides its own verdict —
/// [`CeDesktopRequestGate`] is the CE one; the overlay's gate answers
/// from the signed client claim instead. `Send + Sync` so the overlay
/// installs its gate on [`crate::state::AppState`] once at boot.
pub trait DesktopRequestGate: Send + Sync {
    fn is_desktop(&self, credential: &DesktopCredential) -> bool;
}

/// CE verdict: the `pidash_client == "desktop"` session-key check
/// (`ee/authentication/desktop.py:27-37`), through the F-10
/// [`pidash_auth::permissions::desktop::DesktopGate`] so the CE check
/// keeps its single definition. Headers are ignored — CE
/// authenticates with Django sessions. A missing/unreadable session is
/// not a desktop (the `except` in Python returns `False`).
pub struct CeDesktopRequestGate;

impl DesktopRequestGate for CeDesktopRequestGate {
    fn is_desktop(&self, credential: &DesktopCredential) -> bool {
        use pidash_auth::permissions::desktop::DesktopGate;
        use pidash_auth::permissions::desktop::SessionDesktopGate;
        use pidash_auth::permissions::desktop::DESKTOP_SESSION_KEY;
        let value = credential
            .session
            .and_then(|data| data.get(DESKTOP_SESSION_KEY))
            .and_then(Value::as_str);
        SessionDesktopGate.is_desktop(true, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An overlay-shaped gate: answers from the bearer token (like the
    /// cloud `client`-claim check) and ignores the session dict.
    struct TokenDesktopGate;

    impl DesktopRequestGate for TokenDesktopGate {
        fn is_desktop(&self, credential: &DesktopCredential) -> bool {
            matches!(
                credential.authorization,
                Some(header) if header == "Bearer desktop-token"
            )
        }
    }

    fn credential<'a>(
        session: Option<&'a Value>,
        authorization: Option<&'a str>,
        cookie: Option<&'a str>,
    ) -> DesktopCredential<'a> {
        DesktopCredential {
            session,
            authorization,
            cookie,
        }
    }

    #[test]
    fn ce_gate_matches_the_session_dict_check() {
        let gate = CeDesktopRequestGate;
        assert!(gate.is_desktop(&credential(
            Some(&serde_json::json!({"pidash_client": "desktop"})),
            None,
            None,
        )));
        assert!(!gate.is_desktop(&credential(
            Some(&serde_json::json!({"pidash_client": "browser"})),
            None,
            None,
        )));
        assert!(!gate.is_desktop(&credential(Some(&serde_json::json!({})), None, None,)));
        assert!(!gate.is_desktop(&credential(None, None, None)));
    }

    #[test]
    fn ce_gate_ignores_the_auth_headers() {
        let gate = CeDesktopRequestGate;
        // A bearer token never makes a CE session a desktop, and never
        // unmakes one either.
        assert!(!gate.is_desktop(&credential(
            None,
            Some("Bearer desktop-token"),
            Some("airepublic_access=desktop-token"),
        )));
        assert!(gate.is_desktop(&credential(
            Some(&serde_json::json!({"pidash_client": "desktop"})),
            Some("Bearer desktop-token"),
            Some("airepublic_access=desktop-token"),
        )));
    }

    #[test]
    fn alternate_gate_answers_from_the_token() {
        let gate = TokenDesktopGate;
        assert!(gate.is_desktop(&credential(None, Some("Bearer desktop-token"), None)));
        assert!(!gate.is_desktop(&credential(
            Some(&serde_json::json!({"pidash_client": "desktop"})),
            None,
            None,
        )));
        assert!(!gate.is_desktop(&credential(None, Some("Bearer browser-token"), None)));
        assert!(!gate.is_desktop(&credential(None, None, None)));
    }

    #[test]
    fn header_value_reads_first_str_or_absent() {
        use axum::http::HeaderValue;
        let mut headers = HeaderMap::new();
        assert_eq!(
            header_value(&headers, axum::http::header::AUTHORIZATION),
            None
        );
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(
            header_value(&headers, axum::http::header::AUTHORIZATION),
            Some("Bearer abc")
        );
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_bytes(&[0xff, 0xfe]).expect("non-utf8 value"),
        );
        assert_eq!(header_value(&headers, axum::http::header::COOKIE), None);
    }
}
