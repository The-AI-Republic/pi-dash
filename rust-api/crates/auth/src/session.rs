#![forbid(unsafe_code)]

//! Django DB-session reader (`pi_dash.db.models.session` + `SessionMiddleware`).
//!
//! Python references:
//!
//! - `pi_dash/db/models/session.py` — the `sessions` table: `session_key`
//!   (primary key, 128 lowercase-alphanumeric chars for new keys),
//!   `session_data` (TimestampSigner-signed JSON, see [`crate::signing`]),
//!   `expire_date`, plus the mirrored `user_id` / `device_info` columns.
//! - `pi_dash/authentication/middleware/session.py` — the cookie contract:
//!   the cookie holds the **raw session key** (it is not signed); requests
//!   whose path contains `"instances"` use the `admin-session-id` cookie,
//!   everything else uses `session-id`.
//!
//! This module is the pure half of that middleware: cookie-name routing,
//! key-shape screening, `session_data` decoding, and the expiry predicate.
//! Row fetching (`SELECT session_data, expire_date FROM sessions WHERE
//! session_key = ?`) is the caller's SQL — the same query the Django
//! `SessionStore` issues — so this crate stays free of database I/O.

use thiserror::Error as ThisError;

use crate::signing::{Signer, SigningError, SESSION_SIGNING_SALT};

/// Cookie for ordinary sessions (`settings.SESSION_COOKIE_NAME`).
pub const SESSION_COOKIE_NAME: &str = "session-id";
/// Cookie for license-admin paths (`settings.ADMIN_SESSION_COOKIE_NAME`).
pub const ADMIN_SESSION_COOKIE_NAME: &str = "admin-session-id";

/// Pick the session cookie exactly like `SessionMiddleware.process_request`:
/// paths containing `"instances"` read the admin cookie, the rest read the
/// ordinary one. The substring test is verbatim Python (`in request.path`).
pub fn cookie_name_for_path(path: &str) -> &'static str {
    if path.contains("instances") {
        ADMIN_SESSION_COOKIE_NAME
    } else {
        SESSION_COOKIE_NAME
    }
}

/// Screen a cookie value before it reaches SQL.
///
/// Django looks up whatever string the cookie holds; new keys are 128
/// `[a-z0-9]` chars (`SessionStore._get_new_session_key`) while legacy rows
/// hold shorter keys, so the reader stays liberal: any non-empty key of at
/// most 128 chars passes and the database decides the rest.
pub fn is_plausible_session_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 128
}

/// A decoded Django session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionData {
    /// `request.user` id, from `_auth_user_id`. `None` means anonymous:
    /// Django's `login()` writes the key, so its absence is "not logged in".
    pub user_id: Option<String>,
    /// Backend path from `_auth_user_backend` (informational only).
    pub backend: Option<String>,
    /// Session-auth hash from `_auth_user_hash` (informational only).
    pub session_hash: Option<String>,
    /// `device_info` dict written by `user_login()`. Django stores `None`
    /// unless the value is a dict; anything else decodes to `None` here too.
    pub device_info: Option<serde_json::Value>,
}

/// Why a session could not be established.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum SessionError {
    #[error("no session cookie on the request")]
    Missing,
    #[error("session key is not usable")]
    BadKey,
    #[error("session row is missing, undecodable, or expired")]
    Invalid,
    #[error("session signing failed: {0}")]
    Signing(#[from] SigningError),
}

/// Decode a `session_data` column value with Django's `SECRET_KEY`.
///
/// Mirrors `SessionStore.decode`: on any failure Django logs and returns an
/// empty session, i.e. an anonymous one. Callers that need that behaviour
/// map `Err` to "no session"; the error is preserved here so middleware can
/// still distinguish corrupt rows from missing ones in logs.
pub fn decode_session_data(
    session_data: &str,
    secret_key: impl AsRef<[u8]>,
) -> Result<SessionData, SessionError> {
    decode_with_signer(session_data, &Signer::new(secret_key, SESSION_SIGNING_SALT))
}

