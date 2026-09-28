//! Work-item link crawler: title + favicon pipeline (D-08, services layer).
//!
//! Port of `apps/api/pi_dash/bgtasks/work_item_link_task.py:24-260`
//! (`DEFAULT_FAVICON`, `validate_url_ip`, `safe_get`,
//! `crawl_work_item_link_title_and_favicon`, `find_favicon_url`,
//! `fetch_and_encode_favicon`). The Celery entry point
//! (`crawl_work_item_link_title`, `:262-273`) lives in `pidash-jobs`
//! (`tasks_webhooks::link_crawl`); this module owns the pure pipeline it
//! drives. Fixture: `rust-api/fixtures/tasks_webhooks/fx-link-01-crawler.json`
//! (FX-LINK-01).
//!
//! Everything here is pure over injected seams: DNS resolution, HTTP GET
//! and HTTP HEAD arrive as closures, so the SSRF matrix, the redirect loop
//! and the favicon fallback are unit-testable without a network. The only
//! I/O the pipeline itself owns is none.
//!
//! Translation notes (translate, don't redesign):
//!
//! * Deny set is exactly `is_private | is_loopback | is_reserved |
//!   is_link_local` — `is_multicast`/`is_unspecified` are NOT checked,
//!   ported as-is per the fixture.
//! * The redirect loop checks `redirect_count >= 5` per hop, so 5 redirects
//!   followed by a 200 succeed (pinned by the unit oracle); the 6th
//!   redirect raises `Too many redirects for URL: <original url>`.
//! * A redirect with no `Location` header breaks out and returns the
//!   redirect response as-is (transcribed; no Python test covers it).
//! * `find_favicon_url` tries the four `link[rel=…]` selectors in order and
//!   the first tag *with* an `href` wins; its `href` is resolved with
//!   `urljoin` against the post-redirect URL and a *private* href raises
//!   out of the function (no guard on that branch).
//! * The `/favicon.ico` fallback is returned only on HEAD 200; a HEAD
//!   transport failure returns `None`, while a `validate_url_ip` failure
//!   on the fallback propagates to `fetch_and_encode_favicon`, whose broad
//!   guard turns ANY failure into the default favicon with `favicon_url`
//!   `None`.
//! * The crawl result carries the ORIGINAL input `url`, never the
//!   post-redirect URL; the error shape has no `favicon_url` key.

use std::net::IpAddr;

use serde_json::{Map, Value};

/// Celery task name (`work_item_link_task.py:262`, `@shared_task`).
pub const CRAWL_TASK_NAME: &str = "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title";

/// Default favicon (`work_item_link_task.py:24`, lucide link-icon SVG,
/// base64, 500 chars). Served as `data:image/svg+xml;base64,…`.
pub const DEFAULT_FAVICON: &str = "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyNCIgaGVpZ2h0PSIyNCIgdmlld0JveD0iMCAwIDI0IDI0IiBmaWxsPSJub25lIiBzdHJva2U9ImN1cnJlbnRDb2xvciIgc3Ryb2tlLXdpZHRoPSIyIiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiIGNsYXNzPSJsdWNpZGUgbHVjaWRlLWxpbmstaWNvbiBsdWNpZGUtbGluayI+PHBhdGggZD0iTTEwIDEzYTUgNSAwIDAgMCA3LjU0LjU0bDMtM2E1IDUgMCAwIDAtNy4wNy03LjA3bC0xLjcyIDEuNzEiLz48cGF0aCBkPSJNMTQgMTFhNSA1IDAgMCAwLTcuNTQtLjU0bC0zIDNhNSA1IDAgMCAwIDcuMDcgNy4wN2wxLjcxLTEuNzEiLz48L3N2Zz4=";

/// Browser User-Agent sent on the title fetch (`:131-133`).
pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";

/// Redirect budget (`MAX_REDIRECTS = 5`, `:69`).
pub const MAX_REDIRECTS: u32 = 5;

/// GET timeout in seconds (`safe_get` default `timeout=1`, `:75`).
pub const GET_TIMEOUT_SECS: u64 = 1;

/// `/favicon.ico` HEAD timeout in seconds (`:209`).
pub const FAVICON_HEAD_TIMEOUT_SECS: u64 = 2;

/// Fallback content type when the favicon response carries none (`:244`).
pub const DEFAULT_FAVICON_CONTENT_TYPE: &str = "image/x-icon";

/// `link[rel=…]` selectors tried in order (`:180-185`).
pub const FAVICON_SELECTORS: [&str; 4] = [
    "icon",
    "shortcut icon",
    "apple-touch-icon",
    "apple-touch-icon-precomposed",
];

// ---------------------------------------------------------------------------
// URL helpers (minimal `urlparse`/`urljoin` surface this port needs)
// ---------------------------------------------------------------------------

/// Lower-cased scheme of `url` (`""` when there is none).
///
/// `file:///etc/passwd` → `"file"`; `https://example.com/x` → `"https"`.
/// Mirrors `urlparse(url).scheme`, which lower-cases the scheme.
pub fn url_scheme(url: &str) -> String {
    let rest = match url.find("://") {
        Some(i) => return url[..i].to_ascii_lowercase(),
        None => url,
    };
    match rest.find(':') {
        Some(i) => rest[..i].to_ascii_lowercase(),
        None => String::new(),
    }
}

