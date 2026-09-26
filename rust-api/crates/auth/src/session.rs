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

/// Format unix seconds as an IMF-fixdate (`http_date`), e.g.
/// `Wed, 21 Oct 2026 07:28:00 GMT`. Mirrors
/// `django.utils.http.http_date` (via `email.utils.formatdate(usegmt=True)`);
/// used for cookie `expires` attributes.
pub fn http_date(unix_secs: i64) -> String {
    const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = unix_secs.div_euclid(86_400);
    let tod = unix_secs.rem_euclid(86_400);
    // civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    // 1970-01-01 was a Thursday; Monday-based weekday index 3.
    let weekday = WEEKDAYS[((days + 3).rem_euclid(7)) as usize];
    format!(
        "{weekday}, {day:02} {} {year:04} {:02}:{:02}:{:02} GMT",
        MONTHS[(month - 1) as usize],
        tod / 3_600,
        (tod % 3_600) / 60,
        tod % 60,
    )
}

/// Attributes for one `Set-Cookie` value, mirroring Django's `set_cookie`
/// arguments as `SessionMiddleware.process_response` passes them: `Path`
/// is always `/`, `SameSite` always `Lax`; `Secure`/`HttpOnly` are only
/// present when truthy (Django passes `secure=... or None`); `Domain` only
/// when configured. `max_age`/`expires` are both `None` for
/// expire-at-browser-close sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetCookie {
    pub name: String,
    pub value: String,
    pub expires: Option<String>,
    pub max_age: Option<i64>,
    pub domain: Option<String>,
    pub path: String,
    pub secure: bool,
    pub httponly: bool,
    pub samesite: String,
}

/// Render one `Set-Cookie` header value. Attribute order is
/// case-insensitive alphabetical (`Domain`, `expires`, `HttpOnly`,
/// `Max-Age`, `Path`, `SameSite`, `Secure`), exactly as Python's
/// `Morsel.output` emits them; an empty value renders quoted (`""`), as
/// `delete_cookie` produces.
pub fn render_set_cookie(cookie: &SetCookie) -> String {
    let mut out = String::new();
    out.push_str(&cookie.name);
    out.push('=');
    if cookie.value.is_empty() {
        out.push_str("\"\"");
    } else {
        out.push_str(&cookie.value);
    }
    // Case-insensitive alphabetical, matching Morsel emission order.
    if let Some(domain) = &cookie.domain {
        out.push_str("; Domain=");
        out.push_str(domain);
    }
    if let Some(expires) = &cookie.expires {
        out.push_str("; expires=");
        out.push_str(expires);
    }
    if cookie.httponly {
        out.push_str("; HttpOnly");
    }
    if let Some(max_age) = cookie.max_age {
        out.push_str(&format!("; Max-Age={max_age}"));
    }
    out.push_str("; Path=");
    out.push_str(&cookie.path);
    out.push_str("; SameSite=");
    out.push_str(&cookie.samesite);
    if cookie.secure {
        out.push_str("; Secure");
    }
    out
}

/// Render the `Set-Cookie` value `delete_cookie` emits: empty value,
/// `Max-Age=0`, the epoch expiry, `Path` and `SameSite`, plus `Domain` when
/// configured — never `Secure`/`HttpOnly`. Django's `delete_cookie` passes
/// `SESSION_COOKIE_DOMAIN` through, so a deployment with `COOKIE_DOMAIN`
/// set deletes a domain-scoped cookie.
pub fn render_delete_cookie(
    name: &str,
    path: &str,
    samesite: &str,
    domain: Option<&str>,
) -> String {
    render_set_cookie(&SetCookie {
        name: name.to_string(),
        value: String::new(),
        expires: Some("Thu, 01 Jan 1970 00:00:00 GMT".to_string()),
        max_age: Some(0),
        domain: domain.map(str::to_owned),
        path: path.to_string(),
        secure: false,
        httponly: false,
        samesite: samesite.to_string(),
    })
}

/// Generate a fresh session key, mirroring
/// `SessionStore._get_new_session_key`: 128 chars from lowercase ASCII +
/// digits (`VALID_KEY_CHARS` in `pi_dash/db/models/session.py`). The store
/// loops until the key is unused, exactly like the Python `while True`.
pub fn generate_session_key() -> String {
    const KEY_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    const KEY_LEN: usize = 128;
    let mut rng = rand::rng();
    (0..KEY_LEN)
        .map(|_| {
            let i = rand::Rng::random_range(&mut rng, 0..KEY_CHARS.len());
            KEY_CHARS[i] as char
        })
        .collect()
}

