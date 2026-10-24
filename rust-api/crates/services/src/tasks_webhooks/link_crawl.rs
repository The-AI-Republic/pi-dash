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
//!   is_link_local` on CPython 3.12. `is_multicast` is NOT checked, so
//!   multicast addresses stay reachable. `is_unspecified` is not a
//!   separate predicate in Django, but CPython's `is_private` covers both
//!   `0.0.0.0` and `::`, so both are denied.
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
//! * Title decoding mirrors `UnicodeDammit` + `html.parser` + BS4's own
//!   entity handlers — not `html.unescape` (PIDASHCONV-1280): BOM, then
//!   the XML prolog / meta charset, then UTF-8, then windows-1252. The
//!   response `Content-Type` is never consulted (Django hands raw bytes
//!   to BeautifulSoup). Known gaps: no `charset-normalizer` detection
//!   step (undeclared non-UTF-8 pages are best-effort), no exotic
//!   codecs (hz, ptcp154, utf-7, ebcdic/dos code pages), superset
//!   approximations where WHATWG is wider than the declared codec
//!   (gb2312, big5-hkscs, tis-620, iso-8859-9/11, windows-125x
//!   undefined bytes, shift_jis/gb18030 lone 0x80), and surrogate
//!   code points read as U+FFFD (unrepresentable in UTF-8 strings).

use std::net::IpAddr;
use std::sync::LazyLock;

use encoding_rs::Encoding;
use serde_json::{Map, Value};

use super::html_entities;
use crate::assistant::ssrf::address_is_private_or_reserved;

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
/// is_reserved | is_link_local` on CPython 3.12. `is_multicast` is NOT
/// checked, so multicast stays allowed. `is_unspecified` is covered by
/// `is_private` for `0.0.0.0` and `::`, exactly as in Django.
pub fn is_denied_ip(ip: IpAddr) -> bool {
    address_is_private_or_reserved(ip)
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

/// The winning body decoder: the Rust half of
/// `UnicodeDammit.original_encoding`. It travels with the decoded page
/// because `handle_charref` resolves references below 256 through it.
#[derive(Debug, Clone, Copy)]
pub enum PageEncoding {
    /// Any `encoding_rs` codec (UTF-8, windows-1252, Shift_JIS, ...).
    Rs(&'static Encoding),
    /// Python `iso-8859-1` (pure latin-1). WHATWG aliases every
    /// `iso-8859-1` spelling to windows-1252, which maps `0x80-0x9F`
    /// to printable characters Django leaves as controls.
    Latin1,
    /// Bare `utf-16`: BOM-selected endianness, else little-endian
    /// (CPython's native default on every supported target).
    Utf16Bare,
    /// Bare `utf-32`: same BOM-or-LE rule (`encoding_rs` has no
    /// UTF-32, so the fixed-endian forms below are manual too).
    Utf32Bare,
    /// Python `utf-32be` / `utf-32le`, by hand.
    Utf32Be,
    Utf32Le,
}

impl PartialEq for PageEncoding {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PageEncoding::Rs(left), PageEncoding::Rs(right)) => left.name() == right.name(),
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

impl Eq for PageEncoding {}

impl PageEncoding {
    /// Strict decode: `None` on any malformed sequence
    /// (`str(data, encoding, "strict")` raising).
    fn strict_decode(self, body: &[u8]) -> Option<String> {
        match self {
            // No BOM sniffing here: Dammit strips exactly one mark
            // itself (`strip_bom`); `decode()` would strip a second
            // one and even switch decoders.
            PageEncoding::Rs(encoding) => encoding
                .decode_without_bom_handling_and_without_replacement(body)
                .map(|text| text.into_owned()),
            PageEncoding::Latin1 => Some(body.iter().map(|&byte| byte as char).collect()),
            PageEncoding::Utf16Bare => {
                if let Some(rest) = body.strip_prefix(b"\xfe\xff") {
                    PageEncoding::Rs(encoding_rs::UTF_16BE).strict_decode(rest)
                } else if let Some(rest) = body.strip_prefix(b"\xff\xfe") {
                    PageEncoding::Rs(encoding_rs::UTF_16LE).strict_decode(rest)
                } else {
                    PageEncoding::Rs(encoding_rs::UTF_16LE).strict_decode(body)
                }
            }
            PageEncoding::Utf32Bare => {
                if let Some(rest) = body.strip_prefix(b"\x00\x00\xfe\xff") {
                    PageEncoding::Utf32Be.strict_decode(rest)
                } else if let Some(rest) = body.strip_prefix(b"\xff\xfe\x00\x00") {
                    PageEncoding::Utf32Le.strict_decode(rest)
                } else {
                    PageEncoding::Utf32Le.strict_decode(body)
                }
            }
            PageEncoding::Utf32Be => decode_utf32(body, true),
            PageEncoding::Utf32Le => decode_utf32(body, false),
        }
    }

    /// Lossy decode: every undecodable unit becomes U+FFFD
    /// (`errors="replace"`). Always succeeds.
    fn decode_replace(self, body: &[u8]) -> String {
        match self {
            // Same single-strip rule as `strict_decode` above.
            PageEncoding::Rs(encoding) => encoding.decode_without_bom_handling(body).0.into_owned(),
            PageEncoding::Latin1 => body.iter().map(|&byte| byte as char).collect(),
            PageEncoding::Utf16Bare => {
                if let Some(rest) = body.strip_prefix(b"\xfe\xff") {
                    PageEncoding::Rs(encoding_rs::UTF_16BE).decode_replace(rest)
                } else if let Some(rest) = body.strip_prefix(b"\xff\xfe") {
                    PageEncoding::Rs(encoding_rs::UTF_16LE).decode_replace(rest)
                } else {
                    PageEncoding::Rs(encoding_rs::UTF_16LE).decode_replace(body)
                }
            }
            PageEncoding::Utf32Bare => {
                if let Some(rest) = body.strip_prefix(b"\x00\x00\xfe\xff") {
                    PageEncoding::Utf32Be.decode_replace(rest)
                } else if let Some(rest) = body.strip_prefix(b"\xff\xfe\x00\x00") {
                    PageEncoding::Utf32Le.decode_replace(rest)
                } else {
                    PageEncoding::Utf32Le.decode_replace(body)
                }
            }
            PageEncoding::Utf32Be => decode_utf32_replace(body, true),
            PageEncoding::Utf32Le => decode_utf32_replace(body, false),
        }
    }

    /// Strict decode of one byte (`bytearray([b]).decode(...)` in
    /// `handle_charref`).
    fn strict_byte(self, byte: u8) -> Option<char> {
        self.strict_decode(&[byte])
            .and_then(|text| text.chars().next())
    }
}

/// One UTF-32 unit in the given endianness.
fn utf32_unit(unit: &[u8; 4], big_endian: bool) -> u32 {
    if big_endian {
        u32::from_be_bytes(*unit)
    } else {
        u32::from_le_bytes(*unit)
    }
}

/// Strict UTF-32BE/LE decode (`char::from_u32` rejects surrogates and
/// out-of-range units exactly like CPython's strict decoder).
fn decode_utf32(body: &[u8], big_endian: bool) -> Option<String> {
    let (units, tail) = body.as_chunks::<4>();
    if !tail.is_empty() {
        return None;
    }
    let mut out = String::new();
    for unit in units {
        out.push(char::from_u32(utf32_unit(unit, big_endian))?);
    }
    Some(out)
}

/// UTF-32BE/LE with `errors="replace"`: one U+FFFD per bad unit, one
/// for a short tail.
fn decode_utf32_replace(body: &[u8], big_endian: bool) -> String {
    let (units, tail) = body.as_chunks::<4>();
    let mut out = String::new();
    for unit in units {
        out.push(char::from_u32(utf32_unit(unit, big_endian)).unwrap_or('\u{FFFD}'));
    }
    if !tail.is_empty() {
        out.push('\u{FFFD}');
    }
    out
}

/// BOM strip (`EncodingDetector.strip_byte_order_mark`,
/// `bs4/dammit.py:452`): the body without the mark plus the implied
/// encoding name. Mirrors the check order and, deliberately, the two
/// dead `!= "\x00\x00"` bytes-vs-str guards — so a UTF-32LE BOM
/// misdetects as UTF-16LE with only 2 of its 4 bytes stripped, exactly
/// as in Django.
fn strip_bom(body: &[u8]) -> (&[u8], Option<&'static str>) {
    if body.len() >= 4 && body.starts_with(&[0xFE, 0xFF]) {
        (&body[2..], Some("utf-16be"))
    } else if body.len() >= 4 && body.starts_with(&[0xFF, 0xFE]) {
        (&body[2..], Some("utf-16le"))
    } else if let Some(rest) = body.strip_prefix(b"\xef\xbb\xbf") {
        (rest, Some("utf-8"))
    } else if let Some(rest) = body.strip_prefix(b"\x00\x00\xfe\xff") {
        (rest, Some("utf-32be"))
    } else if let Some(rest) = body.strip_prefix(b"\xff\xfe\x00\x00") {
        (rest, Some("utf-32le"))
    } else {
        (body, None)
    }
}

static XML_ENCODING_RE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
    // `(?-u)`: Dammit runs these on bytes, so `\s`, `.` and case
    // folding are ASCII-only (Unicode mode would match NBSP, ſ and
    // skip non-UTF-8 bytes differently).
    regex::bytes::Regex::new(r#"(?i-u)^\s*<\?.*encoding=['"](.*?)['"].*\?>"#)
        .expect("Dammit XML encoding pattern compiles")
});
static META_CHARSET_RE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
    regex::bytes::Regex::new(r#"(?i-u)<\s*meta[^>]+charset\s*=\s*["']?([^>]*?)[ /;'">]"#)
        .expect("Dammit meta charset pattern compiles")
});