/// Lower-cased hostname of `url`, or `None` when there is none.
///
/// Strips userinfo (`user@`), ports (`:8080`) and IPv6 brackets, mirroring
/// `urlparse(url).hostname` (which also lower-cases).
pub fn url_hostname(url: &str) -> Option<String> {
    let sep = url.find("://")?;
    let after = &url[sep + 3..];
    let authority = after
        .find(['/', '?', '#'])
        .map(|i| &after[..i])
        .unwrap_or(after);
    if authority.is_empty() {
        return None;
    }
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host.starts_with('[') {
        match host.find(']') {
            Some(end) => &host[1..end],
            None => host,
        }
    } else {
        match host.find(':') {
            Some(i) => &host[..i],
            None => host,
        }
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// `scheme://authority` of `url`, or `None` when either half is missing.
/// Used for the `/favicon.ico` fallback (`:205-206`).
pub fn url_origin(url: &str) -> Option<String> {
    let scheme_end = url.find("://")?;
    let scheme = &url[..scheme_end];
    let after = &url[scheme_end + 3..];
    let authority = after
        .find(['/', '?', '#'])
        .map(|i| &after[..i])
        .unwrap_or(after);
    if scheme.is_empty() || authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{authority}"))
}

/// Minimal `urljoin(base, rel)` for this port's cases: absolute URLs pass
/// through, scheme-relative (`//host/path`) inherits the base scheme,
/// root-relative (`/path`) resolves against the origin, and anything else
/// merges onto the base directory.
pub fn urljoin(base: &str, rel: &str) -> String {
    if !url_scheme(rel).is_empty() && rel.contains("://") {
        return rel.to_owned();
    }
    if let Some(stripped) = rel.strip_prefix("//") {
        let scheme_end = base.find("://").map(|i| &base[..i]).unwrap_or("https");
        return format!("{scheme_end}://{stripped}");
    }
    if let Some(origin) = url_origin(base) {
        if rel.starts_with('/') {
            return format!("{origin}{rel}");
        }
        let dir = match base.rfind('/') {
            Some(i) if i >= origin.len() => &base[..=i],
            _ => &format!("{origin}/"),
        };
        return format!("{dir}{rel}");
    }
    rel.to_owned()
}

// ---------------------------------------------------------------------------
// SSRF guard (`validate_url_ip`, `:27-66`)
// ---------------------------------------------------------------------------

/// How DNS resolution can fail, mirroring the two `getaddrinfo` branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsFailure {
    /// `socket.gaierror` → `"Hostname could not be resolved"`.
    Unresolvable,
    /// Empty address list → `"No IP addresses found for the hostname"`.
    NoAddresses,
}

/// The SSRF denial reasons, each rendering the exact Python message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidateError {
    /// `"Invalid URL scheme. Only HTTP and HTTPS are allowed"`.
    Scheme,
    /// `"Invalid URL: No hostname found"`.
    NoHostname,
    /// `"Hostname could not be resolved"`.
    Unresolvable,
    /// `"No IP addresses found for the hostname"`.
    NoAddresses,
    /// `"Access to private/internal networks is not allowed"`.
    Private,
}

impl ValidateError {
    /// The exact `ValueError` text the Python guard raises.
    pub fn message(&self) -> &'static str {
        match self {
            ValidateError::Scheme => "Invalid URL scheme. Only HTTP and HTTPS are allowed",
            ValidateError::NoHostname => "Invalid URL: No hostname found",
            ValidateError::Unresolvable => "Hostname could not be resolved",
            ValidateError::NoAddresses => "No IP addresses found for the hostname",
            ValidateError::Private => "Access to private/internal networks is not allowed",
        }
    }
}

/// True when `ip` is in the denied set: `is_private | is_loopback |
/// is_reserved | is_link_local`. `is_multicast`/`is_unspecified` are NOT
/// checked, ported as-is per FX-LINK-01.
pub fn is_denied_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v4_denied(v.octets()),
        IpAddr::V6(v) => {
            // IPv4-mapped addresses (`::ffff:192.168.0.1`) are judged by
            // their inner IPv4 address, like the Python check would be.
            if let Some(mapped) = v.to_ipv4_mapped() {
                return v4_denied(mapped.octets());
            }
            v6_denied(v.segments(), v.is_unspecified(), v.is_multicast())
        }
    }
}

/// IPv4 deny check on raw octets (`is_reserved` is unstable on this
/// toolchain, so the `240.0.0.0/4` reserved range is a manual prefix
/// check alongside the stable `is_private`/`is_loopback`/`is_link_local`).
fn v4_denied(octets: [u8; 4]) -> bool {
    let addr = std::net::Ipv4Addr::from(octets);
    addr.is_private() || addr.is_loopback() || addr.is_link_local() || octets[0] & 0xf0 == 0xf0
}

/// IPv6 deny check on raw segments (the `Ipv6Addr` classifier helpers for
/// unique-local / link-local / global are unstable on this toolchain, so
/// the Python `is_private | is_loopback | is_reserved | is_link_local`
/// set is spelled out as prefix checks; `is_multicast`/`is_unspecified`
/// stay allowed per the fixture).
fn v6_denied(segments: [u16; 8], is_unspecified: bool, is_multicast: bool) -> bool {
    // `::1`.
    if segments == [0, 0, 0, 0, 0, 0, 0, 1] {
        return true;
    }
    // Unique-local `fc00::/7` (Python `is_private`).
    if segments[0] & 0xfe00 == 0xfc00 {
        return true;
    }
    // Link-local `fe80::/10` (Python `is_link_local`).
    if segments[0] & 0xffc0 == 0xfe80 {
        return true;
    }
    // Documentation `2001:db8::/32` (Python `is_reserved`).
    if segments[0] == 0x2001 && segments[1] == 0x0db8 {
        return true;
    }
    // Anything else outside global unicast `2000::/3` is reserved, except
    // the unspecified `::` and multicast `ff00::/8`, which stay allowed.
    if !is_unspecified && !is_multicast && segments[0] & 0xe000 != 0x2000 {
        return true;
    }
    false
}

/// SSRF guard (`validate_url_ip`, `:27-66`): scheme first (so
/// `file:///etc/passwd` reports the scheme, not a missing hostname), then
/// hostname presence, then DNS, then EVERY resolved address against the
/// deny set.
pub fn validate_url_ip(
    url: &str,
    resolve: &dyn Fn(&str) -> Result<Vec<IpAddr>, DnsFailure>,
) -> Result<(), ValidateError> {
    let scheme = url_scheme(url);
    if scheme != "http" && scheme != "https" {
        return Err(ValidateError::Scheme);
    }
    let hostname = url_hostname(url).ok_or(ValidateError::NoHostname)?;
    let addrs = resolve(&hostname).map_err(|failure| match failure {
        DnsFailure::Unresolvable => ValidateError::Unresolvable,
        DnsFailure::NoAddresses => ValidateError::NoAddresses,
    })?;
    if addrs.is_empty() {
        return Err(ValidateError::NoAddresses);
    }
    for addr in &addrs {
        if is_denied_ip(*addr) {
            return Err(ValidateError::Private);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP seam
// ---------------------------------------------------------------------------

/// Case-insensitive header map. `requests` headers are case-insensitive,
/// so `content-type` (Python `:244`) matches a `Content-Type` response
/// header here too.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpHeaders(pub Vec<(String, String)>);

impl HttpHeaders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn content_type(&self) -> &str {
        self.get("content-type")
            .unwrap_or(DEFAULT_FAVICON_CONTENT_TYPE)
    }

    pub fn location(&self) -> Option<&str> {
        self.get("location")
    }
}

/// One HTTP response. `is_redirect` is set by the caller/adapter the way
/// `requests.Response.is_redirect` reports it (redirect status *with* a
/// `Location` header); the loop additionally tolerates a redirect flag
/// without a location by returning that response as-is (`:102-103`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HttpHeaders,
    pub body: Vec<u8>,
    pub is_redirect: bool,
}

/// A redirect hop: request line the adapter executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout_secs: u64,
}

