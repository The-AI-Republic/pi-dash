//! `CsrfViewMiddleware.process_view`'s Origin and Referer checks
//! (`django/middleware/csrf.py`, Django 4.2.30), shared by every Rust
//! CSRF gate (NEWPIDASH-12).
//!
//! Django runs these before the token check on every unsafe request a
//! CSRF gate covers: the plain auth Views through the middleware and the
//! DRF views through `SessionAuthentication.enforce_csrf`, which calls the
//! same `process_view`. An `Origin` header must equal the request's own
//! `scheme://host` (scheme from `is_secure()`) or match
//! `CSRF_TRUSTED_ORIGINS` exactly or by a `*.` subdomain wildcard. A
//! secure request without `Origin` must pass `_check_referer`. A request
//! that is not secure and has no `Origin` skips both checks.
//!
//! Moved from the private port in `api::prompting` (PIDASHCONV-1228/1506).
//! The functions take plain values or a `HeaderMap`, so OSS and overlay
//! gates can call them without the api crate's types.

use axum::http::HeaderMap;
use std::collections::{HashMap, HashSet};

/// `request.is_secure()` (`django/http/request.py:256-271`): the URI
/// scheme is https or, when `SECURE_PROXY_SSL_HEADER` is configured
/// (`production.py`: `("HTTP_X_FORWARDED_PROTO", "https")`), the first
/// comma-separated `X-Forwarded-Proto` value, stripped, equals `https`.
pub fn request_is_secure(
    uri_scheme: Option<&str>,
    headers: &HeaderMap,
    trust_forwarded_proto: bool,
) -> bool {
    if uri_scheme == Some("https") {
        return true;
    }
    trust_forwarded_proto
        && headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.split(',').next().unwrap_or("").trim() == "https")
}

/// The Origin/Referer half of `process_view` (`csrf.py:430-465`) for an
/// unsafe request: `Ok(())`, or Django's rejection reason
/// (`REASON_BAD_ORIGIN` or a `_check_referer` reason). The caller renders
/// it: the `csrf_failure` page for plain Views, `{"detail": "CSRF Failed:
/// <reason>"}` (403) for DRF views. Runs before the cookie and token
/// checks, exactly like Django.
pub fn check_origin_and_referer(
    headers: &HeaderMap,
    secure: bool,
    trusted: &[String],
    cookie_domain: Option<&str>,
) -> Result<(), String> {
    if let Some(origin) = headers.get("origin") {
        // A header that is not visible ASCII cannot equal any good
        // origin; Django reports it latin-1 decoded, rendered lossily.
        let origin = origin
            .to_str()
            .map(str::to_owned)
            .unwrap_or_else(|_| origin.as_bytes().iter().map(|&b| b as char).collect());
        let host = headers
            .get("host")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if !origin_verified(&origin, host, secure, trusted) {
            return Err(format!(
                "Origin checking failed - {origin} does not match any trusted origins."
            ));
        }
    } else if secure {
        check_referer(headers, cookie_domain, trusted)?;
    }
    Ok(())
}

/// `is_same_domain` (`django/utils/http.py:239-256`): a `.`-prefixed
/// pattern matches the bare domain and every subdomain. Only the pattern
/// is lowercased; the host is compared as given, exactly like Django.
pub fn same_domain(host: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = pattern.to_lowercase();
    let suffix_match =
        pattern.starts_with('.') && (host.ends_with(&pattern) || host == &pattern[1..]);
    suffix_match || pattern == host
}

/// The subset of `urllib.parse.urlparse` this gate needs: scheme,
/// netloc, and the remainder (path/query/fragment). `None` stands for
/// Python's empty scheme or netloc and for the `ValueError` branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub scheme: String,
    pub netloc: String,
    pub rest: String,
}

impl ParsedUrl {
    /// `ParseResult.geturl()`: the scheme is lowercased; the remaining
    /// components are returned as parsed.
    pub fn geturl(&self) -> String {
        format!("{}://{}{}", self.scheme, self.netloc, self.rest)
    }
}

pub fn urlparse(value: &str) -> Option<ParsedUrl> {
    let value = value
        .trim_start_matches(|c: char| c <= ' ')
        .replace(['\t', '\r', '\n'], "");
    let (scheme, after) = value.split_once(':')?;
    if !valid_url_scheme(scheme) {
        return None;
    }
    let after = after.strip_prefix("//")?;
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let netloc = &after[..end];
    if netloc.is_empty() || !valid_bracketed_netloc(netloc) {
        return None;
    }
    Some(ParsedUrl {
        scheme: scheme.to_ascii_lowercase(),
        netloc: netloc.to_owned(),
        rest: after[end..].to_owned(),
    })
}