/// Read one cookie value from a `Cookie` header, mirroring
/// `request.COOKIES.get(name)`: `;`-separated pairs, surrounding whitespace
/// stripped, surrounding double quotes stripped.
pub fn cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if key.trim() != name {
            return None;
        }
        let value = value.trim();
        Some(
            value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value)
                .to_string(),
        )
    })
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

    #[test]
    fn http_date_matches_python_formatdate() {
        // Vectors from email.utils.formatdate(ts, usegmt=True).
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(1), "Thu, 01 Jan 1970 00:00:01 GMT");
        assert_eq!(http_date(-1), "Wed, 31 Dec 1969 23:59:59 GMT");
        assert_eq!(http_date(1790452337), "Sat, 26 Sep 2026 19:52:17 GMT");
        assert_eq!(http_date(1761092880), "Wed, 22 Oct 2025 00:28:00 GMT");
        assert_eq!(http_date(253402300799), "Fri, 31 Dec 9999 23:59:59 GMT");
    }

    #[test]
    fn set_cookie_renders_like_django_morsel() {
        // Vectors from HttpResponse.set_cookie(...).cookies.output().
        let rendered = render_set_cookie(&SetCookie {
            name: "session-id".to_string(),
            value: "abc123".to_string(),
            expires: Some("Wed, 21 Oct 2026 07:28:00 GMT".to_string()),
            max_age: Some(604800),
            domain: None,
            path: "/".to_string(),
            secure: false,
            httponly: false,
            samesite: "Lax".to_string(),
        });
        assert_eq!(
            rendered,
            "session-id=abc123; expires=Wed, 21 Oct 2026 07:28:00 GMT; Max-Age=604800; Path=/; SameSite=Lax"
        );
        let rendered = render_set_cookie(&SetCookie {
            name: "session-id".to_string(),
            value: "abc123".to_string(),
            expires: Some("Wed, 21 Oct 2026 07:28:00 GMT".to_string()),
            max_age: Some(3600),
            domain: Some("example.com".to_string()),
            path: "/".to_string(),
            secure: true,
            httponly: true,
            samesite: "Lax".to_string(),
        });
        assert_eq!(
            rendered,
            "session-id=abc123; Domain=example.com; expires=Wed, 21 Oct 2026 07:28:00 GMT; HttpOnly; Max-Age=3600; Path=/; SameSite=Lax; Secure"
        );
        // Browser-close: no Max-Age, no expires.
        let rendered = render_set_cookie(&SetCookie {
            name: "session-id".to_string(),
            value: "abc123".to_string(),
            expires: None,
            max_age: None,
            domain: None,
            path: "/".to_string(),
            secure: false,
            httponly: false,
            samesite: "Lax".to_string(),
        });
        assert_eq!(rendered, "session-id=abc123; Path=/; SameSite=Lax");
    }

    #[test]
    fn delete_cookie_renders_like_django() {
        assert_eq!(
            render_delete_cookie("session-id", "/", "Lax", None),
            "session-id=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax"
        );
        // A configured COOKIE_DOMAIN rides along, like `delete_cookie`
        // receiving `SESSION_COOKIE_DOMAIN` (verified against live Django).
        assert_eq!(
            render_delete_cookie("session-id", "/", "Lax", Some("example.com")),
            "session-id=\"\"; Domain=example.com; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax"
        );
    }

    #[test]
    fn cookie_value_parses_like_django() {
        assert_eq!(
            cookie_value("other=1; session-id=abc123; x=2", "session-id"),
            Some("abc123".to_string())
        );
        assert_eq!(
            cookie_value("session-id=\"quoted\"; x=2", "session-id"),
            Some("quoted".to_string())
        );
        assert_eq!(
            cookie_value("session-id = spaced ", "session-id"),
            Some("spaced".to_string())
        );
        assert_eq!(cookie_value("other=1", "session-id"), None);
        assert_eq!(cookie_value("", "session-id"), None);
    }

    #[test]
    fn generated_keys_match_django_shape() {
        for _ in 0..10 {
            let key = generate_session_key();
            assert_eq!(key.len(), 128);
            assert!(key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
        }
        assert_ne!(generate_session_key(), generate_session_key());
    }
}