/// Declared encoding (`EncodingDetector.find_declared_encoding`,
/// `bs4/dammit.py:482`): the XML prolog first (even for HTML), then
/// the meta tag, each within its search window. An XML match ends the
/// search even when its value is empty.
fn find_declared_encoding(body: &[u8]) -> Option<String> {
    let xml_end = body.len().min(1024);
    if let Some(captures) = XML_ENCODING_RE.captures(&body[..xml_end]) {
        return captured_value(&captures);
    }
    let window = ((body.len() as f64 * 0.05) as usize).max(2048);
    let html_end = body.len().min(window);
    if let Some(captures) = META_CHARSET_RE.captures(&body[..html_end]) {
        return captured_value(&captures);
    }
    None
}

/// Capture group 1 as Dammit reads it: ASCII with U+FFFD per
/// non-ASCII byte, lowercased; empty reads as undeclared.
fn captured_value(captures: &regex::bytes::Captures) -> Option<String> {
    let raw = captures.get(1)?.as_bytes();
    let mut value = String::with_capacity(raw.len());
    for &byte in raw {
        if byte.is_ascii() {
            value.push(byte.to_ascii_lowercase() as char);
        } else {
            value.push('\u{FFFD}');
        }
    }
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Dammit's `CHARSET_ALIASES` (`bs4/dammit.py:530`).
fn dammit_alias(label: &str) -> String {
    match label {
        "macintosh" => "mac-roman".to_owned(),
        "x-sjis" => "shift-jis".to_owned(),
        _ => label.to_owned(),
    }
}

/// `codecs` label → decoder (`UnicodeDammit.find_codec` +
/// `codecs.lookup`): each Dammit attempt form (as-is, `-`-stripped,
/// `-`→`_`) runs the explicit spellings first and
/// `Encoding::for_label` last. An unresolvable label reads as absent
/// (Dammit's `LookupError` branch skips it).
fn resolve_label(label: &str) -> Option<PageEncoding> {
    let lower = label.to_ascii_lowercase();
    for form in [
        dammit_alias(&lower),
        lower.replace('-', ""),
        lower.replace('-', "_"),
    ] {
        if let Some(encoding) = resolve_form(&form) {
            return Some(encoding);
        }
    }
    None
}

/// One attempt form → decoder. The explicit tables are the exact
/// spellings CPython decodes (`encodings.aliases` + canonical names,
/// verified) wherever WHATWG maps them wrongly or not at all; the
/// `for_label` fallback covers the rest. Labels WHATWG knows but
/// CPython rejects (`mac`, `x-mac-roman`, ...) decode here and skip
/// there — accepted gap, never seen in real meta tags.
fn resolve_form(form: &str) -> Option<PageEncoding> {
    match form {
        // Pure latin-1, never WHATWG's windows-1252 mapping.
        "8859" | "cp819" | "csisolatin1" | "ibm819" | "iso8859" | "iso8859_1" | "iso8859-1"
        | "iso_8859_1" | "iso_8859_1_1987" | "iso_ir_100" | "l1" | "latin" | "latin1"
        | "latin_1" | "iso-8859-1" => Some(PageEncoding::Latin1),
        // Short `latinN` names (CPython decodes them; WHATWG has no
        // such labels). `latin5` is iso-8859-9, read as its
        // windows-1254 superset like the dashed spelling.
        "latin2" | "l2" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_2)),
        "latin3" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_3)),
        "latin4" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_4)),
        "latin5" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1254)),
        "latin6" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_10)),
        "latin7" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_13)),
        "latin8" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_14)),
        "latin9" | "l9" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_15)),
        "latin10" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_16)),
        "l7" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_13)),
        "l8" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_14)),
        "l10" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_16)),
        // Underscore `iso_8859_N` / `iso8859_N` spellings WHATWG
        // lacks (the dashed forms hit `for_label`); `_9`/`_11` read
        // as windows-1254/874 like `latin5`/`tis-620`.
        "iso_8859_2" | "iso8859_2" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_2)),
        "iso_8859_3" | "iso8859_3" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_3)),
        "iso_8859_4" | "iso8859_4" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_4)),
        "iso_8859_5" | "iso8859_5" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_5)),
        "iso_8859_6" | "iso8859_6" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_6)),
        "iso_8859_7" | "iso8859_7" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_7)),
        "iso_8859_8" | "iso8859_8" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_8)),
        "iso_8859_9" | "iso8859_9" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1254)),
        "iso_8859_10" | "iso8859_10" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_10)),
        "iso_8859_11" | "iso8859_11" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_874)),
        "iso_8859_13" | "iso8859_13" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_13)),
        "iso_8859_14" | "iso8859_14" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_14)),
        "iso_8859_15" | "iso8859_15" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_15)),
        "iso_8859_16" | "iso8859_16" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_16)),
        // Short 8859-6/7 names WHATWG lacks.
        "asmo_708" | "ecma_114" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_6)),
        "ecma_118" => Some(PageEncoding::Rs(encoding_rs::ISO_8859_7)),
        // Bare UTF-16/32 (BOM-or-LE) and fixed-endian forms.
        "u16" | "utf16" | "utf_16" | "utf-16" => Some(PageEncoding::Utf16Bare),
        "utf16be" | "utf_16be" | "utf_16_be" | "utf-16be" | "unicodebigunmarked" => {
            Some(PageEncoding::Rs(encoding_rs::UTF_16BE))
        }
        "utf16le" | "utf_16le" | "utf_16_le" | "utf-16le" | "unicodelittleunmarked" => {
            Some(PageEncoding::Rs(encoding_rs::UTF_16LE))
        }
        "u32" | "utf32" | "utf_32" | "utf-32" => Some(PageEncoding::Utf32Bare),
        "utf32be" | "utf_32be" | "utf_32_be" | "utf-32be" => Some(PageEncoding::Utf32Be),
        "utf32le" | "utf_32le" | "utf_32_le" | "utf-32le" => Some(PageEncoding::Utf32Le),
        // Shift_JIS spellings WHATWG lacks (`cp932`'s A0/FD-FF
        // PUA bytes read as malformed — accepted).
        "csshiftjis" | "s_jis" | "shiftjis" | "sjis" | "932" | "cp932" | "mskanji"
        | "x_mac_japanese" => Some(PageEncoding::Rs(encoding_rs::SHIFT_JIS)),
        // EUC-JP / EUC-KR spellings WHATWG lacks (`cp949`'s extra
        // multibyte rows read as malformed — accepted).
        "eucjp" | "u_jis" | "ujis" | "euc_jp" => Some(PageEncoding::Rs(encoding_rs::EUC_JP)),
        "euckr" | "euc_kr" | "ks_c_5601" | "ks_c_5601_1987" | "ks_x_1001" | "ksx1001" | "949"
        | "cp949" | "ms949" | "uhc" | "x_mac_korean" => Some(PageEncoding::Rs(encoding_rs::EUC_KR)),
        // GBK spellings WHATWG lacks.
        "936" | "cp936" | "euc_cn" | "euccn" | "eucgb2312_cn" | "ms936" | "gb2312_80"
        | "gb2312_1980" | "x_mac_simp_chinese" => Some(PageEncoding::Rs(encoding_rs::GBK)),
        "gb18030_2000" => Some(PageEncoding::Rs(encoding_rs::GB18030)),
        // Big5 spellings WHATWG lacks.
        "big5_tw" | "big5_hkscs" | "big5hkscs" | "hkscs" | "x_mac_trad_chinese" | "950"
        | "cp950" | "ms950" => Some(PageEncoding::Rs(encoding_rs::BIG5)),
        // `tis620`: WHATWG only lists `tis-620` (which hits
        // `for_label`); both read as windows-874, a superset of
        // strict TIS-620.
        "tis620" | "tis_620" | "tis_620_0" | "tis_620_2529_0" | "tis_620_2529_1" | "thai" => {
            Some(PageEncoding::Rs(encoding_rs::WINDOWS_874))
        }
        // ISO-2022-JP spellings WHATWG lacks (the dashed form hits
        // `for_label`).
        "iso2022_jp" | "iso2022jp" | "iso_2022_jp" => {
            Some(PageEncoding::Rs(encoding_rs::ISO_2022_JP))
        }
        // WHATWG reads `csiso2022kr` as the replacement encoding,
        // but CPython decodes it (`iso2022_kr`, which WHATWG lacks),
        // so skip it like the sibling spellings.
        "csiso2022kr" => None,
        // MacRoman spellings WHATWG lacks (`macintosh` hits `for_label`).
        "mac-roman" | "macroman" | "mac_roman" => Some(PageEncoding::Rs(encoding_rs::MACINTOSH)),
        // Bare-number windows-125x spellings WHATWG lacks.
        "1250" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1250)),
        "1251" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1251)),
        "1252" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1252)),
        "1253" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1253)),
        "1254" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1254)),
        "1255" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1255)),
        "1256" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1256)),
        "1257" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1257)),
        "1258" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1258)),
        // Underscore / `cp` windows-125x spellings WHATWG lacks.
        "windows_1250" | "cp1250" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1250)),
        "windows_1251" | "cp1251" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1251)),
        "windows_1252" | "cp1252" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1252)),
        "windows_1253" | "cp1253" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1253)),
        "windows_1254" | "cp1254" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1254)),
        "windows_1255" | "cp1255" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1255)),
        "windows_1256" | "cp1256" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1256)),
        "windows_1257" | "cp1257" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1257)),
        "windows_1258" | "cp1258" => Some(PageEncoding::Rs(encoding_rs::WINDOWS_1258)),
        // `utf8` / `utf_8`: WHATWG lacks the latter; skipping it would
        // still land on UTF-8, but resolving it keeps the strict
        // attempt (and `original_encoding` behavior) exact.
        "utf8" | "utf_8" => Some(PageEncoding::Rs(encoding_rs::UTF_8)),
        // ASCII spellings: WHATWG maps `ascii` (and the dashed
        // `ansi_x3.4-*` spellings) to windows-1252, but CPython's
        // strict ASCII either succeeds (pure-ASCII page, same text
        // as UTF-8) or fails into the same fallback this skip
        // takes — so skipping is outcome-exact here.
        "646" | "ansi_x3.4_1968" | "ansi_x3.4_1986" | "ansi_x3_4_1968" | "ascii" | "cp367"
        | "csascii" | "ibm367" | "iso646_us" | "iso_646.irv_1991" | "iso_ir_6" | "us"
        | "us_ascii" | "us-ascii" | "ansi_x3.4-1968" | "ansi_x3.4-1986" => None,
        // `koi8_u`: WHATWG only lists `koi8-u` (which hits
        // `for_label`).
        "koi8_u" => Some(PageEncoding::Rs(encoding_rs::KOI8_U)),
        // WHATWG reads `unicode` as UTF-16LE, but CPython has no such
        // codec (`LookupError`), so Dammit skips it.
        "unicode" => None,
        // WHATWG's `x-user-defined` (PUA mapping) likewise has no
        // CPython codec, so Dammit skips it too.
        "x-user-defined" | "x_user_defined" => None,
        // Dashless `iso8859N`: WHATWG labels, but CPython has no such
        // spellings (`LookupError`), so Dammit skips them.
        "iso88591" | "iso88592" | "iso88593" | "iso88594" | "iso88595" | "iso88596"
        | "iso88597" | "iso88598" | "iso88599" | "iso885910" | "iso885911" | "iso885913"
        | "iso885914" | "iso885915" | "iso885916" => None,
        _ => Encoding::for_label(form.as_bytes()).map(PageEncoding::Rs),
    }
}