fn decode_with_signer(session_data: &str, signer: &Signer) -> Result<SessionData, SessionError> {
    let data: serde_json::Value = signer.unsign_object(session_data)?;
    let get_string = |key: &str| {
        data.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    Ok(SessionData {
        user_id: get_string("_auth_user_id"),
        backend: get_string("_auth_user_backend"),
        session_hash: get_string("_auth_user_hash"),
        device_info: data.get("device_info").filter(|v| v.is_object()).cloned(),
    })
}

/// True when the `sessions.expire_date` row is no longer usable.
///
/// Mirrors the store's `expire_date__gt=now` lookup: a session is live only
/// while its expiry is strictly after now. Timestamps are unix seconds.
pub fn is_expired(expire_date_unix: i64, now_unix: i64) -> bool {
    expire_date_unix <= now_unix
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"f05-test-secret-key-12345";

    // Same Django-issued value as in signing.rs: a full login session.
    const DJANGO_SESSION: &str = ".eJxVjEEOwiAQRe_Cup3YFkvoTj2DazKF0RItkEKNifHuQuNCl_PnvfdiCtc0qTXSoqxhAxNaUt8LWQve7mu-M1RLzseaWjnuSXRC7jpW_Woj6hu54garDMYJyo9cshqT9Q7QYEi0gF7IlBnvcFpj8vMhc8ev_ZecciX3cNRN221HxQw9rCZl3cWz4cU2Dq-5l8HzIQM2KDRmoRjz0kALHfDi-RmtK7UQgJ44hzuB9jN7vz8DyFT-:1xAYRV:-9lka6d1CruUv4KZTvhYIUY9g4AO6iV4PW0-zh110xc";

    #[test]
    fn decodes_login_session() {
        let session = decode_session_data(DJANGO_SESSION, SECRET).unwrap();
        assert_eq!(
            session.user_id.as_deref(),
            Some("7c9e6679-7425-40de-944b-e29b5e737903")
        );
        assert_eq!(
            session.backend.as_deref(),
            Some("pi_dash.authentication.adapter.credential.CustomAuthBackend")
        );
        assert_eq!(session.session_hash.as_deref(), Some("abc123hash"));
        assert_eq!(
            session.device_info.as_ref().unwrap()["domain"],
            "app.example.com"
        );
    }

    #[test]
    fn anonymous_session_has_no_user() {
        let signer = Signer::new(SECRET, SESSION_SIGNING_SALT);
        let raw = signer
            .sign_object(&serde_json::json!({}), 1790452337)
            .unwrap();
        let session = decode_with_signer(&raw, &signer).unwrap();
        assert_eq!(session.user_id, None);
        assert_eq!(session.device_info, None);
    }

    #[test]
    fn non_dict_device_info_is_none() {
        // Django's create_model_instance keeps device_info only for dicts.
        let signer = Signer::new(SECRET, SESSION_SIGNING_SALT);
        let raw = signer
            .sign_object(
                &serde_json::json!({"_auth_user_id": "1", "device_info": "nope"}),
                1790452337,
            )
            .unwrap();
        let session = decode_with_signer(&raw, &signer).unwrap();
        assert_eq!(session.user_id.as_deref(), Some("1"));
        assert_eq!(session.device_info, None);
    }

    #[test]
    fn corrupt_data_is_invalid_not_anonymous_confusion() {
        assert!(decode_session_data("definitely-not-signed", SECRET).is_err());
    }

    #[test]
    fn expiry_is_strictly_greater_than_now() {
        assert!(is_expired(1000, 1000));
        assert!(is_expired(999, 1000));
        assert!(!is_expired(1001, 1000));
    }

    #[test]
    fn cookie_routing_matches_middleware() {
        assert_eq!(
            cookie_name_for_path("/api/auth/sign-in/"),
            SESSION_COOKIE_NAME
        );
        assert_eq!(
            cookie_name_for_path("/api/instances/license/"),
            ADMIN_SESSION_COOKIE_NAME
        );
        assert_eq!(cookie_name_for_path("/"), SESSION_COOKIE_NAME);
    }

    #[test]
    fn key_screening() {
        assert!(is_plausible_session_key(&"a".repeat(128)));
        assert!(is_plausible_session_key("abc123"));
        assert!(!is_plausible_session_key(""));
        assert!(!is_plausible_session_key(&"a".repeat(129)));
    }
}
