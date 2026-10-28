//! Request/session/CSRF plumbing (OVL-04 part 6, PIDASHCONV-1542).
//!
//! Every `api` module kept private copies of the query map, the
//! session-auth hash and the CSRF failure shapes. Each family below has
//! exactly one canonical implementation; OSS modules import it.
//!
//! Moved here: `OneOrMany` (21 copies), `QueryMap` (21), `query_last`
//! (21), `fields_param` (8), `query_param` (3), `read_body` (6, plus 6
//! thin `Denial` wrappers at the call sites), `session_auth_hash` (6
//! plus the `license` `make_session_auth_hash` twin),
//! salt consts (`SESSION_AUTH_HASH_SALT` / `SALT`, 5), `verify_session_hash` (3),
//! `constant_time_eq` (3), `hex_encode` (6), `csrf_failure_json` (2),
//! `csrf_failure_response` (3), `TokenSource` + `csrf_token_source` (3),
//! `normalize_email` (2).
//!
//! Shapes that stay local: the test-only `body_bytes` / `set_cookies` /
//! nested `hex_encode` helpers; `csrf_gate`, `csrf_check_token` and
//! `csrf_rotate_cookie` (thin adapters over the `pidash_auth` kernels
//! and the local `CsrfDenial` / `CookieCtx` types); the divergent
//! `csrf_request_token`, `bearer_token`, `header_value` and `urljoin`
//! pairs; the `origin_allowed` / `split_origin` / `origin_verified` /
//! `urlparse` singletons; the `query_last` shapes over raw pairs,
//! `HashMap<String, String>` and raw query strings; the
//! `handlers_user` `normalize_email` (`py_strip`, intentional: it
//! strips `\x1c`-`\x1f`, `trim` does not); the webhook
//! `constant_time_eq` (zero-padded, different timing) and the `&str`
//! password-token one; the `space` / `app_issues` `read_body` variants
//! (different return shapes).

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

/// One query value or many (`api/views/base.py` multi-value params, so
/// callers read last-wins like Django's `QueryDict`).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

/// The multi-value query map every handler extracts.
pub type QueryMap = HashMap<String, OneOrMany>;

/// Django `QueryDict.get`: the last value, or `None`.
pub fn query_last(query: &QueryMap, key: &str) -> Option<String> {
    query.get(key).map(|value| match value {
        OneOrMany::One(one) => one.clone(),
        OneOrMany::Many(many) => many.last().cloned().unwrap_or_default(),
    })
}

/// `self.fields` / `self.expand` (`api/views/base.py:212-220`): comma
/// split, empties dropped, `None` when empty.
pub fn fields_param(query: &QueryMap, key: &str) -> Option<Vec<String>> {
    let raw = query_last(query, key).unwrap_or_default();
    let fields: Vec<String> = raw
        .split(',')
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .collect();
    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

/// Django `request.query_params.get`: the last value wins on repeats
/// (`query_last`), and an empty value disables the filter (`None`).
pub fn query_param(params: &QueryMap, key: &str) -> Option<String> {
    query_last(params, key).filter(|raw| !raw.is_empty())
}

// ---------------------------------------------------------------------------
// Body
// ---------------------------------------------------------------------------

/// Drain the request body. The six `v1_work_items` copies wrap this in
/// their local `Denial` via `db_error(error, "read-body")`.
pub async fn read_body(body: Body) -> Result<Vec<u8>, axum::Error> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .map(|bytes| bytes.to_vec())
}

// ---------------------------------------------------------------------------
// Session auth hash
// ---------------------------------------------------------------------------

/// Salt for `user.get_session_auth_hash()`: the Django method path
/// `salted_hmac` mixes into the HMAC key.
pub const SESSION_AUTH_HASH_SALT: &str =
    "django.contrib.auth.models.AbstractBaseUser.get_session_auth_hash";

/// Lowercase hex of `bytes` (HMAC digests, avatar colors).
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Constant-time byte equality (lengths compared first, then every
/// byte OR-ed into the accumulator).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// `user.get_session_auth_hash()`: `salted_hmac(...get_session_auth_hash,
/// password).hexdigest()` — `HMAC-SHA256(password)` under
/// `SHA256(salt || secret_key)`, hex-encoded.
pub fn session_auth_hash(password_field: &str, secret_key: &[u8]) -> String {
    let key = Sha256::digest([SESSION_AUTH_HASH_SALT.as_bytes(), secret_key].concat());
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC-SHA256 accepts any key length");
    mac.update(password_field.as_bytes());
    hex_encode(&mac.finalize().into_bytes())
}

/// `get_session_auth_hash()` compared constant-time against the session
/// `_auth_user_hash`.
pub fn verify_session_hash(session_hash: &str, password_field: &str, secret_key: &[u8]) -> bool {
    let expected = session_auth_hash(password_field, secret_key);
    constant_time_eq(expected.as_bytes(), session_hash.as_bytes())
}

// ---------------------------------------------------------------------------
// CSRF
// ---------------------------------------------------------------------------

/// Where the request token came from (`CsrfViewMiddleware._check_token`:
/// the `csrfmiddlewaretoken` POST field wins when non-empty, else the
/// `X-CSRFToken` header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Post,
    Header,
}