/// Decoded page plus the winning decoder (`UnicodeDammit` over
/// `response.content`): BOM-sniffed encoding, declared (XML prolog,
/// then meta tag), UTF-8, then windows-1252 — each strict, then once
/// more with replacement (`bs4/dammit.py`). The
/// `charset-normalizer` detection step has no Rust equivalent and is
/// skipped: undeclared non-UTF-8 pages are best-effort. The response
/// `Content-Type` is never consulted (Django hands raw bytes to
/// BeautifulSoup).
fn decode_body(body: &[u8]) -> (String, PageEncoding) {
    let (stripped, sniffed) = strip_bom(body);
    let declared = find_declared_encoding(stripped);
    let mut candidates: Vec<&str> = Vec::with_capacity(4);
    let mut seen: Vec<String> = Vec::with_capacity(4);
    for label in sniffed
        .into_iter()
        .chain(declared.as_deref())
        .chain(["utf-8", "windows-1252"])
    {
        let lower = label.to_ascii_lowercase();
        if !seen.contains(&lower) {
            seen.push(lower);
            candidates.push(label);
        }
    }
    for label in &candidates {
        if let Some(encoding) = resolve_label(label) {
            if let Some(text) = encoding.strict_decode(stripped) {
                return (text, encoding);
            }
        }
    }
    for label in &candidates {
        if label.eq_ignore_ascii_case("ascii") {
            continue;
        }
        if let Some(encoding) = resolve_label(label) {
            return (encoding.decode_replace(stripped), encoding);
        }
    }
    // Unreachable in practice (UTF-8 with replacement always
    // succeeds); Dammit gives up here, we keep the lossy text.
    (
        String::from_utf8_lossy(stripped).into_owned(),
        PageEncoding::Rs(encoding_rs::UTF_8),
    )
}

