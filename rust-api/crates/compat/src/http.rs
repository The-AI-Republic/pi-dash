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
//! PIDASHCONV-1623 moved `iri_to_uri` (from
//! `api::response_headers`, the `HttpResponseRedirect` encoder) and
//! `percent_encode` (from the `jobs::integrations::{git_sync,
//! github_sync}` twins, the strict segment/query encoder) here.
//!
//! OVL-11 residue (PIDASHCONV-1626): `split_authority` (the
//! `services::auth_session::shapes` twin, re-shaped to the overlay's
//! `Option` so the two overlay twins can adopt it) and `form_field`
//! (one urlencoded field over body + headers, Django `QueryDict.get`
//! last-wins, for the overlay `auth/session.rs` twin).
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
//! (different return shapes); the `(pairs, key)` form readers (the
//! `magic.rs` `String`-default, `password.rs` `Field`, `handlers_admin`
//! `form_last` and services `FormField` shapes over pre-parsed pairs)
//! and multipart framing (three divergent OSS semantics).
//!
//! PIDASHCONV-1626: OVL-04 listed `urlparse`/`urljoin` as in-scope but
//! did not ship them; `split_authority` below is the first urlsplit
//! piece to move here.

use axum::body::Body;
use axum::http::{header, HeaderMap, StatusCode};
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

// ---------------------------------------------------------------------------
// URL encoders
// ---------------------------------------------------------------------------

/// `django.utils.encoding.iri_to_uri` (`quote(iri,
/// safe="/#%[]=:;$&()+,!?*@'~")`, Django 4.2 `utils/encoding.py:110-138`):
/// `quote` always spares ASCII alphanumerics plus `-_.~`; the safe set
/// spares the RFC 3986 reserved marks and `%`. Every other byte — UTF-8
/// multibyte included — becomes uppercase `%XX`.
///
/// Django's `HttpResponseRedirect` applies this to the URL before storing
/// `Location` (`django/http/response.py:620`); every `HttpResponseRedirect`
/// emulation must call it before building the header value. It must
/// NOT be applied to other headers: a hand-set `response["Location"]`
/// skips it in Django.
///
/// Moved from `api::response_headers` (PIDASHCONV-1623); the OSS call
/// sites import it and the old path keeps a `pub use` shim.
pub fn iri_to_uri(joined: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(joined.len());
    for byte in joined.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || matches!(
                byte,
                b'/' | b'#'
                    | b'%'
                    | b'['
                    | b']'
                    | b'='
                    | b':'
                    | b';'
                    | b'$'
                    | b'&'
                    | b'('
                    | b')'
                    | b'+'
                    | b','
                    | b'!'
                    | b'?'
                    | b'*'
                    | b'@'
                    | b'\''
            );
        if keep {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 15) as usize] as char);
        }
    }
    out
}

/// `urllib.parse.quote(value, safe="")`: the RFC 3986 unreserved set
/// (`A-Z a-z 0-9 - _ . ~`) passes through, every other byte of the
/// UTF-8 form becomes uppercase `%XX`.
///
/// Moved from the `jobs::integrations::{git_sync, github_sync}` twins
/// (PIDASHCONV-1623), which used it for GitHub query values. This is
/// NOT `percent_encode_path` (the `services::assistant::tools_github`
/// path-segment encoder): same byte behavior today, but that twin is
/// out of scope here and keeps its own copy.
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// URL authority + form field (OVL-11 residue, PIDASHCONV-1626)
// ---------------------------------------------------------------------------

/// Authority split for the post-scheme remainder: a leading `//`
/// opens an authority running to the next `/?#` (empty authority, as
/// in `///x`, yields netloc `""` and path `/x`, matching `urlparse`);
/// `None` when there is no `//` at all.
///
/// Moved from `services::auth_session::shapes` (whose callers restore
/// the old `("", rest)` tuple with `unwrap_or`) to unblock the two
/// overlay twins (`cloud-api/src/auth/native.rs`,
/// `cloud-api/src/auth/web.rs`): `None` versus an empty authority is
/// load-bearing there (an empty authority still consumes the `//`),
/// so the tuple shape cannot serve them. The cut itself is identical
/// in all three twins.
pub fn split_authority(rest: &str) -> Option<(&str, &str)> {
    let after = rest.strip_prefix("//")?;
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    Some((&after[..end], &after[end..]))
}