fn valid_url_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// The bracketed-host validation in `urllib.parse.urlsplit` /
/// `_check_bracketed_netloc`.
fn valid_bracketed_netloc(netloc: &str) -> bool {
    let host_port = netloc.rsplit_once('@').map_or(netloc, |(_, rest)| rest);
    if !host_port.contains(['[', ']']) {
        return !netloc.contains(['[', ']']);
    }
    let Some(after_open) = host_port.strip_prefix('[') else {
        return false;
    };
    let Some((host, after_close)) = after_open.split_once(']') else {
        return false;
    };
    if !(after_close.is_empty() || after_close.starts_with(':')) {
        return false;
    }
    if let Some(rest) = host.strip_prefix('v') {
        let Some((hex, suffix)) = rest.split_once('.') else {
            return false;
        };
        !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) && !suffix.is_empty()
    } else {
        host.parse::<std::net::Ipv6Addr>().is_ok()
    }
}

/// Split `scheme://netloc` origins into exact entries and per-scheme
/// subdomain suffixes (`csrf_trusted_origins_hosts` /
/// `allowed_origins_exact` / `allowed_origin_subdomains`).
fn split_trusted_origins(trusted: &[String]) -> (HashSet<String>, HashMap<String, Vec<String>>) {
    let mut exact = HashSet::new();
    let mut subdomains: HashMap<String, Vec<String>> = HashMap::new();
    for origin in trusted {
        if !origin.contains('*') {
            exact.insert(origin.clone());
            continue;
        }
        if let Some(parsed) = urlparse(origin) {
            subdomains
                .entry(parsed.scheme)
                .or_default()
                .push(parsed.netloc.trim_start_matches('*').to_owned());
        }
    }
    (exact, subdomains)
}

/// `csrf_trusted_origins_hosts` (`csrf.py:180-184`): every trusted
/// origin's netloc with leading wildcards removed.
fn trusted_origin_hosts(trusted: &[String]) -> Vec<String> {
    trusted
        .iter()
        .filter_map(|origin| urlparse(origin))
        .map(|parsed| parsed.netloc.trim_start_matches('*').to_owned())
        .collect()
}

/// `_origin_verified` (`csrf.py:276-300`): same-origin, exact trusted,
/// or trusted-subdomain. The good origin's scheme follows `is_secure()`.
pub fn origin_verified(origin: &str, host: &str, secure: bool, trusted: &[String]) -> bool {
    if !host.is_empty() && origin == format!("{}://{host}", if secure { "https" } else { "http" }) {
        return true;
    }
    let (exact, subdomains) = split_trusted_origins(trusted);
    if exact.contains(origin) {
        return true;
    }
    let Some(parsed) = urlparse(origin) else {
        return false;
    };
    subdomains
        .get(&parsed.scheme)
        .map(|suffixes| {
            suffixes
                .iter()
                .any(|suffix| same_domain(&parsed.netloc, suffix))
        })
        .unwrap_or(false)
}

/// The port Django's `request.get_port()` would report: the Host header's
/// port, else the scheme default. `CSRF_COOKIE_DOMAIN` only consults it
/// for the non-default ports Django would append.
fn request_port(headers: &HeaderMap, secure: bool) -> String {
    if let Some(host) = headers.get("host").and_then(|value| value.to_str().ok()) {
        if let Some(port) = host_port(host) {
            return port;
        }
    }
    if secure { "443" } else { "80" }.to_owned()
}

fn host_port(host: &str) -> Option<String> {
    if let Some(inner) = host.strip_prefix('[') {
        let (_, after) = inner.split_once(']')?;
        return after
            .strip_prefix(':')
            .filter(|p| !p.is_empty())
            .map(str::to_owned);
    }
    let (_, port) = host.rsplit_once(':')?;
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(port.to_owned())
}