impl TokenSource {
    /// `_bad_token_message` source rendering (`csrf.py:347-352`;
    /// `CSRF_HEADER_NAME` is the default, parsed as `X-Csrftoken`).
    pub fn describe(self) -> &'static str {
        match self {
            TokenSource::Post => "POST",
            TokenSource::Header => "the 'X-Csrftoken' HTTP header",
        }
    }
}

/// Resolve the request token in Django's order: the last
/// `csrfmiddlewaretoken` form value when non-empty, else the header
/// when present (even empty — an empty header fails format, it does
/// not fall through to "missing").
pub fn csrf_token_source(
    pairs: &[(String, String)],
    header_token: Option<&str>,
) -> Option<(String, TokenSource)> {
    if let Some(value) = pairs
        .iter()
        .rev()
        .find(|(k, _)| k == "csrfmiddlewaretoken")
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
    {
        return Some((value.to_owned(), TokenSource::Post));
    }
    header_token.map(|token| (token.to_owned(), TokenSource::Header))
}

/// The 403 JSON CSRF failure (`{"detail":"CSRF Failed: <reason>"}`).
pub fn csrf_failure_json(reason: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::VARY, "Cookie"),
        ],
        format!(
            "{{\"detail\":\"CSRF Failed: {}\"}}",
            reason.replace('\\', "\\\\").replace('"', "\\\"")
        ),
    )
        .into_response()
}

/// The 200 HTML CSRF failure page (`csrf_failure` view): status,
/// content type and `Vary: Cookie` around the caller-rendered `body`.
pub fn csrf_failure_response(body: String) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::VARY, "Cookie"),
        ],
        body,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Email
// ---------------------------------------------------------------------------