/// The request MIME type: the content-type before `;`, lowercased
/// (the overlay `mime_of` gate, verbatim).
fn request_mime(headers: &HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// One urlencoded form field (`request.POST.get`, last wins): the
/// `application/x-www-form-urlencoded` parse of `body`, or `None`
/// when the content type is anything else (multipart framing stays
/// with the callers — the three OSS framers diverge — and Django
/// leaves `POST` empty for every other type).
///
/// Moved for the overlay `auth/session.rs` twin: the MIME gate is its
/// check verbatim, and `serde_urlencoded::from_bytes` parses through
/// the same `form_urlencoded::parse` iterator as the overlay's
/// `url::form_urlencoded::parse`, so both sides read byte-identical
/// pairs; Django `QueryDict.get` last-wins on top. Present-but-empty
/// reads `Some("")`, like `QueryDict.get` — callers that treat empty
/// as missing filter it themselves.
pub fn form_field(body: &[u8], headers: &HeaderMap, key: &str) -> Option<String> {
    if request_mime(headers) != "application/x-www-form-urlencoded" {
        return None;
    }
    let pairs: Vec<(String, String)> = serde_urlencoded::from_bytes(body).unwrap_or_default();
    pairs
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
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

    // Moved from `api::response_headers` with `iri_to_uri`.
    #[test]
    fn iri_to_uri_matches_django_vectors() {
        // Oracle: `django.utils.encoding.iri_to_uri` on the venv Django
        // (moved from `app_assets::handlers_v1`; CR/LF rows added for
        // PIDASHCONV-1356: `HttpResponseRedirect` percent-encodes them,
        // so they stay a 302 instead of `BadHeaderError` 500s).
        for (raw, expected) in [
            ("http://h/p/a b", "http://h/p/a%20b"),
            ("http://h/p/caf\u{e9}", "http://h/p/caf%C3%A9"),
            ("http://h/p/100%25x", "http://h/p/100%25x"),
            ("http://h/p/a?b#c", "http://h/p/a?b#c"),
            ("http://h/p/a+b", "http://h/p/a+b"),
            ("http://h/p/a\"b<c>d", "http://h/p/a%22b%3Cc%3Ed"),
            (
                "http://h/p/\u{4e2d}\u{6587}",
                "http://h/p/%E4%B8%AD%E6%96%87",
            ),
            ("http://h/p/a\nb", "http://h/p/a%0Ab"),
            ("http://h/p/a\rb", "http://h/p/a%0Db"),
        ] {
            assert_eq!(iri_to_uri(raw), expected, "iri_to_uri({raw:?})");
        }
    }

    #[test]
    fn percent_encode_strict_segment_set() {
        assert_eq!(percent_encode(""), "");
        assert_eq!(percent_encode("AZaz09-_.~"), "AZaz09-_.~");
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(
            percent_encode("owner,collaborator,organization_member"),
            "owner%2Ccollaborator%2Corganization_member"
        );
        assert_eq!(percent_encode("open"), "open");
        assert_eq!(percent_encode("100"), "100");
        assert_eq!(percent_encode("/"), "%2F");
        assert_eq!(percent_encode("%"), "%25");
        assert_eq!(percent_encode("caf\u{e9}"), "caf%C3%A9");
    }

    #[test]
    fn split_authority_cuts_at_path_query_fragment() {
        assert_eq!(split_authority("//host/path"), Some(("host", "/path")));
        assert_eq!(split_authority("//host?query"), Some(("host", "?query")));
        assert_eq!(split_authority("//host#frag"), Some(("host", "#frag")));
        assert_eq!(split_authority("//host"), Some(("host", "")));
        // An empty authority still consumes the `//` (matches `urlparse`).
        assert_eq!(split_authority("///x"), Some(("", "/x")));
        assert_eq!(split_authority("//"), Some(("", "")));
        // No `//` at all is `None` (never `("", rest)`).
        assert_eq!(split_authority("no-authority"), None);
        assert_eq!(split_authority("/relative"), None);
        assert_eq!(split_authority(""), None);
    }

    fn headers_with_content_type(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, value.parse().expect("content type"));
        headers
    }

    #[test]
    fn form_field_last_wins_urlencoded_only() {
        let headers = headers_with_content_type("application/x-www-form-urlencoded");
        assert_eq!(form_field(b"a=1&a=2", &headers, "a").as_deref(), Some("2"));
        assert_eq!(form_field(b"a=1&a=2", &headers, "missing"), None);
        // Present-but-empty is `Some("")`, like `QueryDict.get`.
        assert_eq!(form_field(b"a=", &headers, "a").as_deref(), Some(""));
        assert_eq!(form_field(b"a", &headers, "a").as_deref(), Some(""));
        // Decoding: `+` is a space, `%XX` percent-decoded (the
        // overlay `request_token_prefers_post_field_then_header`
        // vector reads identically through the canonical).
        assert_eq!(
            form_field(b"e=a%40x.com+e", &headers, "e").as_deref(),
            Some("a@x.com e")
        );
        assert_eq!(
            form_field(
                b"csrfmiddlewaretoken=a+b%20c",
                &headers,
                "csrfmiddlewaretoken"
            )
            .as_deref(),
            Some("a b c")
        );
        // Parameters ride along; the MIME match is case-insensitive.
        let params = headers_with_content_type("Application/X-WWW-Form-Urlencoded; charset=UTF-8");
        assert_eq!(form_field(b"a=1", &params, "a").as_deref(), Some("1"));
        // Multipart framing stays with the callers: `None` here even
        // for a framable body.
        let multipart = headers_with_content_type("multipart/form-data; boundary=abc");
        let framed = b"--abc\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--abc--\r\n";
        assert_eq!(form_field(framed, &multipart, "a"), None);
        // Anything else (and a missing or non-UTF8 header) is `None`.
        let json = headers_with_content_type("application/json");
        assert_eq!(form_field(br#"{"a":"1"}"#, &json, "a"), None);
        assert_eq!(form_field(b"a=1", &HeaderMap::new(), "a"), None);
        let mut invalid = HeaderMap::new();
        invalid.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_bytes(b"\xff\xfe").expect("non-utf8"),
        );
        assert_eq!(form_field(b"a=1", &invalid, "a"), None);
    }
}