/// `_check_referer` (`csrf.py:302-345`): no Referer, malformed URL,
/// non-https scheme, a trusted-origin host, or the cookie domain / current
/// host via `is_same_domain` in that order, with Django's reason strings.
pub fn check_referer(
    headers: &HeaderMap,
    cookie_domain: Option<&str>,
    trusted: &[String],
) -> Result<(), String> {
    let Some(referer) = headers.get("referer").and_then(|value| value.to_str().ok()) else {
        return Err("Referer checking failed - no Referer.".to_owned());
    };
    let Some(parsed) = urlparse(referer) else {
        return Err("Referer checking failed - Referer is malformed.".to_owned());
    };
    if parsed.scheme != "https" {
        return Err(
            "Referer checking failed - Referer is insecure while host is secure.".to_owned(),
        );
    }
    if trusted_origin_hosts(trusted)
        .iter()
        .any(|host| same_domain(&parsed.netloc, host))
    {
        return Ok(());
    }
    let good_referer = match cookie_domain {
        Some(domain) => {
            let port = request_port(headers, true);
            if port == "443" || port == "80" {
                domain.to_owned()
            } else {
                format!("{domain}:{port}")
            }
        }
        None => headers
            .get("host")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned(),
    };
    if !same_domain(&parsed.netloc, &good_referer) {
        return Err(format!(
            "Referer checking failed - {} does not match any trusted origins.",
            parsed.geturl()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    const EVIL: &str =
        "Origin checking failed - https://evil.example does not match any trusted origins.";

    #[test]
    fn same_origin_passes_and_foreign_origin_fails() {
        let same = headers(&[
            ("host", "127.0.0.1:8000"),
            ("origin", "http://127.0.0.1:8000"),
        ]);
        assert_eq!(check_origin_and_referer(&same, false, &[], None), Ok(()));
        let evil = headers(&[
            ("host", "127.0.0.1:8000"),
            ("origin", "https://evil.example"),
        ]);
        assert_eq!(
            check_origin_and_referer(&evil, false, &[], None),
            Err(EVIL.to_owned())
        );
        // A valid-looking Referer never rescues a failed Origin.
        let evil_with_referer = headers(&[
            ("host", "127.0.0.1:8000"),
            ("origin", "https://evil.example"),
            ("referer", "http://127.0.0.1:8000/"),
        ]);
        assert_eq!(
            check_origin_and_referer(&evil_with_referer, true, &[], None),
            Err(EVIL.to_owned())
        );
    }

    #[test]
    fn null_origin_fails_with_django_reason() {
        let map = headers(&[("host", "app.test"), ("origin", "null")]);
        assert_eq!(
            check_origin_and_referer(&map, false, &[], None),
            Err("Origin checking failed - null does not match any trusted origins.".to_owned())
        );
    }

    #[test]
    fn scheme_follows_is_secure() {
        let map = headers(&[("host", "app.test"), ("origin", "http://app.test")]);
        assert!(check_origin_and_referer(&map, true, &[], None).is_err());
        let map = headers(&[("host", "app.test"), ("origin", "https://app.test")]);
        assert!(check_origin_and_referer(&map, true, &[], None).is_ok());
        assert!(check_origin_and_referer(&map, false, &[], None).is_err());
    }

    #[test]
    fn trusted_origins_exact_and_wildcard() {
        let trusted = vec![
            "https://web.example.com".to_owned(),
            "https://*.corp.example".to_owned(),
        ];
        for origin in [
            "https://web.example.com",
            "https://a.corp.example",
            "https://corp.example",
        ] {
            let map = headers(&[("host", "api.test"), ("origin", origin)]);
            assert_eq!(
                check_origin_and_referer(&map, false, &trusted, None),
                Ok(()),
                "{origin}"
            );
        }
        for origin in [
            "http://web.example.com",
            "http://a.corp.example",
            "https://evilcorp.example",
        ] {
            let map = headers(&[("host", "api.test"), ("origin", origin)]);
            assert!(
                check_origin_and_referer(&map, false, &trusted, None).is_err(),
                "{origin}"
            );
        }
    }

    #[test]
    fn missing_origin_skips_on_http_and_checks_referer_on_https() {
        let bare = headers(&[("host", "app.test")]);
        assert_eq!(check_origin_and_referer(&bare, false, &[], None), Ok(()));
        assert_eq!(
            check_origin_and_referer(&bare, true, &[], None),
            Err("Referer checking failed - no Referer.".to_owned())
        );
        let good = headers(&[("host", "app.test"), ("referer", "https://app.test/x")]);
        assert_eq!(check_origin_and_referer(&good, true, &[], None), Ok(()));
        let insecure = headers(&[("host", "app.test"), ("referer", "http://app.test/x")]);
        assert_eq!(
            check_origin_and_referer(&insecure, true, &[], None),
            Err("Referer checking failed - Referer is insecure while host is secure.".to_owned())
        );
        let foreign = headers(&[("host", "app.test"), ("referer", "https://evil.example/x")]);
        assert_eq!(
            check_origin_and_referer(&foreign, true, &[], None),
            Err(
                "Referer checking failed - https://evil.example/x does not match any trusted origins."
                    .to_owned()
            )
        );
        // `CSRF_COOKIE_DOMAIN` widens the good referer to the cookie domain.
        let sibling = headers(&[
            ("host", "api.example.com"),
            ("referer", "https://app.example.com/"),
        ]);
        assert_eq!(
            check_origin_and_referer(&sibling, true, &[], Some(".example.com")),
            Ok(())
        );
    }

    #[test]
    fn non_ascii_origin_is_rejected_not_skipped() {
        let mut map = headers(&[("host", "app.test")]);
        map.insert(
            "origin",
            axum::http::HeaderValue::from_bytes(b"https://\xe9.example").unwrap(),
        );
        assert_eq!(
            check_origin_and_referer(&map, false, &[], None),
            Err("Origin checking failed - https://\u{e9}.example does not match any trusted origins."
                .to_owned())
        );
    }

    #[test]
    fn request_is_secure_matches_django() {
        let map = headers(&[("x-forwarded-proto", "https")]);
        assert!(!request_is_secure(None, &map, false));
        assert!(request_is_secure(None, &map, true));
        assert!(request_is_secure(Some("https"), &HeaderMap::new(), false));
        // First comma-separated value, stripped, compared exactly.
        assert!(request_is_secure(
            None,
            &headers(&[("x-forwarded-proto", " https , http")]),
            true
        ));
        assert!(!request_is_secure(
            None,
            &headers(&[("x-forwarded-proto", "http, https")]),
            true
        ));
        assert!(!request_is_secure(
            None,
            &headers(&[("x-forwarded-proto", "HTTPS")]),
            true
        ));
        assert!(!request_is_secure(None, &HeaderMap::new(), true));
    }
}