/// Stripped `<title>` text (`soup.find("title").get_text().strip()`,
/// `:144-145`), or `None` when there is no title tag. `get_text`
/// concatenates descendant text (nested markup dropped) and decodes
/// entities; both are mirrored here. `original` is the winning body
/// decoder from [`decode_body`].
pub fn extract_title(html: &str, original: PageEncoding) -> Option<String> {
    let open = find_open_tag(html, "title")?;
    let end = tag_end(html, open)?;
    let lower = html.to_ascii_lowercase();
    let rest = &lower[end..];
    let close = rest.find("</title").map(|i| end + i).unwrap_or(html.len());
    Some(
        decode_entities(&strip_tags(&html[end + 1..close]), original)
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

/// Entity-decoded text (`handle_data` after `handle_entityref` /
/// `handle_charref`, `bs4/builder/_htmlparser.py:182-241`): the full
/// HTML5 named table plus numeric references. Tokenizing mirrors
/// `html.parser.goahead` (`parser.py:307-349`): a reference ends at
/// the first character outside its name class, and that terminator is
/// consumed only when it is `;`. Unknown names — and malformed
/// numeric refs — pass through with a dropped `;`, exactly as Django
/// emits them. All slices split at ASCII bytes, so they are always
/// char boundaries.
fn decode_entities(text: &str, original: PageEncoding) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'&' {
            let start = index;
            while index < bytes.len() && bytes[index] != b'&' {
                index += 1;
            }
            out.push_str(&text[start..index]);
            continue;
        }
        if index + 1 < bytes.len() && bytes[index + 1] == b'#' {
            if let Some(next) = match_charref(&mut out, text, index, original) {
                index = next;
            } else {
                // Failed numeric ref with a `;` later in the document
                // emits `&#` literally (`parser.py:317-321`). The no-`;`
                // tail-dump (`break` there) is not mirrored: real pages
                // always contain a `;` after a `<title>`.
                out.push_str("&#");
                index += 2;
            }
            continue;
        }
        if let Some(next) = match_entityref(&mut out, text, index) {
            index = next;
        } else {
            out.push('&');
            index += 1;
        }
    }
    out
}

/// Tries `&#...` at `index` (which points at `&`):
/// `&#(?:[0-9]+|[xX][0-9a-fA-F]+)[^0-9a-fA-F]` (`parser.py:25`).
/// Pushes the decoded char and returns the resume index on success.
fn match_charref(
    out: &mut String,
    text: &str,
    index: usize,
    original: PageEncoding,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut end = index + 2;
    let hexadecimal = end < bytes.len() && (bytes[end] == b'x' || bytes[end] == b'X');
    if hexadecimal {
        end += 1;
    }
    let digits = end;
    while end < bytes.len()
        && (bytes[end].is_ascii_digit() || hexadecimal && bytes[end].is_ascii_hexdigit())
    {
        end += 1;
    }
    if end == digits {
        return None;
    }
    if !hexadecimal && end < bytes.len() && bytes[end].is_ascii_hexdigit() {
        // A decimal run followed by a hex digit (`&#65B`) never
        // matches: shrinking the run only exposes another hex digit.
        return None;
    }
    let number = if hexadecimal {
        u32::from_str_radix(&text[digits..end], 16).ok()
    } else {
        text[digits..end].parse::<u32>().ok()
    };
    out.push(match number {
        Some(value) => decode_charref(value, original),
        None => '\u{FFFD}',
    });
    // The terminator is consumed only when it is `;`
    // (`parser.py:311-316`). A reference at the very end of the title
    // reads as terminated: in the document a `<` (at least `</title>`)
    // always follows it.
    if end < bytes.len() && bytes[end] == b';' {
        end += 1;
    }
    Some(end)
}

/// Numeric reference value → char (`handle_charref`,
/// `_htmlparser.py:182-220`): below 256, one byte through the
/// winning body decoder, then windows-1252 (compensating for pages
/// that number Windows-1252 code points — `&#147;` for `"`), then
/// the code point itself; otherwise the code point, or U+FFFD when
/// out of range. Surrogates also read as U+FFFD: Python emits them
/// as lone-surrogate escapes, which UTF-8 strings cannot hold.
fn decode_charref(number: u32, original: PageEncoding) -> char {
    if number < 256 {
        let byte = number as u8;
        if let Some(decoded) = original
            .strict_byte(byte)
            .or_else(|| PageEncoding::Rs(encoding_rs::WINDOWS_1252).strict_byte(byte))
        {
            return decoded;
        }
    }
    char::from_u32(number).unwrap_or('\u{FFFD}')
}