/// What `safe_get` can report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafeGetError {
    /// Any hop's `validate_url_ip` denial; the message is the exact
    /// Python `ValueError` text.
    Invalid(ValidateError),
    /// `redirect_count >= MAX_REDIRECTS` on a further redirect; the
    /// message names the ORIGINAL url (`:101`).
    TooManyRedirects { url: String },
    /// Transport failure (`requests.RequestException`).
    Fetch(String),
}

impl SafeGetError {
    /// Human text used in the title-phase warning (`:149-152`).
    pub fn message(&self) -> String {
        match self {
            SafeGetError::Invalid(validation) => validation.message().to_owned(),
            SafeGetError::TooManyRedirects { url } => {
                format!("Too many redirects for URL: {url}")
            }
            SafeGetError::Fetch(detail) => detail.clone(),
        }
    }

    /// True for the `ValueError`/`RuntimeError` branch (`:151-152`) as
    /// opposed to the `RequestException` branch (`:149-150`).
    pub fn is_validation(&self) -> bool {
        !matches!(self, SafeGetError::Fetch(_))
    }
}

/// Redirect-safe GET (`safe_get`, `:72-114`): validates the initial URL,
/// issues one `allow_redirects=False` GET per hop, re-validates every hop
/// AFTER `urljoin` (relative `Location` resolved), and returns the final
/// response with the final URL.
pub fn safe_get(
    url: &str,
    headers: &[(String, String)],
    timeout_secs: u64,
    validate: &dyn Fn(&str) -> Result<(), ValidateError>,
    get: &dyn Fn(&HttpRequest) -> Result<HttpResponse, String>,
) -> Result<(HttpResponse, String), SafeGetError> {
    validate(url).map_err(SafeGetError::Invalid)?;

    let mut current_url = url.to_owned();
    let mut response = get(&HttpRequest {
        url: current_url.clone(),
        headers: headers.to_vec(),
        timeout_secs,
    })
    .map_err(SafeGetError::Fetch)?;

    let mut redirect_count: u32 = 0;
    while response.is_redirect {
        if redirect_count >= MAX_REDIRECTS {
            return Err(SafeGetError::TooManyRedirects {
                url: url.to_owned(),
            });
        }
        let redirect_url = match response.headers.location() {
            Some(location) => location.to_owned(),
            None => break,
        };
        current_url = urljoin(&current_url, &redirect_url);
        validate(&current_url).map_err(SafeGetError::Invalid)?;
        redirect_count += 1;
        response = get(&HttpRequest {
            url: current_url.clone(),
            headers: headers.to_vec(),
            timeout_secs,
        })
        .map_err(SafeGetError::Fetch)?;
    }

    Ok((response, current_url))
}

// ---------------------------------------------------------------------------
// HTML extraction (BeautifulSoup surface this port needs)
// ---------------------------------------------------------------------------

/// Lower-case ASCII tag scan: finds `<name` (word-boundary) case-insensitively.
fn find_open_tag(html: &str, name: &str) -> Option<usize> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("<{name}");
    let mut start = 0;
    while let Some(i) = lower[start..].find(&needle) {
        let pos = start + i;
        let after = lower.as_bytes().get(pos + needle.len());
        match after {
            Some(b) if b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_' => {
                start = pos + 1;
            }
            _ => return Some(pos),
        }
    }
    None
}