/// Trim and lowercase an email address (login/registration forms).
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(entries: &[(&str, OneOrMany)]) -> QueryMap {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn query_last_wins_and_missing() {
        let query = params(&[
            ("a", OneOrMany::One("1".to_owned())),
            (
                "multi",
                OneOrMany::Many(vec!["x".to_owned(), "y".to_owned()]),
            ),
            ("empty_many", OneOrMany::Many(vec![])),
        ]);
        assert_eq!(query_last(&query, "a").as_deref(), Some("1"));
        assert_eq!(query_last(&query, "multi").as_deref(), Some("y"));
        assert_eq!(query_last(&query, "empty_many").as_deref(), Some(""));
        assert_eq!(query_last(&query, "missing"), None);
    }

    #[test]
    fn query_map_deserializes_untagged() {
        let query: QueryMap =
            serde_json::from_str(r#"{"a":"1","multi":["x","y"]}"#).expect("query map");
        assert_eq!(query_last(&query, "a").as_deref(), Some("1"));
        assert_eq!(query_last(&query, "multi").as_deref(), Some("y"));
    }

    #[test]
    fn fields_param_splits_and_drops_empties() {
        let query = params(&[
            ("fields", OneOrMany::One("a,,b".to_owned())),
            ("blank", OneOrMany::One(String::new())),
        ]);
        assert_eq!(
            fields_param(&query, "fields"),
            Some(vec!["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(fields_param(&query, "blank"), None);
        assert_eq!(fields_param(&query, "missing"), None);
    }

    /// Query params: last value wins, empty/missing disables.
    #[test]
    fn query_param_last_wins() {
        let params = params(&[
            ("a", OneOrMany::One("1".to_owned())),
            (
                "multi",
                OneOrMany::Many(vec!["x".to_owned(), "y".to_owned()]),
            ),
            ("empty", OneOrMany::One(String::new())),
        ]);
        assert_eq!(query_param(&params, "a").as_deref(), Some("1"));
        assert_eq!(query_param(&params, "multi").as_deref(), Some("y"));
        assert_eq!(query_param(&params, "empty"), None);
        assert_eq!(query_param(&params, "missing"), None);
    }

    #[tokio::test]
    async fn read_body_drains() {
        assert_eq!(
            read_body(Body::from("hello")).await.expect("body"),
            b"hello".to_vec()
        );
        assert!(read_body(Body::empty()).await.expect("body").is_empty());
    }

    #[test]
    fn hex_encode_vectors() {
        assert_eq!(hex_encode(b""), "");
        assert_eq!(hex_encode(&[0x00, 0x0f, 0xf0, 0xff]), "000ff0ff");
    }

    #[test]
    fn constant_time_eq_vectors() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"a", b"b"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    // `session_auth_hash` goldens: `salted_hmac` vectors from the
    // contract harness (`_harness/http.py::session_auth_hash`), which
    // mirrors Django's `get_session_auth_hash` (moved from
    // `auth_oauth::login`, with the relocated gitlab oracle).
    #[test]
    fn session_auth_hash_matches_django_vectors() {
        assert_eq!(
            session_auth_hash(
                "pbkdf2_sha256$600000$abc$def",
                b"contract-1357-fixed-secret"
            ),
            "b0e4e43bc88423e12b4733b8049b3a0c9e01c854e932de289f7ee18809c85e8d"
        );
        assert_eq!(
            session_auth_hash("pw", b"secret"),
            "78c7ecf82272227e62a5aa5f01cc8e2e3b4ce3b13352ddc6521ec0c4fa3e7f81"
        );
        // Django `salted_hmac(...get_session_auth_hash,
        // "pbkdf2_sha256$1200000$abc$def", secret="topsecret",
        // algorithm="sha256")` (the default `algorithm="sha1"` is a
        // trap — `get_session_auth_hash` passes sha256 explicitly).
        assert_eq!(
            session_auth_hash("pbkdf2_sha256$1200000$abc$def", b"topsecret"),
            "9c9a3cd895ca244f0c1fc8a4d24cd83d65287bb3353f39d69ab926a0d240b079"
        );
    }

    #[test]
    fn verify_session_hash_round_trip() {
        let password = "pbkdf2_sha256$600000$abc$def";
        let secret = b"contract-1357-fixed-secret".as_slice();
        // Independent oracle: re-derive the hex without `hex_encode`.
        let key = Sha256::digest([SESSION_AUTH_HASH_SALT.as_bytes(), secret].concat());
        let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("key");
        mac.update(password.as_bytes());
        let expected: Vec<u8> = mac.finalize().into_bytes().to_vec();
        let hex: String = expected.iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify_session_hash(&hex, password, secret));
        assert!(!verify_session_hash(&hex, "other-password-field", secret));
        assert!(!verify_session_hash(&hex, password, b"other-secret"));
    }

    #[test]
    fn token_source_prefers_post() {
        let pairs = vec![
            ("a".to_owned(), "1".to_owned()),
            ("csrfmiddlewaretoken".to_owned(), "post-token".to_owned()),
        ];
        assert_eq!(
            csrf_token_source(&pairs, Some("header-token")),
            Some(("post-token".to_owned(), TokenSource::Post))
        );
        assert_eq!(
            csrf_token_source(&[], Some("header-token")),
            Some(("header-token".to_owned(), TokenSource::Header))
        );
        // Empty form value falls through to the header.
        let empty = vec![("csrfmiddlewaretoken".to_owned(), String::new())];
        assert_eq!(
            csrf_token_source(&empty, Some("header-token")),
            Some(("header-token".to_owned(), TokenSource::Header))
        );
        assert_eq!(csrf_token_source(&[], None), None);
        // Empty header is Some (fails format, not "missing").
        assert_eq!(
            csrf_token_source(&[], Some("")),
            Some((String::new(), TokenSource::Header))
        );
    }

    #[test]
    fn token_source_describes() {
        assert_eq!(TokenSource::Post.describe(), "POST");
        assert_eq!(
            TokenSource::Header.describe(),
            "the 'X-Csrftoken' HTTP header"
        );
    }

    #[tokio::test]
    async fn csrf_failure_json_shape() {
        let body = axum::body::to_bytes(
            csrf_failure_json("CSRF cookie not set.").into_body(),
            usize::MAX,
        )
        .await
        .expect("body");
        assert_eq!(
            String::from_utf8(body.to_vec()).expect("utf8"),
            r#"{"detail":"CSRF Failed: CSRF cookie not set."}"#
        );
        let escaped = axum::body::to_bytes(
            csrf_failure_json("say \"hi\" \\ bye").into_body(),
            usize::MAX,
        )
        .await
        .expect("body");
        assert_eq!(
            String::from_utf8(escaped.to_vec()).expect("utf8"),
            r#"{"detail":"CSRF Failed: say \"hi\" \\ bye"}"#
        );
    }

    #[test]
    fn csrf_failure_json_status_and_headers() {
        let response = csrf_failure_json("x");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let headers = response.headers();
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        assert_eq!(headers[header::VARY], "Cookie");
    }

    #[tokio::test]
    async fn csrf_failure_response_shape() {
        let response = csrf_failure_response("<html>nope</html>".to_owned());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(response.headers()[header::VARY], "Cookie");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(
            String::from_utf8(body.to_vec()).expect("utf8"),
            "<html>nope</html>"
        );
    }

    #[test]
    fn normalize_email_strips_and_lowers() {
        assert_eq!(
            normalize_email("  Admin@Example.COM  "),
            "admin@example.com"
        );
        assert_eq!(normalize_email("a@b.com"), "a@b.com");
    }
}