/// Tries `&name` at `index` (which points at `&`):
/// `&([a-zA-Z][-.a-zA-Z0-9]*)[^a-zA-Z0-9]` (`parser.py:24`). Pushes
/// the table text, or `&name` for unknown names (`handle_entityref`,
/// `_htmlparser.py:222-241`), and returns the resume index.
fn match_entityref(out: &mut String, text: &str, index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if index + 1 >= bytes.len() || !bytes[index + 1].is_ascii_alphabetic() {
        return None;
    }
    let mut end = index + 2;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-' || bytes[end] == b'.')
    {
        end += 1;
    }
    let name = &text[index + 1..end];
    match html_entities::lookup(name) {
        Some(replacement) => out.push_str(replacement),
        None => {
            out.push('&');
            out.push_str(name);
        }
    }
    if end < bytes.len() && bytes[end] == b';' {
        end += 1;
    }
    Some(end)
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
            let (page, encoding) = decode_body(&response.body);
            title = extract_title(&page, encoding);
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
    fn rejects_every_cpython_private_reserved_range() {
        // Vectors from the issue: `is_private` on CPython 3.12 covers
        // `0.0.0.0/8`, `::`, the TEST-NETs, `198.18.0.0/15`, `2001::/23`
        // and `2002::/16`; loopback/link-local/reserved add the rest.
        for ip in [
            "0.0.0.0",
            "0.1.2.3",
            "0.255.255.255",
            "::",
            "192.0.0.0",
            "192.0.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.1",
            "203.0.113.1",
            "2001::1",
            "2002::1",
            "2001:db8::1",
        ] {
            let err = validate_url_ip("http://example.com", &|_| Ok(vec![ip.parse().unwrap()]))
                .unwrap_err();
            assert_eq!(err, ValidateError::Private, "expected deny for {ip}");
        }
    }

    #[test]
    fn allows_multicast_only() {
        // Django never checks `is_multicast`; like CPython, it is not part
        // of the private/loopback/reserved/link-local deny set.
        for ip in ["224.0.0.1", "239.255.255.255", "ff02::1"] {
            validate_url_ip("http://example.com", &|_| Ok(vec![ip.parse().unwrap()]))
                .unwrap_or_else(|err| panic!("{ip} must pass, got {err:?}"));
        }
    }

    #[test]
    fn preserves_cpython_private_exceptions_and_public_addresses() {
        for ip in ["192.0.0.9", "192.0.0.10", "2001:1::1", "93.184.216.34"] {
            validate_url_ip("http://example.com", &|_| Ok(vec![ip.parse().unwrap()]))
                .unwrap_or_else(|err| panic!("{ip} must pass, got {err:?}"));
        }
    }

    #[test]
    fn rejects_ipv4_mapped_ipv6_reserved_range() {
        // CPython delegates `is_private` to the mapped IPv4 address but
        // keeps `is_reserved` on the IPv6 side (`::/8`), so even a public
        // mapped address is denied by Django.
        let err = validate_url_ip("http://example.com", &|_| {
            Ok(vec!["::ffff:93.184.216.34".parse().unwrap()])
        })
        .unwrap_err();
        assert_eq!(err, ValidateError::Private);
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
        let title = extract_title(
            "<html><head><title>A &amp; B <b>bold</b>&#33;</title></head></html>",
            PageEncoding::Rs(encoding_rs::UTF_8),
        );
        assert_eq!(title, Some("A & B bold!".to_owned()));
    }

    // -- PIDASHCONV-1280: HTML5 entities + charset-aware decode --------------
    // Expected values below are Django-observed (bs4 4.12.3, CPython 3.12):
    // `BeautifulSoup(content, "html.parser").find("title").get_text().strip()`.

    /// Title `crawl()` reports for one stubbed 200 body (404 favicon HEAD).
    fn crawl_title_of(body: &[u8], headers: HttpHeaders) -> Option<String> {
        let body = body.to_vec();
        let get = move |_: &HttpRequest| {
            Ok(HttpResponse {
                status: 200,
                headers: headers.clone(),
                body: body.clone(),
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
        match report.result.get("title") {
            Some(Value::String(title)) => Some(title.clone()),
            _ => None,
        }
    }

    #[test]
    fn crawl_title_decodes_html5_named_entities() {
        // Issue probe: `&mdash;` → EM DASH, `&nbsp;` → interior NBSP (strip
        // keeps it; only edge NBSP goes).
        let body =
            b"<html><head><title>Docs &mdash; Getting started&nbsp;guide</title></head></html>";
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("Docs \u{2014} Getting started\u{a0}guide".to_owned())
        );
    }

    #[test]
    fn crawl_title_honours_meta_charset_windows_1252() {
        // Issue probe: 0xE9 is U+FFFD under UTF-8, é under windows-1252.
        let body =
            b"<html><head><meta charset=\"windows-1252\"><title>Caf\xe9 menu</title></head></html>";
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("Caf\u{e9} menu".to_owned())
        );
    }

    #[test]
    fn crawl_title_honours_meta_charset_shift_jis() {
        // shift_jis bytes for 日本語 (93 FA 96 7B 8C EA).
        let body = b"<html><head><meta charset=\"shift_jis\"><title>\x93\xfa\x96\x7b\x8c\xea</title></head></html>";
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("\u{65e5}\u{672c}\u{8a9e}".to_owned())
        );
    }

    #[test]
    fn crawl_title_declared_charset_beats_valid_utf8() {
        // meta wins even when the bytes are valid UTF-8: C3 A9 → Ã©.
        let body =
            b"<html><head><meta charset=\"windows-1252\"><title>Caf\xc3\xa9</title></head></html>";
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("Caf\u{c3}\u{a9}".to_owned())
        );
    }

    #[test]
    fn crawl_title_bom_beats_declared_charset() {
        // BOM is tried before the meta declaration: C3 A9 → é.
        let body = b"\xef\xbb\xbf<html><head><meta charset=\"windows-1252\"><title>Caf\xc3\xa9</title></head></html>";
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("Caf\u{e9}".to_owned())
        );
    }

    #[test]
    fn crawl_title_honours_http_equiv_meta() {
        let body = b"<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\"><title>Caf\xe9</title></head></html>";
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("Caf\u{e9}".to_owned())
        );
    }

    #[test]
    fn crawl_title_xml_prolog_beats_meta() {
        // Dammit tries the XML declaration before the meta tag, even for
        // HTML: shift_jis bytes for 日.
        let body = b"<?xml version=\"1.0\" encoding=\"shift_jis\"?><html><head><meta charset=\"windows-1252\"><title>\x93\xfa</title></head></html>";
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("\u{65e5}".to_owned())
        );
    }

    #[test]
    fn crawl_title_ignores_content_type_charset() {
        // Django hands raw bytes to BeautifulSoup: a lying Content-Type
        // never overrides the meta declaration.
        let body =
            b"<html><head><meta charset=\"windows-1252\"><title>Caf\xe9 menu</title></head></html>";
        let headers = HttpHeaders(vec![(
            "Content-Type".to_owned(),
            "text/html; charset=utf-8".to_owned(),
        )]);
        assert_eq!(
            crawl_title_of(body, headers),
            Some("Caf\u{e9} menu".to_owned())
        );
    }

    #[test]
    fn crawl_title_metacharset_without_space_declares_nothing() {
        // `<metacharset=` does NOT declare: `[^>]+` needs a character
        // between `meta` and `charset` (Django-probed: declared None),
        // so both backends fall back — Rust deterministically to
        // windows-1252 here (Django consults its detector).
        let body =
            b"<html><head><metacharset=\"windows-1252\"><title>Caf\xe9</title></head></html>";
        assert_eq!(find_declared_encoding(body), None);
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("Caf\u{e9}".to_owned())
        );
    }

    #[test]
    fn crawl_title_decodes_utf16le_bom_page() {
        // BOM path: real UTF-16LE page behind its BOM.
        let mut body = b"\xff\xfe".to_vec();
        body.extend_from_slice(
            &"<html><head><title>Hi</title></head></html>"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<u8>>(),
        );
        assert_eq!(
            crawl_title_of(&body, HttpHeaders::new()),
            Some("Hi".to_owned())
        );
    }

    #[test]
    fn crawl_title_reproduces_utf32le_bom_quirk() {
        // Dammit misdetects the UTF-32LE BOM as UTF-16LE (dead `!=`
        // bytes-vs-str guard) and strips only 2 of its 4 bytes; the tags
        // then never parse, so the title is None.
        let mut body = b"\xff\xfe\x00\x00".to_vec();
        body.extend_from_slice(
            &"<html><head><title>Hi</title></head></html>"
                .chars()
                .flat_map(|c| (c as u32).to_le_bytes())
                .collect::<Vec<u8>>(),
        );
        assert_eq!(crawl_title_of(&body, HttpHeaders::new()), None);
    }

    #[test]
    fn crawl_title_decodes_utf32be_bom_page() {
        // The correctly-detected sibling: UTF-32BE BOM + UTF-32BE page.
        let mut body = b"\x00\x00\xfe\xff".to_vec();
        body.extend_from_slice(
            &"<html><head><title>Hi</title></head></html>"
                .chars()
                .flat_map(|c| (c as u32).to_be_bytes())
                .collect::<Vec<u8>>(),
        );
        assert_eq!(
            crawl_title_of(&body, HttpHeaders::new()),
            Some("Hi".to_owned())
        );
    }

    #[test]
    fn crawl_title_falls_past_failed_bom_decode_to_declared() {
        // BOM-sniffed UTF-8 fails strict on 0xE9, so the meta
        // declaration wins.
        let body = b"\xef\xbb\xbf<html><head><meta charset=\"windows-1252\"><title>Caf\xe9</title></head></html>";
        assert_eq!(
            crawl_title_of(body, HttpHeaders::new()),
            Some("Caf\u{e9}".to_owned())
        );
    }

    #[test]
    fn decode_entities_mirrors_bs4_tokenizer_edges() {
        // Each pair probed against bs4 4.12.3 (`get_text()` output for
        // the title; `.strip()` not applied here) — except the two
        // surrogate rows, where Django yields lone surrogates no
        // UTF-8 string can hold (read as U+FFFD; see module docs).
        let utf8 = PageEncoding::Rs(encoding_rs::UTF_8);
        for (input, expected) in [
            ("&apos", "'"),
            ("&apos;", "'"),
            ("&notin", "\u{2209}"),
            ("&notin;", "\u{2209}"),
            ("&unknown;", "&unknown"),
            ("&unknownx", "&unknownx"),
            ("&unknownx;", "&unknownx"),
            ("&ampX;", "&ampX"),
            ("R&D", "R&D"),
            ("AT&T", "AT&T"),
            ("Q&A;", "Q&A"),
            ("&#65BC", "&#65BC"),
            ("&#65x", "Ax"),
            ("&#x41g", "Ag"),
            ("&#65", "A"),
            ("&#;", "&#;"),
            ("&;", "&;"),
            ("&9;", "&9;"),
            ("&aMp;", "&aMp"),
            ("&AMP", "&"),
            ("&LT", "<"),
            ("&amp", "&"),
            ("&lt", "<"),
            ("&gt", ">"),
            ("&quot", "\""),
            ("&#65;", "A"),
            ("&#x41;", "A"),
            ("&#X41;", "A"),
            ("&#x0;", "\x00"),
            ("&#xD800;", "\u{FFFD}"),
            ("&#xDFFF;", "\u{FFFD}"),
            ("&#x110000;", "\u{FFFD}"),
            ("&#99999999;", "\u{FFFD}"),
            ("&#99999999999999999999;", "\u{FFFD}"),
            ("&#x80;", "\u{20ac}"),
            ("&#128;", "\u{20ac}"),
            ("&#147;", "\u{201c}"),
            ("&CounterClockwiseContourIntegral;", "\u{2233}"),
            ("&frac12;", "\u{bd}"),
            ("&aacute;", "\u{e1}"),
            ("&lt=3;", "<=3;"),
            ("&amp=3;", "&=3;"),
            ("&a-b.c1;", "&a-b.c1"),
            ("&", "&"),
            ("&#", "&#"),
            ("&#x", "&#x"),
            ("& ", "& "),
        ] {
            assert_eq!(decode_entities(input, utf8), expected, "input: {input:?}");
        }
        // NUL survives (Python emits it as a `\u0000` escape downstream).
        assert_eq!(decode_entities("&#0;", utf8), "\x00");
    }

    #[test]
    fn decode_charref_resolves_bytes_through_winning_decoder() {
        // `&#147;` on a UTF-8 page compensates through windows-1252;
        // on a latin-1 page the byte decodes there first (U+0093).
        assert_eq!(
            decode_entities("&#147;", PageEncoding::Rs(encoding_rs::UTF_8)),
            "\u{201c}"
        );
        assert_eq!(decode_entities("&#147;", PageEncoding::Latin1), "\u{93}");
        assert_eq!(decode_entities("&#232;", PageEncoding::Latin1), "\u{e8}");
        assert_eq!(
            decode_entities("&#232;", PageEncoding::Rs(encoding_rs::UTF_8)),
            "\u{e8}"
        );
    }

    #[test]
    fn resolve_label_matches_codecs_lookup_outcomes() {
        use PageEncoding::{Latin1, Rs, Utf16Bare, Utf32Bare, Utf32Be, Utf32Le};
        for (label, expected) in [
            ("utf-8", Some(Rs(encoding_rs::UTF_8))),
            ("UTF-8", Some(Rs(encoding_rs::UTF_8))),
            ("utf8", Some(Rs(encoding_rs::UTF_8))),
            ("utf_8", Some(Rs(encoding_rs::UTF_8))),
            ("windows-1252", Some(Rs(encoding_rs::WINDOWS_1252))),
            ("windows_1252", Some(Rs(encoding_rs::WINDOWS_1252))),
            ("cp1252", Some(Rs(encoding_rs::WINDOWS_1252))),
            ("windows1252", None),
            ("1250", Some(Rs(encoding_rs::WINDOWS_1250))),
            ("1251", Some(Rs(encoding_rs::WINDOWS_1251))),
            ("1252", Some(Rs(encoding_rs::WINDOWS_1252))),
            ("1253", Some(Rs(encoding_rs::WINDOWS_1253))),
            ("1254", Some(Rs(encoding_rs::WINDOWS_1254))),
            ("1255", Some(Rs(encoding_rs::WINDOWS_1255))),
            ("1256", Some(Rs(encoding_rs::WINDOWS_1256))),
            ("1257", Some(Rs(encoding_rs::WINDOWS_1257))),
            ("1258", Some(Rs(encoding_rs::WINDOWS_1258))),
            ("shift_jis", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("shift-jis", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("shiftjis", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("sjis", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("s_jis", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("x-sjis", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("x_sjis", None),
            ("xsjis", None),
            ("932", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("cp932", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("mskanji", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("x-mac-japanese", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("x_mac_japanese", Some(Rs(encoding_rs::SHIFT_JIS))),
            ("iso-8859-1", Some(Latin1)),
            ("iso8859-1", Some(Latin1)),
            ("latin1", Some(Latin1)),
            ("l1", Some(Latin1)),
            ("l_1", None),
            ("iso-8859-2", Some(Rs(encoding_rs::ISO_8859_2))),
            ("iso8859-2", Some(Rs(encoding_rs::ISO_8859_2))),
            ("iso88592", None),
            ("iso88591", None),
            ("iso8859_2", Some(Rs(encoding_rs::ISO_8859_2))),
            ("iso_8859_2", Some(Rs(encoding_rs::ISO_8859_2))),
            ("iso8859_11", Some(Rs(encoding_rs::WINDOWS_874))),
            ("iso_8859_11", Some(Rs(encoding_rs::WINDOWS_874))),
            ("iso8859_16", Some(Rs(encoding_rs::ISO_8859_16))),
            ("iso_8859_16", Some(Rs(encoding_rs::ISO_8859_16))),
            ("iso8859_3", Some(Rs(encoding_rs::ISO_8859_3))),
            ("iso_8859_3", Some(Rs(encoding_rs::ISO_8859_3))),
            ("iso8859_4", Some(Rs(encoding_rs::ISO_8859_4))),
            ("iso_8859_4", Some(Rs(encoding_rs::ISO_8859_4))),
            ("iso8859_5", Some(Rs(encoding_rs::ISO_8859_5))),
            ("iso_8859_5", Some(Rs(encoding_rs::ISO_8859_5))),
            ("iso8859_6", Some(Rs(encoding_rs::ISO_8859_6))),
            ("iso_8859_6", Some(Rs(encoding_rs::ISO_8859_6))),
            ("iso8859_7", Some(Rs(encoding_rs::ISO_8859_7))),
            ("iso_8859_7", Some(Rs(encoding_rs::ISO_8859_7))),
            ("iso8859_8", Some(Rs(encoding_rs::ISO_8859_8))),
            ("iso_8859_8", Some(Rs(encoding_rs::ISO_8859_8))),
            ("iso8859_9", Some(Rs(encoding_rs::WINDOWS_1254))),
            ("iso_8859_9", Some(Rs(encoding_rs::WINDOWS_1254))),
            ("iso8859_10", Some(Rs(encoding_rs::ISO_8859_10))),
            ("iso_8859_10", Some(Rs(encoding_rs::ISO_8859_10))),
            ("iso8859_13", Some(Rs(encoding_rs::ISO_8859_13))),
            ("iso_8859_13", Some(Rs(encoding_rs::ISO_8859_13))),
            ("iso8859_14", Some(Rs(encoding_rs::ISO_8859_14))),
            ("iso_8859_14", Some(Rs(encoding_rs::ISO_8859_14))),
            ("iso8859_15", Some(Rs(encoding_rs::ISO_8859_15))),
            ("iso_8859_15", Some(Rs(encoding_rs::ISO_8859_15))),
            ("l7", Some(Rs(encoding_rs::ISO_8859_13))),
            ("l8", Some(Rs(encoding_rs::ISO_8859_14))),
            ("l10", Some(Rs(encoding_rs::ISO_8859_16))),
            ("asmo_708", Some(Rs(encoding_rs::ISO_8859_6))),
            ("ecma_114", Some(Rs(encoding_rs::ISO_8859_6))),
            ("ecma_118", Some(Rs(encoding_rs::ISO_8859_7))),
            ("latin2", Some(Rs(encoding_rs::ISO_8859_2))),
            ("latin9", Some(Rs(encoding_rs::ISO_8859_15))),
            ("latin5", Some(Rs(encoding_rs::WINDOWS_1254))),
            ("euc-jp", Some(Rs(encoding_rs::EUC_JP))),
            ("euc-kr", Some(Rs(encoding_rs::EUC_KR))),
            ("gb2312", Some(Rs(encoding_rs::GBK))),
            ("gbk", Some(Rs(encoding_rs::GBK))),
            ("big5", Some(Rs(encoding_rs::BIG5))),
            ("utf-16", Some(Utf16Bare)),
            ("u16", Some(Utf16Bare)),
            ("utf-16le", Some(Rs(encoding_rs::UTF_16LE))),
            ("utf-16be", Some(Rs(encoding_rs::UTF_16BE))),
            ("utf_16_le", Some(Rs(encoding_rs::UTF_16LE))),
            ("utf_16_be", Some(Rs(encoding_rs::UTF_16BE))),
            ("unicodebigunmarked", Some(Rs(encoding_rs::UTF_16BE))),
            ("unicodelittleunmarked", Some(Rs(encoding_rs::UTF_16LE))),
            ("utf-32", Some(Utf32Bare)),
            ("u32", Some(Utf32Bare)),
            ("utf-32be", Some(Utf32Be)),
            ("utf-32le", Some(Utf32Le)),
            ("utf_32_be", Some(Utf32Be)),
            ("utf_32_le", Some(Utf32Le)),
            ("unicode", None),
            ("macintosh", Some(Rs(encoding_rs::MACINTOSH))),
            ("mac-roman", Some(Rs(encoding_rs::MACINTOSH))),
            ("macroman", Some(Rs(encoding_rs::MACINTOSH))),
            ("ascii", None),
            ("us-ascii", None),
            ("ansi_x3.4-1968", None),
            ("ansi_x3.4-1986", None),
            // KOI8 tables verified byte-identical to CPython's,
            // except koi8-u 0xAE/0xBE (RFC short-U vs CPython
            // box-drawing) — accepted 2-byte variance.
            ("koi8-r", Some(Rs(encoding_rs::KOI8_R))),
            ("koi8-u", Some(Rs(encoding_rs::KOI8_U))),
            ("koi8_u", Some(Rs(encoding_rs::KOI8_U))),
            // TIS-620 reads as its windows-874 superset.
            ("tis-620", Some(Rs(encoding_rs::WINDOWS_874))),
            ("tis620", Some(Rs(encoding_rs::WINDOWS_874))),
            ("tis_620", Some(Rs(encoding_rs::WINDOWS_874))),
            ("tis_620_0", Some(Rs(encoding_rs::WINDOWS_874))),
            ("tis_620_2529_0", Some(Rs(encoding_rs::WINDOWS_874))),
            ("tis_620_2529_1", Some(Rs(encoding_rs::WINDOWS_874))),
            ("thai", Some(Rs(encoding_rs::WINDOWS_874))),
            ("cp866", Some(Rs(encoding_rs::IBM866))),
            ("866", Some(Rs(encoding_rs::IBM866))),
            // Escape-heavy ISO-2022-JP pages are best-effort.
            ("iso-2022-jp", Some(Rs(encoding_rs::ISO_2022_JP))),
            ("iso2022_jp", Some(Rs(encoding_rs::ISO_2022_JP))),
            ("iso2022jp", Some(Rs(encoding_rs::ISO_2022_JP))),
            ("iso_2022_jp", Some(Rs(encoding_rs::ISO_2022_JP))),
            ("csiso2022kr", None),
            ("x-user-defined", None),
            ("x_user_defined", None),
            ("eucjp", Some(Rs(encoding_rs::EUC_JP))),
            ("ujis", Some(Rs(encoding_rs::EUC_JP))),
            ("euc_jp", Some(Rs(encoding_rs::EUC_JP))),
            ("euckr", Some(Rs(encoding_rs::EUC_KR))),
            ("euc_kr", Some(Rs(encoding_rs::EUC_KR))),
            ("korean", Some(Rs(encoding_rs::EUC_KR))),
            ("ks_c_5601", Some(Rs(encoding_rs::EUC_KR))),
            ("ks_c_5601_1987", Some(Rs(encoding_rs::EUC_KR))),
            ("ks_x_1001", Some(Rs(encoding_rs::EUC_KR))),
            ("ksx1001", Some(Rs(encoding_rs::EUC_KR))),
            ("949", Some(Rs(encoding_rs::EUC_KR))),
            ("cp949", Some(Rs(encoding_rs::EUC_KR))),
            ("ms949", Some(Rs(encoding_rs::EUC_KR))),
            ("uhc", Some(Rs(encoding_rs::EUC_KR))),
            ("x_mac_korean", Some(Rs(encoding_rs::EUC_KR))),
            ("x-mac-korean", Some(Rs(encoding_rs::EUC_KR))),
            ("chinese", Some(Rs(encoding_rs::GBK))),
            ("euccn", Some(Rs(encoding_rs::GBK))),
            ("eucgb2312_cn", Some(Rs(encoding_rs::GBK))),
            ("gb2312_80", Some(Rs(encoding_rs::GBK))),
            ("gb2312_1980", Some(Rs(encoding_rs::GBK))),
            ("x_mac_simp_chinese", Some(Rs(encoding_rs::GBK))),
            ("x-mac-simp-chinese", Some(Rs(encoding_rs::GBK))),
            ("936", Some(Rs(encoding_rs::GBK))),
            ("gb18030_2000", Some(Rs(encoding_rs::GB18030))),
            ("big5_tw", Some(Rs(encoding_rs::BIG5))),
            ("950", Some(Rs(encoding_rs::BIG5))),
            ("cp950", Some(Rs(encoding_rs::BIG5))),
            ("ms950", Some(Rs(encoding_rs::BIG5))),
            ("big5_hkscs", Some(Rs(encoding_rs::BIG5))),
            ("big5hkscs", Some(Rs(encoding_rs::BIG5))),
            ("hkscs", Some(Rs(encoding_rs::BIG5))),
            ("x_mac_trad_chinese", Some(Rs(encoding_rs::BIG5))),
            ("x-mac-trad-chinese", Some(Rs(encoding_rs::BIG5))),
            // Django decodes utf-7; no WHATWG equivalent — accepted gap.
            ("utf-7", None),
            ("bogus-9", None),
        ] {
            assert_eq!(resolve_label(label), expected, "label: {label:?}");
        }
    }

    #[test]
    fn strip_bom_mirrors_dammit_order_and_quirks() {
        for (input, rest, sniffed) in [
            ("<\u{0}h\u{0}".as_bytes(), "<\u{0}h\u{0}".as_bytes(), None),
            (
                b"\xef\xbb\xbf<html>".as_slice(),
                b"<html>".as_slice(),
                Some("utf-8"),
            ),
            (
                b"\x00\x00\xfe\xff rest".as_slice(),
                b" rest".as_slice(),
                Some("utf-32be"),
            ),
            // UTF-32LE BOM misdetects as UTF-16LE, strips 2 bytes.
            (
                b"\xff\xfe\x00\x00 rest".as_slice(),
                b"\x00\x00 rest".as_slice(),
                Some("utf-16le"),
            ),
            // Short inputs never sniff a 16-bit BOM.
            (b"\xfe\xff".as_slice(), b"\xfe\xff".as_slice(), None),
            (b"".as_slice(), b"".as_slice(), None),
        ] {
            assert_eq!(strip_bom(input), (rest, sniffed));
        }
    }

    #[test]
    fn find_declared_encoding_honours_windows_and_order() {
        // XML prolog wins over meta, even in HTML.
        let body = b"<?xml version=\"1.0\" encoding=\"shift_jis\"?><html><head><meta charset=\"windows-1252\">";
        assert_eq!(find_declared_encoding(body), Some("shift_jis".to_owned()));
        // Empty XML value ends the search (no meta fallback).
        let body =
            b"<?xml version=\"1.0\" encoding=\"\"?><html><head><meta charset=\"windows-1252\">";
        assert_eq!(find_declared_encoding(body), None);
        // Meta past the 2048-byte window is invisible.
        let mut body = b"<html><head>".to_vec();
        body.extend_from_slice(&vec![b'p'; 2500]);
        body.extend_from_slice(b"<meta charset=\"windows-1252\">");
        assert_eq!(find_declared_encoding(&body), None);
        // ...but reachable in a longer document (5% window): the meta
        // sits at ~2512 while 5% of the length is ~3126.
        let mut body = b"<html><head>".to_vec();
        body.extend_from_slice(&vec![b'p'; 2500]);
        body.extend_from_slice(b"<meta charset=\"windows-1252\">");
        body.extend_from_slice(&vec![b'q'; 60_000]);
        assert_eq!(
            find_declared_encoding(&body),
            Some("windows-1252".to_owned())
        );
        // Case-insensitive; empty value means undeclared.
        assert_eq!(
            find_declared_encoding(b"<META CHARSET=\"WINDOWS-1252\">"),
            Some("windows-1252".to_owned())
        );
        assert_eq!(find_declared_encoding(b"<meta charset=>"), None);
        // Byte-level pattern fidelity (all Django-probed): the
        // patterns are ASCII-only — NBSP/`ſ` never match `\s`/`(?i)`,
        // while raw non-UTF-8 bytes match `[^>]`/`.`.
        assert_eq!(
            find_declared_encoding(b"<\xc2\xa0meta charset=\"shift_jis\">"),
            None
        );
        assert_eq!(
            find_declared_encoding(b"<meta http-equiv=\"\xe9\" charset=\"shift_jis\">"),
            Some("shift_jis".to_owned())
        );
        assert_eq!(
            find_declared_encoding("<meta char\u{17f}et=\"shift_jis\">".as_bytes()),
            None
        );
        assert_eq!(
            find_declared_encoding(b"<?xml encoding=\"\xe9nc\"?><meta charset=\"shift_jis\">"),
            Some("\u{fffd}nc".to_owned())
        );
    }

    #[test]
    fn decode_body_strips_exactly_one_bom() {
        // Dammit strips one mark; a second decodes as data (U+FEFF),
        // never re-sniffed (Django-probed, detector-free inputs).
        let (text, encoding) = decode_body(b"\xef\xbb\xbf\xef\xbb\xbfHi");
        assert_eq!(text, "\u{feff}Hi");
        assert_eq!(encoding, PageEncoding::Rs(encoding_rs::UTF_8));
        let (text, encoding) = decode_body(b"\xff\xfe\xff\xfeA\x00");
        assert_eq!(text, "\u{feff}A");
        assert_eq!(encoding, PageEncoding::Rs(encoding_rs::UTF_16LE));
    }

    #[test]
    fn decode_utf32_rejects_and_replaces_like_python() {
        assert_eq!(decode_utf32(&[0, 0, 0, 0x41, 0, 0, 0xD8, 0], true), None);
        assert_eq!(decode_utf32(&[0, 0, 0], true), None);
        assert_eq!(
            decode_utf32_replace(&[0, 0, 0, 0x41, 0, 0, 0xD8, 0, 0, 0], true),
            "A\u{FFFD}\u{FFFD}"
        );
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