/// End of the tag opened at `open` (the `>` closing it), respecting single
/// and double quotes. `None` when the tag never closes.
fn tag_end(html: &str, open: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = open;
    while i < bytes.len() {
        let byte = bytes[i];
        match quote {
            Some(q) => {
                if byte == q {
                    quote = None;
                }
            }
            None => {
                if byte == b'"' || byte == b'\'' {
                    quote = Some(byte);
                } else if byte == b'>' {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// Attributes of the tag `html[open..=end]` (names lower-cased, first
/// occurrence wins, mirroring BeautifulSoup duplicate-attribute behavior).
fn tag_attrs(html: &str, open: usize, end: usize) -> Vec<(String, String)> {
    let tag = &html[open..=end];
    let bytes = tag.as_bytes();
    let mut i = 1;
    // Skip the tag name.
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' && bytes[i] != b'>'
    {
        i += 1;
    }
    let mut attrs = Vec::new();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'>' || bytes[i] == b'/' {
            i += 1;
            continue;
        }
        let name_start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_' | b':' | b'.'))
        {
            i += 1;
        }
        if i == name_start {
            i += 1;
            continue;
        }
        let name = tag[name_start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let value_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                value = tag[value_start..i].to_owned();
                i += 1;
            } else {
                let value_start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                value = tag[value_start..i].to_owned();
            }
        }
        if !attrs.iter().any(|(existing, _)| *existing == name) {
            attrs.push((name, value));
        }
    }
    attrs
}

/// Stripped `<title>` text (`soup.find("title").get_text().strip()`,
/// `:144-145`), or `None` when there is no title tag. `get_text`
/// concatenates descendant text (nested markup dropped) and decodes
/// entities; both are mirrored here.
pub fn extract_title(html: &str) -> Option<String> {
    let open = find_open_tag(html, "title")?;
    let end = tag_end(html, open)?;
    let lower = html.to_ascii_lowercase();
    let rest = &lower[end..];
    let close = rest.find("</title").map(|i| end + i).unwrap_or(html.len());
    Some(
        decode_entities(&strip_tags(&html[end + 1..close]))
            .trim()
            .to_owned(),
    )
}

/// Drops `<…>` spans, mirroring `get_text()` concatenation.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(char) = chars.next() {
        if char == '<' {
            for inner in chars.by_ref() {
                if inner == '>' {
                    break;
                }
            }
        } else {
            out.push(char);
        }
    }
    out
}

/// Decodes the entities BeautifulSoup resolves in titles: the five
/// predefined XML entities plus decimal/hex character references.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(semi) = rest.find(';') {
        let Some(hash) = rest[..semi].rfind('&') else {
            break;
        };
        let body = &rest[hash + 1..semi];
        let decoded = if let Some(hex) = body.strip_prefix("#x").or(body.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(dec) = body.strip_prefix('#') {
            dec.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            match body {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => None,
            }
        };
        match decoded {
            Some(char) => {
                out.push_str(&rest[..hash]);
                out.push(char);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push_str(&rest[..semi + 1]);
                rest = &rest[semi + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `href` of the first `<link>` tag whose `rel` is exactly `rel_value`
/// (`soup.select_one('link[rel="…"]')` + `["href"]`, `:187-190`), or `None`.
pub fn find_link_href(html: &str, rel_value: &str) -> Option<String> {
    let mut start = 0;
    while start < html.len() {
        let slice = &html[start..];
        let open = find_open_tag(slice, "link")? + start;
        let end = tag_end(html, open).unwrap_or(html.len().saturating_sub(1));
        let attrs = tag_attrs(html, open, end.min(html.len().saturating_sub(1)));
        let rel = attrs
            .iter()
            .find(|(name, _)| name == "rel")
            .map(|(_, value)| value.as_str());
        if rel == Some(rel_value) {
            if let Some(href) = attrs
                .iter()
                .find(|(name, _)| name == "href")
                .map(|(_, value)| value.as_str())
            {
                if !href.is_empty() {
                    return Some(href.to_owned());
                }
            }
        }
        start = end + 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Favicon discovery + encoding (`find_favicon_url`, `fetch_and_encode_favicon`)
// ---------------------------------------------------------------------------

/// What `find_favicon_url` reports. A HEAD transport failure is NOT an
/// error here — it yields `None` (`:213-215`); only an SSRF denial
/// propagates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaviconFindError {
    Invalid(ValidateError),
}

/// Favicon URL discovery (`find_favicon_url`, `:173-218`): the four link
/// selectors in order (a private `href` raises out — no guard), then the
/// `/favicon.ico` HEAD fallback, returned ONLY on status 200.
pub fn find_favicon_url(
    html: Option<&str>,
    base_url: &str,
    validate: &dyn Fn(&str) -> Result<(), ValidateError>,
    head: &dyn Fn(&HttpRequest) -> Result<HttpResponse, String>,
    warnings: &mut Vec<String>,
) -> Result<Option<String>, FaviconFindError> {
    if let Some(page) = html {
        for rel in FAVICON_SELECTORS {
            if let Some(href) = find_link_href(page, rel) {
                let absolute = urljoin(base_url, &href);
                validate(&absolute).map_err(FaviconFindError::Invalid)?;
                return Ok(Some(absolute));
            }
        }
    }

    let origin = url_origin(base_url).unwrap_or_else(|| base_url.to_owned());
    let fallback_url = format!("{origin}/favicon.ico");
    validate(&fallback_url).map_err(FaviconFindError::Invalid)?;
    match head(&HttpRequest {
        url: fallback_url.clone(),
        headers: Vec::new(),
        timeout_secs: FAVICON_HEAD_TIMEOUT_SECS,
    }) {
        Ok(response) if response.status == 200 => Ok(Some(fallback_url)),
        Ok(_) => Ok(None),
        // `except requests.RequestException: log_exception(e, warning=True)`
        // (`:213-215`): the probe failure is logged, discovery misses.
        Err(detail) => {
            warnings.push(format!("Favicon HEAD probe failed: {detail}"));
            Ok(None)
        }
    }
}

/// Base64 alphabet (`base64.b64encode`, `:247`).
const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 encoding with padding.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut triple: u32 = 0;
        for (i, byte) in chunk.iter().enumerate() {
            triple |= (*byte as u32) << (16 - 8 * i);
        }
        let pad = 3 - chunk.len();
        for i in 0..4 - pad {
            let sextet = ((triple >> (18 - 6 * i)) & 0x3f) as usize;
            out.push(B64_ALPHABET[sextet] as char);
        }
        for _ in 0..pad {
            out.push('=');
        }
    }
    out
}

/// Default favicon payload (`:233-236`): no discovery URL, svg data-uri.
pub fn default_favicon() -> FaviconResult {
    FaviconResult {
        favicon_url: None,
        favicon_base64: format!("data:image/svg+xml;base64,{DEFAULT_FAVICON}"),
    }
}

/// `fetch_and_encode_favicon` result (`:219-260`): `favicon_url` +
/// `favicon_base64` data-uri.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaviconResult {
    pub favicon_url: Option<String>,
    pub favicon_base64: String,
}

impl FaviconResult {
    /// Renders the exact `{"favicon_url", "favicon_base64"}` dict.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "favicon_url".to_owned(),
            self.favicon_url
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        map.insert(
            "favicon_base64".to_owned(),
            Value::String(self.favicon_base64.clone()),
        );
        Value::Object(map)
    }
}

/// Fetch + base64 the favicon (`fetch_and_encode_favicon`, `:219-260`):
/// `None` discovery → default payload; otherwise `safe_get` on the favicon
/// URL with the response content type; ANY failure → warning + default
/// payload with `favicon_url` `None`.
pub fn fetch_and_encode_favicon(
    html: Option<&str>,
    base_url: &str,
    headers: &[(String, String)],
    validate: &dyn Fn(&str) -> Result<(), ValidateError>,
    get: &dyn Fn(&HttpRequest) -> Result<HttpResponse, String>,
    head: &dyn Fn(&HttpRequest) -> Result<HttpResponse, String>,
    warnings: &mut Vec<String>,
) -> FaviconResult {
    let favicon_url = match find_favicon_url(html, base_url, validate, head, warnings) {
        Ok(found) => found,
        Err(FaviconFindError::Invalid(validation)) => {
            warnings.push(format!("Failed to fetch favicon: {}", validation.message()));
            return default_favicon();
        }
    };
    let Some(url) = favicon_url else {
        return default_favicon();
    };
    match safe_get(url.as_str(), headers, GET_TIMEOUT_SECS, validate, get) {
        Ok((response, _)) => {
            let content_type = response.headers.content_type().to_owned();
            let encoded = base64_encode(&response.body);
            FaviconResult {
                favicon_url: Some(url),
                favicon_base64: format!("data:{content_type};base64,{encoded}"),
            }
        }
        Err(error) => {
            let detail = match error {
                SafeGetError::Invalid(validation) => validation.message().to_owned(),
                SafeGetError::TooManyRedirects { url } => {
                    format!("Too many redirects for URL: {url}")
                }
                SafeGetError::Fetch(detail) => detail,
            };
            warnings.push(format!("Failed to fetch favicon: {detail}"));
            default_favicon()
        }
    }
}

// ---------------------------------------------------------------------------
// Crawl entry point (`crawl_work_item_link_title_and_favicon`, `:118-172`)
// ---------------------------------------------------------------------------

/// Successful crawl result: `title`, `favicon` data-uri, ORIGINAL input
/// `url`, `favicon_url` (`:159-164`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrawlResult {
    pub title: Option<String>,
    pub favicon: String,
    pub url: String,
    pub favicon_url: Option<String>,
}

impl CrawlResult {
    /// Renders the exact ok-shape dict (`:159-164`).
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "title".to_owned(),
            self.title.clone().map(Value::String).unwrap_or(Value::Null),
        );
        map.insert("favicon".to_owned(), Value::String(self.favicon.clone()));
        map.insert("url".to_owned(), Value::String(self.url.clone()));
        map.insert(
            "favicon_url".to_owned(),
            self.favicon_url
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        Value::Object(map)
    }
}

/// Outer-failure shape (`:168-173`): `error`/`title`/`favicon`/`url` —
/// notably WITHOUT a `favicon_url` key.
pub fn error_shape(url: &str, detail: &str) -> Value {
    let mut map = Map::new();
    map.insert(
        "error".to_owned(),
        Value::String(format!("Unexpected error: {detail}")),
    );
    map.insert("title".to_owned(), Value::Null);
    map.insert("favicon".to_owned(), Value::Null);
    map.insert("url".to_owned(), Value::String(url.to_owned()));
    Value::Object(map)
}

/// What the crawl produces, plus the warnings the Python run logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrawlReport {
    pub result: Value,
    pub warnings: Vec<String>,
}

/// Title + favicon crawl (`:118-172`): browser-UA title fetch whose
/// transport AND validation failures only warn (`title` stays `None` while
/// the favicon is still attempted), favicon resolved against the
/// post-redirect URL. Infallible by construction — every inner failure is
/// contained exactly where the Python guards contain it, so the outer
/// `except` (`:167-173`, rendered by [`error_shape`]) only guards callers
/// against unexpected failures.
pub fn crawl(
    url: &str,
    validate: &dyn Fn(&str) -> Result<(), ValidateError>,
    get: &dyn Fn(&HttpRequest) -> Result<HttpResponse, String>,
    head: &dyn Fn(&HttpRequest) -> Result<HttpResponse, String>,
) -> CrawlReport {
    let mut warnings = Vec::new();
    let headers = vec![("User-Agent".to_owned(), BROWSER_USER_AGENT.to_owned())];

    let mut title: Option<String> = None;
    let mut final_url = url.to_owned();
    let mut html: Option<String> = None;

    match safe_get(url, &headers, GET_TIMEOUT_SECS, validate, get) {
        Ok((response, resolved)) => {
            final_url = resolved;
            let page = String::from_utf8_lossy(&response.body).into_owned();
            title = extract_title(&page);
            html = Some(page);
        }
        Err(error) => {
            if error.is_validation() {
                warnings.push(format!("URL validation failed: {}", error.message()));
            } else {
                warnings.push(format!(
                    "Failed to fetch HTML for title: {}",
                    error.message()
                ));
            }
        }
    }

    let favicon = fetch_and_encode_favicon(
        html.as_deref(),
        &final_url,
        &headers,
        validate,
        get,
        head,
        &mut warnings,
    );

    let result = CrawlResult {
        title,
        favicon: favicon.favicon_base64,
        url: url.to_owned(),
        favicon_url: favicon.favicon_url,
    }
    .to_json();

    CrawlReport { result, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn public_resolve() -> impl Fn(&str) -> Result<Vec<IpAddr>, DnsFailure> {
        |_| Ok(vec!["93.184.216.34".parse().unwrap()])
    }

    fn resolve_map(
        map: HashMap<String, Vec<IpAddr>>,
    ) -> impl Fn(&str) -> Result<Vec<IpAddr>, DnsFailure> {
        move |host: &str| {
            Ok(map
                .get(host)
                .cloned()
                .unwrap_or_else(|| vec!["93.184.216.34".parse().unwrap()]))
        }
    }

    fn deny_all() -> impl Fn(&str) -> Result<(), ValidateError> {
        |url| validate_url_ip(url, &public_resolve())
    }

    fn plain_response() -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: HttpHeaders::new(),
            body: b"OK".to_vec(),
            is_redirect: false,
        }
    }

    fn redirect_response(location: &str) -> HttpResponse {
        HttpResponse {
            status: 302,
            headers: HttpHeaders(vec![("Location".to_owned(), location.to_owned())]),
            body: Vec::new(),
            is_redirect: true,
        }
    }

    // -- validate_url_ip matrix (FX-LINK-01 validate_url_ip_matrix) --------

    #[test]
    fn rejects_ftp_scheme() {
        let err = validate_url_ip("ftp://example.com/file", &public_resolve()).unwrap_err();
        assert_eq!(err, ValidateError::Scheme);
        assert_eq!(
            err.message(),
            "Invalid URL scheme. Only HTTP and HTTPS are allowed"
        );
    }

    #[test]
    fn scheme_check_fires_first_for_file_url() {
        // `file:///etc/passwd` has no hostname; the scheme error is
        // reported, not "no hostname" (`:38-41` + fixture order note).
        let err = validate_url_ip("file:///etc/passwd", &|_| {
            panic!("DNS must not run when the scheme already failed")
        })
        .unwrap_err();
        assert_eq!(err, ValidateError::Scheme);
    }

    #[test]
    fn rejects_missing_hostname() {
        let err = validate_url_ip("http:///no-host", &public_resolve()).unwrap_err();
        assert_eq!(err, ValidateError::NoHostname);
        assert_eq!(err.message(), "Invalid URL: No hostname found");
    }

    #[test]
    fn rejects_unresolvable_host() {
        let err =
            validate_url_ip("https://example.com", &|_| Err(DnsFailure::Unresolvable)).unwrap_err();
        assert_eq!(err, ValidateError::Unresolvable);
        assert_eq!(err.message(), "Hostname could not be resolved");
    }

    #[test]
    fn rejects_empty_address_list() {
        let err = validate_url_ip("https://example.com", &|_| Ok(vec![])).unwrap_err();
        assert_eq!(err, ValidateError::NoAddresses);
        assert_eq!(err.message(), "No IP addresses found for the hostname");
    }

    #[test]
    fn rejects_private_ip() {
        let err = validate_url_ip("http://example.com", &|_| {
            Ok(vec!["192.168.1.1".parse().unwrap()])
        })
        .unwrap_err();
        assert_eq!(err, ValidateError::Private);
        assert_eq!(
            err.message(),
            "Access to private/internal networks is not allowed"
        );
    }

    #[test]
    fn rejects_loopback() {
        let err = validate_url_ip("http://example.com", &|_| {
            Ok(vec!["127.0.0.1".parse().unwrap()])
        })
        .unwrap_err();
        assert_eq!(err, ValidateError::Private);
    }

    #[test]
    fn rejects_link_local_and_reserved() {
        // `169.254.169.254` (the redirect-target vector) is link-local;
        // `240.0.0.1` is reserved — both deny via their own flags.
        for ip in ["169.254.169.254", "240.0.0.1", "::1", "fe80::1", "fc00::1"] {
            let err = validate_url_ip("http://example.com", &|_| Ok(vec![ip.parse().unwrap()]))
                .unwrap_err();
            assert_eq!(err, ValidateError::Private, "expected deny for {ip}");
        }
    }

    #[test]
    fn every_resolved_address_is_checked() {
        // One public + one private address still denies (`:63-66` loop).
        let err = validate_url_ip("http://example.com", &|_| {
            Ok(vec![
                "93.184.216.34".parse().unwrap(),
                "10.0.0.1".parse().unwrap(),
            ])
        })
        .unwrap_err();
        assert_eq!(err, ValidateError::Private);
    }

    #[test]
    fn allows_public_ip() {
        validate_url_ip("https://example.com", &public_resolve()).unwrap();
    }

    #[test]
    fn allows_multicast_and_unspecified_as_is() {
        // `is_multicast`/`is_unspecified` are NOT in the deny set.
        for ip in ["224.0.0.1", "0.0.0.0", "::", "ff02::1"] {
            validate_url_ip("http://example.com", &|_| Ok(vec![ip.parse().unwrap()]))
                .unwrap_or_else(|err| panic!("{ip} must pass, got {err:?}"));
        }
    }

    // -- safe_get matrix (FX-LINK-01 safe_get_matrix + unit oracle) --------

    #[test]
    fn plain_get_validates_once_and_keeps_url() {
        let headers = vec![("User-Agent".to_owned(), BROWSER_USER_AGENT.to_owned())];
        let calls = std::cell::RefCell::new(Vec::new());
        let get = |req: &HttpRequest| {
            calls.borrow_mut().push((req.url.clone(), req.timeout_secs));
            // Headers pass through untouched (`allow_redirects=False` aside,
            // `safe_get` forwards what it was given).
            assert_eq!(req.headers, headers);
            Ok(plain_response())
        };
        let validated = std::cell::RefCell::new(Vec::new());
        let validate = |url: &str| {
            validated.borrow_mut().push(url.to_owned());
            Ok(())
        };
        // Default timeout is 1 (`:75`).
        let (response, final_url) = safe_get(
            "https://example.com",
            &headers,
            GET_TIMEOUT_SECS,
            &validate,
            &get,
        )
        .unwrap();
        assert_eq!(response, plain_response());
        assert_eq!(final_url, "https://example.com");
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(calls.borrow()[0].1, 1);
        assert_eq!(*validated.borrow(), vec!["https://example.com"]);
    }

    #[test]
    fn follows_redirect_and_validates_each_hop() {
        let get = |req: &HttpRequest| {
            if req.url == "https://example.com" {
                Ok(redirect_response("https://other.com/page"))
            } else {
                Ok(plain_response())
            }
        };
        let validated = std::cell::RefCell::new(Vec::new());
        let validate = |url: &str| {
            validated.borrow_mut().push(url.to_owned());
            Ok(())
        };
        let (response, final_url) =
            safe_get("https://example.com", &[], 1, &validate, &get).unwrap();
        assert_eq!(response, plain_response());
        assert_eq!(final_url, "https://other.com/page");
        assert_eq!(
            *validated.borrow(),
            vec!["https://example.com", "https://other.com/page"]
        );
    }

    #[test]
    fn blocks_redirect_to_private_ip() {
        let get = |_: &HttpRequest| Ok(redirect_response("http://192.168.1.1:8080"));
        let validate = |url: &str| {
            if url == "https://evil.com/redirect" {
                Ok(())
            } else {
                Err(ValidateError::Private)
            }
        };
        let err = safe_get("https://evil.com/redirect", &[], 1, &validate, &get).unwrap_err();
        assert_eq!(err, SafeGetError::Invalid(ValidateError::Private));
        assert_eq!(
            err.message(),
            "Access to private/internal networks is not allowed"
        );
    }

    #[test]
    fn raises_on_too_many_redirects_with_original_url() {
        let get = |_: &HttpRequest| Ok(redirect_response("https://example.com/loop"));
        let err = safe_get("https://example.com/start", &[], 1, &deny_all(), &get).unwrap_err();
        assert_eq!(
            err,
            SafeGetError::TooManyRedirects {
                url: "https://example.com/start".to_owned()
            }
        );
        assert_eq!(
            err.message(),
            "Too many redirects for URL: https://example.com/start"
        );
        assert!(err.is_validation());
    }

    #[test]
    fn succeeds_at_exact_max_redirects() {
        // 5 redirects then a 200 (unit oracle pin).
        let calls = std::cell::Cell::new(0);
        let get = |_: &HttpRequest| {
            calls.set(calls.get() + 1);
            if calls.get() <= 5 {
                Ok(redirect_response("https://example.com/next"))
            } else {
                Ok(plain_response())
            }
        };
        let (response, _) =
            safe_get("https://example.com/start", &[], 1, &deny_all(), &get).unwrap();
        assert_eq!(response, plain_response());
        assert_eq!(calls.get(), 6);
    }

    #[test]
    fn redirect_without_location_returns_response_as_is() {
        let redirect = HttpResponse {
            status: 302,
            headers: HttpHeaders::new(),
            body: b"stuck".to_vec(),
            is_redirect: true,
        };
        let get = |_: &HttpRequest| Ok(redirect.clone());
        let (response, final_url) =
            safe_get("https://example.com", &[], 1, &deny_all(), &get).unwrap();
        assert_eq!(response.body, b"stuck");
        assert_eq!(final_url, "https://example.com");
    }

    #[test]
    fn resolves_relative_location_against_current_url() {
        let seen = std::cell::RefCell::new(Vec::new());
        let get = |req: &HttpRequest| {
            seen.borrow_mut().push(req.url.clone());
            if seen.borrow().len() == 1 {
                Ok(redirect_response("/next"))
            } else {
                Ok(plain_response())
            }
        };
        let (_, final_url) =
            safe_get("https://example.com/a/page", &[], 1, &deny_all(), &get).unwrap();
        assert_eq!(final_url, "https://example.com/next");
        assert_eq!(
            *seen.borrow(),
            vec!["https://example.com/a/page", "https://example.com/next"]
        );
    }

    // -- find_favicon vectors (FX-LINK-01 find_favicon) --------------------

    fn no_head(_: &HttpRequest) -> Result<HttpResponse, String> {
        panic!("HEAD must not run when a link tag wins")
    }

    #[test]
    fn link_tag_href_resolved_with_urljoin() {
        let html = r#"<html><head><link rel="icon" href="/fav.ico"></head></html>"#;
        let found = find_favicon_url(
            Some(html),
            "https://example.com/page",
            &deny_all(),
            &no_head,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(found, Some("https://example.com/fav.ico".to_owned()));
    }

    #[test]
    fn selector_order_beats_document_order() {
        // `shortcut icon` appears first in the document, but the `icon`
        // selector is tried first (`:186-191` outer loop over selectors).
        let html = r#"<html><head><link rel="shortcut icon" href="/short.ico"><link rel="icon" href="/icon.ico"></head></html>"#;
        let found = find_favicon_url(
            Some(html),
            "https://example.com/",
            &deny_all(),
            &no_head,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(found, Some("https://example.com/icon.ico".to_owned()));
    }

    #[test]
    fn first_tag_with_href_wins_and_hrefless_tags_skipped() {
        let html = r#"<html><head><link rel="icon"><link rel="icon" href="/b.ico"></head></html>"#;
        let found = find_favicon_url(
            Some(html),
            "https://example.com/",
            &deny_all(),
            &no_head,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(found, Some("https://example.com/b.ico".to_owned()));
    }

    #[test]
    fn private_href_raises_out_of_discovery() {
        // No guard on the link-tag branch (`:192-193`): with DNS resolving
        // the href host to a private IP, the denial propagates.
        let html = r#"<html><head><link rel="icon" href="http://evil.test/x.ico"></head></html>"#;
        let resolve = resolve_map(HashMap::from([(
            "evil.test".to_owned(),
            vec!["10.1.2.3".parse().unwrap()],
        )]));
        let validate = |url: &str| validate_url_ip(url, &resolve);
        let err = find_favicon_url(
            Some(html),
            "https://example.com/",
            &validate,
            &no_head,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert_eq!(err, FaviconFindError::Invalid(ValidateError::Private));
    }

    #[test]
    fn fallback_head_200_returns_favicon_ico() {
        let html = "<html><head></head><body>no icons</body></html>";
        let head = |req: &HttpRequest| {
            assert_eq!(req.url, "https://example.com/favicon.ico");
            assert_eq!(req.timeout_secs, FAVICON_HEAD_TIMEOUT_SECS);
            Ok(HttpResponse {
                status: 200,
                headers: HttpHeaders::new(),
                body: Vec::new(),
                is_redirect: false,
            })
        };
        let found = find_favicon_url(
            Some(html),
            "https://example.com/page",
            &deny_all(),
            &head,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(found, Some("https://example.com/favicon.ico".to_owned()));
    }

    #[test]
    fn fallback_head_404_returns_none() {
        let head = |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 404,
                headers: HttpHeaders::new(),
                body: Vec::new(),
                is_redirect: false,
            })
        };
        let found = find_favicon_url(
            None,
            "https://example.com/",
            &deny_all(),
            &head,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn fallback_head_transport_failure_returns_none() {
        let head = |_: &HttpRequest| Err("connection refused".to_owned());
        let mut warnings = Vec::new();
        let found = find_favicon_url(
            None,
            "https://example.com/",
            &deny_all(),
            &head,
            &mut warnings,
        )
        .unwrap();
        assert_eq!(found, None);
        // `log_exception(e, warning=True)` (`:213-215`): logged, then miss.
        assert_eq!(
            warnings,
            vec!["Favicon HEAD probe failed: connection refused"]
        );
    }

    // -- fetch_and_encode + crawl shapes (FX-LINK-01 crawl/fetch) ---------

    #[test]
    fn default_favicon_is_500_chars_with_svg_data_uri() {
        assert_eq!(DEFAULT_FAVICON.len(), 500);
        let result = default_favicon();
        assert_eq!(result.favicon_url, None);
        assert_eq!(
            result.favicon_base64,
            format!("data:image/svg+xml;base64,{DEFAULT_FAVICON}")
        );
    }

    #[test]
    fn missing_discovery_yields_default_payload() {
        let head = |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 404,
                headers: HttpHeaders::new(),
                body: Vec::new(),
                is_redirect: false,
            })
        };
        let mut warnings = Vec::new();
        let result = fetch_and_encode_favicon(
            None,
            "https://example.com/",
            &[],
            &deny_all(),
            &|_: &HttpRequest| panic!("no GET when discovery misses"),
            &head,
            &mut warnings,
        );
        assert_eq!(result, default_favicon());
        assert!(warnings.is_empty());
    }

    #[test]
    fn found_favicon_uses_response_content_type() {
        let html = r#"<html><head><link rel="icon" href="/f.png"></head></html>"#;
        let get = |req: &HttpRequest| {
            assert_eq!(req.url, "https://example.com/f.png");
            Ok(HttpResponse {
                status: 200,
                headers: HttpHeaders(vec![("Content-Type".to_owned(), "image/png".to_owned())]),
                body: vec![1, 2, 3],
                is_redirect: false,
            })
        };
        let mut warnings = Vec::new();
        let result = fetch_and_encode_favicon(
            Some(html),
            "https://example.com/",
            &[],
            &deny_all(),
            &get,
            &no_head,
            &mut warnings,
        );
        assert_eq!(
            result.favicon_url,
            Some("https://example.com/f.png".to_owned())
        );
        // `base64.b64encode(b"\x01\x02\x03") == "AQID"`.
        assert_eq!(result.favicon_base64, "data:image/png;base64,AQID");
        assert!(warnings.is_empty());
    }

    #[test]
    fn missing_content_type_defaults_to_x_icon() {
        let html = r#"<html><head><link rel="icon" href="/f.ico"></head></html>"#;
        let get = |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 200,
                headers: HttpHeaders::new(),
                body: b"ico".to_vec(),
                is_redirect: false,
            })
        };
        let mut warnings = Vec::new();
        let result = fetch_and_encode_favicon(
            Some(html),
            "https://example.com/",
            &[],
            &deny_all(),
            &get,
            &no_head,
            &mut warnings,
        );
        assert!(result
            .favicon_base64
            .starts_with("data:image/x-icon;base64,"));
    }

    #[test]
    fn private_favicon_url_falls_back_to_default_with_none_url() {
        // `safe_get` denies the private favicon host; the broad guard
        // warns and returns the default payload with `favicon_url` None.
        let html = r#"<html><head></head></html>"#;
        let resolve = resolve_map(HashMap::from([(
            "example.com".to_owned(),
            vec!["93.184.216.34".parse().unwrap()],
        )]));
        let validate = |url: &str| validate_url_ip(url, &resolve);
        // Discovery HEAD succeeds so the favicon URL is the fallback…
        let head = |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 200,
                headers: HttpHeaders::new(),
                body: Vec::new(),
                is_redirect: false,
            })
        };
        // …but the test rewrites DNS for the favicon host to private via a
        // stateful resolver: first pass (fallback validate) public, then
        // private for the safe_get re-validation.
        let seen = std::cell::Cell::new(0);
        let shifting = |url: &str| {
            seen.set(seen.get() + 1);
            if seen.get() > 1 {
                validate_url_ip(url, &|_| Ok(vec!["10.9.9.9".parse().unwrap()]))
            } else {
                validate(url)
            }
        };
        let mut warnings = Vec::new();
        let result = fetch_and_encode_favicon(
            Some(html),
            "https://example.com/",
            &[],
            &shifting,
            &|_: &HttpRequest| panic!("denied before any GET"),
            &head,
            &mut warnings,
        );
        assert_eq!(result, default_favicon());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("Failed to fetch favicon: "));
    }

    #[test]
    fn crawl_ok_shape_carries_original_url_and_stripped_title() {
        let page = "<html><head><title>  Hello World  </title></head><body></body></html>";
        let get = |req: &HttpRequest| {
            // Browser UA on the title fetch (`:131-133`).
            assert!(req
                .headers
                .iter()
                .any(|(k, v)| k == "User-Agent" && v == BROWSER_USER_AGENT));
            if req.url == "https://example.com/start" {
                Ok(HttpResponse {
                    status: 301,
                    headers: HttpHeaders(vec![("Location".to_owned(), "/landed".to_owned())]),
                    body: Vec::new(),
                    is_redirect: true,
                })
            } else {
                Ok(HttpResponse {
                    status: 200,
                    headers: HttpHeaders::new(),
                    body: page.as_bytes().to_vec(),
                    is_redirect: false,
                })
            }
        };
        let head = |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 404,
                headers: HttpHeaders::new(),
                body: Vec::new(),
                is_redirect: false,
            })
        };
        let report = crawl("https://example.com/start", &deny_all(), &get, &head);
        assert!(report.warnings.is_empty());
        let result = &report.result;
        assert_eq!(
            result.get("title"),
            Some(&Value::String("Hello World".to_owned()))
        );
        // ORIGINAL input url, not the post-redirect one.
        assert_eq!(
            result.get("url"),
            Some(&Value::String("https://example.com/start".to_owned()))
        );
        assert_eq!(result.get("favicon_url"), Some(&Value::Null));
        assert_eq!(
            result.get("favicon"),
            Some(&Value::String(default_favicon().favicon_base64))
        );
        // Exactly the four ok-shape keys.
        let mut keys: Vec<&str> = result
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["favicon", "favicon_url", "title", "url"]);
    }

    #[test]
    fn crawl_title_failure_still_attempts_favicon() {
        // Title GET fails at the transport: title stays None with a
        // "Failed to fetch HTML" warning, but the favicon discovery (here
        // the `/favicon.ico` fallback + favicon GET) still runs and wins.
        let calls = std::cell::Cell::new(0);
        let get = |req: &HttpRequest| {
            calls.set(calls.get() + 1);
            if req.url == "https://example.com/" {
                return Err("connection reset".to_owned());
            }
            assert_eq!(req.url, "https://example.com/favicon.ico");
            Ok(HttpResponse {
                status: 200,
                headers: HttpHeaders(vec![("content-type".to_owned(), "image/x-icon".to_owned())]),
                body: b"FAV".to_vec(),
                is_redirect: false,
            })
        };
        let head = |req: &HttpRequest| {
            assert_eq!(req.url, "https://example.com/favicon.ico");
            Ok(HttpResponse {
                status: 200,
                headers: HttpHeaders::new(),
                body: Vec::new(),
                is_redirect: false,
            })
        };
        let report = crawl("https://example.com/", &deny_all(), &get, &head);
        assert_eq!(report.result.get("title"), Some(&Value::Null));
        assert_eq!(
            report.result.get("url"),
            Some(&Value::String("https://example.com/".to_owned()))
        );
        assert_eq!(
            report.result.get("favicon_url"),
            Some(&Value::String("https://example.com/favicon.ico".to_owned()))
        );
        assert!(report
            .result
            .get("favicon")
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with("data:image/x-icon;base64,"));
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].starts_with("Failed to fetch HTML for title: "));
    }

    #[test]
    fn crawl_validation_failure_warns_and_keeps_original_base() {
        // Guard denial on the title fetch: "URL validation failed"
        // warning, title None, favicon resolved against the ORIGINAL url.
        let validate = |url: &str| {
            if url == "http://blocked.test/" {
                Err(ValidateError::Private)
            } else {
                Ok(())
            }
        };
        let head = |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 404,
                headers: HttpHeaders::new(),
                body: Vec::new(),
                is_redirect: false,
            })
        };
        let report = crawl(
            "http://blocked.test/",
            &validate,
            &|_: &HttpRequest| panic!("denied before any GET"),
            &head,
        );
        assert_eq!(report.result.get("title"), Some(&Value::Null));
        assert_eq!(
            report.result.get("favicon"),
            Some(&Value::String(default_favicon().favicon_base64))
        );
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].starts_with("URL validation failed: "));
    }

    #[test]
    fn title_get_text_strips_markup_and_decodes_entities() {
        // `get_text()` concatenates descendant text and resolves entities.
        let title =
            extract_title("<html><head><title>A &amp; B <b>bold</b>&#33;</title></head></html>");
        assert_eq!(title, Some("A & B bold!".to_owned()));
    }

    #[test]
    fn crawl_missing_title_tag_yields_null_title() {
        let get = |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 200,
                headers: HttpHeaders::new(),
                body: b"<html><body>no title</body></html>".to_vec(),
                is_redirect: false,
            })
        };
        let head = |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 404,
                headers: HttpHeaders::new(),
                body: Vec::new(),
                is_redirect: false,
            })
        };
        let report = crawl("https://example.com/", &deny_all(), &get, &head);
        assert_eq!(report.result.get("title"), Some(&Value::Null));
    }

    #[test]
    fn error_shape_has_no_favicon_url_key() {
        let shape = error_shape("https://example.com/", "boom");
        assert_eq!(
            shape.get("error"),
            Some(&Value::String("Unexpected error: boom".to_owned()))
        );
        assert_eq!(shape.get("title"), Some(&Value::Null));
        assert_eq!(shape.get("favicon"), Some(&Value::Null));
        assert_eq!(
            shape.get("url"),
            Some(&Value::String("https://example.com/".to_owned()))
        );
        assert!(shape.get("favicon_url").is_none());
    }

    #[test]
    fn crawl_task_name_matches_python() {
        assert_eq!(
            CRAWL_TASK_NAME,
            "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title"
        );
    }
}
