//! Raw `requests`-faithful HTTP/1.1 client for webhook delivery (PIDASHCONV-1105).
//!
//! Port of the `requests.post(url, headers=headers, json=payload,
//! timeout=30)` call in `webhook_send_task`
//! (`apps/api/pi_dash/bgtasks/webhook_task.py:340-342`). `reqwest`
//! cannot emit this request: header names carry spaces
//! (`X-Pi Dash-Event`) and `http::HeaderName` rejects them — so the
//! client writes exact bytes over TCP (TLS via `tokio-rustls` for
//! `https`), mirroring `requests` 2.33 + `urllib3` 2.8 + CPython
//! `http.client`.
//!
//! Mirrored wire behavior (each pinned by a unit test or a probe noted
//! in the workpad):
//!
//! * Request layout: `POST {path} HTTP/1.1`, `Host` (port iff
//!   non-default), caller headers in `requests` order, `Content-Length`
//!   last, `Authorization` (URL userinfo only) after it, then the body.
//! * Space-named headers transmit verbatim — the sink's parser (not the
//!   client) truncates the block there.
//! * Redirects: 303/302 (non-HEAD) and 301 (POST) become GET with the
//!   body and content headers dropped; 307/308 replay; at most 30 hops
//!   (`Session.max_redirects`); relative `Location` resolved like
//!   `urljoin` with `requote_uri`; `Authorization` stripped per
//!   `should_strip_auth`; `Set-Cookie` collected into a simple jar.
//! * Responses: `HTTP/1.0` + `HTTP/1.1` status line, latin-1 headers in
//!   wire order (duplicates `", "`-joined at first position, first case
//!   kept — probed), obs-fold continuations, `Content-Length` / chunked
//!   (extensions and trailers ignored) / close-delimited bodies, no body
//!   for 1xx/204/304, gzip/deflate `Content-Encoding` decode.
//! * `response.text`: charset from `Content-Type` (`utf-8`, `ascii`,
//!   `latin-1`/`iso-8859-1`, `windows-1252`; anything else falls back
//!   like `LookupError`), `errors=replace`; no charset assumes UTF-8.
//! * `str(response.headers)`: the `CaseInsensitiveDict` dict repr through
//!   [`crate::pyformat::dict_repr`].
//!
//! Deliberate deviations (stack-specific, no byte-observable oracle
//! input reaches them — see the contract tests):
//!
//! * Proxy environment, `.netrc` auth, and IDNA hosts are not honored
//!   (`requests` merges them from the environment); non-ASCII hosts and
//!   proxy-only networks fail as transport errors.
//! * Missing-charset decoding assumes UTF-8 instead of running
//!   `charset_normalizer` (`apparent_encoding`); the two agree on every
//!   valid-UTF-8 body.
//! * Unknown `Content-Encoding` values pass through raw (only
//!   gzip/deflate decode).
//! * Transport-error *text* is this stack's wording, not urllib3's; the
//!   log-row shape (status `500`, empty headers, message body) and the
//!   retry/deactivation routing are what the port pins.
//!
//! Every failure this module reports is a `requests.RequestException`
//! equivalent: the caller logs the failure row and retries (or, past
//! the budget, deactivates). URL-shape failures are `InvalidURL` /
//! `MissingSchema` / `InvalidSchema` equivalents — also `RequestException`
//! subclasses — never the silent path.

use std::time::Duration;

/// `requests.post(..., timeout=30)`: 30 seconds per connect/read phase.
pub const REQUEST_TIMEOUT_SECS: u64 = 30;

/// `Session.max_redirects`: the initial fetch plus 30 redirect hops.
pub const MAX_REDIRECTS: u32 = 30;

/// Cap on the response header block (generous; `http.client` effectively
/// unbounded). Past it the response is a transport error.
const MAX_HEADER_BLOCK: usize = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------
// URL
// ---------------------------------------------------------------------------

/// Parsed webhook URL: http/https only, userinfo split out for the
/// `Authorization` header, fragment dropped (never sent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUrl {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub userinfo: Option<String>,
}

/// `requests` URL preparation (`PreparedRequest.prepare_url` +
/// `IDNA`/`parse_url` checks), minus IDNA encoding and proxy handling.
/// `MissingSchema` / `InvalidSchema` / `InvalidURL` equivalents surface
/// as `Err` strings in `requests`-like wording.
pub fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    // `requests` strips leading whitespace only.
    let url = url.trim_start();
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => {
            return Err(format!(
                "Invalid URL {url:?}: No scheme supplied. Perhaps you meant https://{url}?"
            ));
        }
    };
    if scheme != "http" && scheme != "https" {
        return Err(format!("No connection adapters were found for {url:?}"));
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, tail)) => (authority, format!("/{tail}")),
        None => (rest, "/".to_owned()),
    };
    if authority.is_empty() {
        return Err(format!("Invalid URL {url:?}: No host supplied"));
    }
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((user, host)) => (Some(user.to_owned()), host),
        None => (None, authority),
    };
    if hostport.is_empty() {
        return Err(format!("Invalid URL {url:?}: No host supplied"));
    }
    // IPv6 literals (`[::1]:port`) keep their brackets in `Host`.
    let (host, port) = if let Some(bracketed) = hostport.strip_prefix('[') {
        let (ip, rest) = bracketed
            .split_once(']')
            .ok_or_else(|| format!("Invalid URL {url:?}: Invalid IPv6 URL"))?;
        let port = match rest.strip_prefix(':') {
            Some("") | None => None,
            Some(text) => Some(parse_port(text, url)?),
        };
        (format!("[{ip}]"), port)
    } else {
        match hostport.rsplit_once(':') {
            Some((host, text)) if !text.is_empty() => {
                (host.to_owned(), Some(parse_port(text, url)?))
            }
            _ => (hostport.trim_end_matches(':').to_owned(), None),
        }
    };
    if host.is_empty() || host.starts_with('*') || host.starts_with('.') {
        return Err(format!("Invalid URL {url:?}: No host supplied"));
    }
    if !host.is_ascii() {
        return Err(format!("Invalid URL {url:?}: URL has an invalid label."));
    }
    // The fragment never reaches the wire (`prepare_url` drops it).
    let path = path
        .split_once('#')
        .map_or(path.clone(), |(head, _)| head.to_owned());
    let path = if path.is_empty() {
        "/".to_owned()
    } else {
        path
    };
    let default_port = if scheme == "https" { 443 } else { 80 };
    Ok(ParsedUrl {
        https: scheme == "https",
        host,
        port: port.unwrap_or(default_port),
        path,
        userinfo,
    })
}

fn parse_port(text: &str, url: &str) -> Result<u16, String> {
    if text.len() > 5 {
        return Err(format!("Invalid URL {url:?}: Port out of range"));
    }
    text.parse::<u32>()
        .ok()
        .filter(|p| *p <= 65535)
        .map(|p| p as u16)
        .ok_or_else(|| {
            format!("Invalid URL {url:?}: Port could not be cast to integer value as {text:?}")
        })
}

impl ParsedUrl {
    /// The `Host` header value: bare host, with `:port` iff non-default
    /// (`http.client` behavior).
    pub fn host_header(&self) -> String {
        let default = if self.https { 443 } else { 80 };
        if self.port == default {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// `Authorization: Basic ...` for URL userinfo, appended after
    /// `Content-Length` (`prepare_auth` runs after `prepare_body`).
    pub fn auth_header(&self) -> Option<String> {
        use base64::Engine;
        self.userinfo.as_ref().map(|info| {
            let (user, password) = info.split_once(':').unwrap_or((info.as_str(), ""));
            let token =
                base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
            format!("Basic {token}")
        })
    }
}

// ---------------------------------------------------------------------------
// request bytes
// ---------------------------------------------------------------------------

/// Exact request bytes: `{METHOD} {path} HTTP/1.1`, `Host`, the caller
/// headers verbatim (requests order — including space-named ones, which
/// no typed header map can hold), `Content-Length` last when a body is
/// present, the userinfo `Authorization` after it, then the body.
pub fn request_bytes(
    method: &str,
    parsed: &ParsedUrl,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("{} {} HTTP/1.1\r\n", method, parsed.path).as_bytes());
    out.extend_from_slice(format!("Host: {}\r\n", parsed.host_header()).as_bytes());
    for (name, value) in headers {
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    if let Some(body) = body {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    if let Some(auth) = parsed.auth_header() {
        out.extend_from_slice(format!("Authorization: {auth}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    if let Some(body) = body {
        out.extend_from_slice(body);
    }
    out
}

// ---------------------------------------------------------------------------
// response parse
// ---------------------------------------------------------------------------

/// One parsed HTTP response: final status, headers in wire order
/// (duplicates joined), raw (content-decoded) body bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Case-insensitive first-match header lookup.
pub fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Decode latin-1 bytes (how `http.client` reads header lines).
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| *b as char).collect()
}

/// Split a header block into `(name, value)` pairs: obs-fold
/// continuations append, duplicates `", "`-join at first position with
/// first-seen case (probed against `requests` 2.33).
pub fn parse_header_block(block: &[u8]) -> Result<Vec<(String, String)>, String> {
    let text = latin1(block);
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in text.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            // Obs-fold: append to the previous value (`http.client`).
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "Invalid header line in response".to_owned())?;
        let name = name.trim().to_owned();
        let value = value.trim().to_owned();
        if name.is_empty() {
            return Err("Invalid header line in response".to_owned());
        }
        match headers
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(&name))
        {
            Some((_, existing)) => {
                existing.push_str(", ");
                existing.push_str(&value);
            }
            None => headers.push((name, value)),
        }
    }
    Ok(headers)
}

/// Parsed status line + header block: status, headers, and the byte
/// offset where the body starts.
pub type StatusAndHeaders = (u16, Vec<(String, String)>, usize);

/// Parse the status line + header block; returns the status, headers,
/// and the byte offset where the body starts. Only `HTTP/1.0` and
/// `HTTP/1.1` parse (else `BadStatusLine`, a transport error).
pub fn parse_status_and_headers(bytes: &[u8]) -> Result<StatusAndHeaders, String> {
    let end = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "Response ended before headers completed".to_owned())?;
    if end > MAX_HEADER_BLOCK {
        return Err("Response header block too large".to_owned());
    }
    let head = &bytes[..end];
    let first_line_end = head
        .windows(2)
        .position(|w| w == b"\r\n")
        .unwrap_or(head.len());
    let status_line = latin1(&head[..first_line_end]);
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    let code = parts.next().unwrap_or("");
    if version != "HTTP/1.0" && version != "HTTP/1.1" {
        return Err(format!("Bad status line {status_line:?}"));
    }
    let status: u16 = code
        .parse()
        .map_err(|_| format!("Bad status line {status_line:?}"))?;
    let block = if first_line_end + 2 <= head.len() {
        &head[first_line_end + 2..]
    } else {
        &[][..]
    };
    let headers = parse_header_block(block)?;
    Ok((status, headers, end + 4))
}

/// Decode one chunked body (extensions and trailers ignored, like
/// `urllib3`'s reader). Returns the joined bytes and the offset just
/// past the terminal chunk.
pub fn decode_chunked(bytes: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let mut body = Vec::new();
    let mut at = 0;
    loop {
        let line_end = bytes[at..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| "Truncated chunked body".to_owned())?
            + at;
        let size_text = latin1(&bytes[at..line_end]);
        let size_text = size_text
            .split_once(';')
            .map_or(size_text.as_str(), |(n, _)| n)
            .trim();
        let size =
            usize::from_str_radix(size_text, 16).map_err(|_| "Invalid chunked body".to_owned())?;
        at = line_end + 2;
        if size == 0 {
            // Consume trailers through the blank line.
            let trailer_end = bytes[at..]
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| p + at + 4)
                .or_else(|| {
                    // Bare final CRLF with no trailers.
                    bytes[at..]
                        .windows(2)
                        .position(|w| w == b"\r\n")
                        .map(|p| p + at + 2)
                })
                .ok_or_else(|| "Truncated chunked body".to_owned())?;
            return Ok((body, trailer_end));
        }
        let chunk_end = at
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "Truncated chunked body".to_owned())?;
        body.extend_from_slice(&bytes[at..chunk_end]);
        at = chunk_end;
        if bytes.get(at..at + 2) != Some(b"\r\n") {
            return Err("Invalid chunked body".to_owned());
        }
        at += 2;
    }
}

/// Decode `Content-Encoding` (`urllib3` subset): gzip (incl. multi-member
/// streams) and deflate (zlib-wrapped, falling back to raw). Anything
/// else passes through raw.
pub fn decode_content(headers: &[(String, String)], body: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::bufread::{DeflateDecoder, GzDecoder};
    use std::io::Read;
    let encoding = header_value(headers, "content-encoding").unwrap_or("");
    // Multiple encodings apply in reverse listed order.
    let mut decoded = body.to_vec();
    for coding in encoding
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
            let mut decoder = GzDecoder::new(&decoded[..]);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| format!("Content decoding failed: {e}"))?;
            decoded = out;
        } else if coding.eq_ignore_ascii_case("deflate") {
            let mut decoder = DeflateDecoder::new(&decoded[..]);
            let mut out = Vec::new();
            match decoder.read_to_end(&mut out) {
                Ok(_) => decoded = out,
                Err(_) => {
                    // Raw deflate fallback (zlib header absent), like
                    // `urllib3`'s `-MAX_WBITS` retry.
                    decoded = decode_raw_deflate(&decoded)?;
                }
            }
        }
    }
    Ok(decoded)
}

/// Raw-deflate decode (no zlib wrapper).
fn decode_raw_deflate(body: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::{Decompress, FlushDecompress, Status};
    let mut decompress = Decompress::new(false);
    let mut out = vec![0u8; body.len().saturating_mul(4).max(1024)];
    loop {
        let filled = decompress.total_out() as usize;
        let consumed = decompress.total_in() as usize;
        if filled == out.len() {
            out.resize(out.len().saturating_mul(2).max(1024), 0);
            continue;
        }
        match decompress
            .decompress(
                &body[consumed..],
                &mut out[filled..],
                FlushDecompress::Finish,
            )
            .map_err(|e| format!("Content decoding failed: {e}"))?
        {
            Status::StreamEnd => {
                out.truncate(decompress.total_out() as usize);
                return Ok(out);
            }
            Status::BufError => {
                return Err("Content decoding failed: invalid deflate stream".to_owned());
            }
            Status::Ok => {
                if decompress.total_in() as usize >= body.len() {
                    // Complete input with no stream end: short stream,
                    // keep what decoded (matches `zlib.decompress` leniency
                    // on trailing state, minus the checksum it would check).
                    out.truncate(decompress.total_out() as usize);
                    return Ok(out);
                }
            }
        }
    }
}

/// True for statuses that never carry a body on the wire the way this
/// client reads (`http.client`: 1xx / 204 / 304).
pub fn status_has_no_body(status: u16) -> bool {
    (100..200).contains(&status) || status == 204 || status == 304
}

// ---------------------------------------------------------------------------
// response.text
// ---------------------------------------------------------------------------

/// Charset names `response.text` decodes, lowercased and stripped of
/// quotes/whitespace. `None` means "fall back like `LookupError`".
fn charset_kind(charset: &str) -> Option<Charset> {
    let name = charset.trim().trim_matches('"').to_ascii_lowercase();
    match name.as_str() {
        "utf-8" | "utf8" => Some(Charset::Utf8),
        "ascii" | "us-ascii" | "ansi_x3.4-1968" => Some(Charset::Ascii),
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => Some(Charset::Latin1),
        "windows-1252" | "cp1252" => Some(Charset::Windows1252),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    Utf8,
    Ascii,
    Latin1,
    Windows1252,
}

/// Map one windows-1252 byte: the 0x80-0x9F holes (`81 8D 8F 90 9D`)
/// are unmapped (`None` → U+FFFD under `errors=replace`).
fn windows_1252(byte: u8) -> Option<char> {
    const TABLE: [u32; 32] = [
        0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
        0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC,
        0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
    ];
    if byte < 0x80 {
        Some(byte as char)
    } else if byte < 0xA0 {
        let code = TABLE[(byte - 0x80) as usize];
        if code == 0 {
            None
        } else {
            char::from_u32(code)
        }
    } else {
        char::from_u32(byte as u32)
    }
}

/// Extract the `charset` parameter of the `Content-Type` header
/// (`cgi.parse_header` semantics: first parameter wins, quotes stripped).
pub fn content_charset(headers: &[(String, String)]) -> Option<String> {
    let content_type = header_value(headers, "content-type")?;
    for param in content_type.split(';').skip(1) {
        let param = param.trim();
        if let Some((name, value)) = param.split_once('=') {
            if name.trim().eq_ignore_ascii_case("charset") {
                let value = value.trim().trim_matches('"').trim().to_owned();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// `response.text`: charset decode with `errors=replace`; no charset (or
/// an unknown one) assumes UTF-8 lossy instead of `apparent_encoding`.
pub fn response_text(headers: &[(String, String)], body: &[u8]) -> String {
    let kind = content_charset(headers)
        .as_deref()
        .and_then(charset_kind)
        .unwrap_or(Charset::Utf8);
    match kind {
        Charset::Utf8 => String::from_utf8_lossy(body).into_owned(),
        Charset::Ascii => body
            .iter()
            .map(|b| if *b < 0x80 { *b as char } else { '\u{FFFD}' })
            .collect(),
        Charset::Latin1 => latin1(body),
        Charset::Windows1252 => body
            .iter()
            .map(|b| windows_1252(*b).unwrap_or('\u{FFFD}'))
            .collect(),
    }
}

/// `str(response.headers)`: the `CaseInsensitiveDict` dict repr over the
/// joined pairs (keys already unique).
pub fn headers_repr(headers: &[(String, String)]) -> String {
    let map: serde_json::Map<String, serde_json::Value> = headers
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    crate::pyformat::dict_repr(&map)
}

// ---------------------------------------------------------------------------
// redirects
// ---------------------------------------------------------------------------

/// `REDIRECT_STATI`: only these statuses follow a `Location`.
pub fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// First `Location` header value (case-insensitive name).
pub fn redirect_location(headers: &[(String, String)]) -> Option<&str> {
    header_value(headers, "location")
}

/// `Session.rebuild_method`: 303 (non-HEAD) and 302 (non-HEAD) become
/// GET; 301 becomes GET for POST only; 307/308 keep the method.
pub fn redirect_method(status: u16, method: &str) -> String {
    if status == 303 && method != "HEAD" {
        return "GET".to_owned();
    }
    if status == 302 && method != "HEAD" {
        return "GET".to_owned();
    }
    if status == 301 && method == "POST" {
        return "GET".to_owned();
    }
    method.to_owned()
}

/// `Session.should_strip_auth(old_url, new_url)`: hostname change always
/// strips; the http→https standard-ports upgrade keeps; otherwise strip
/// when the port or scheme changed.
pub fn should_strip_auth(old: &ParsedUrl, new: &ParsedUrl) -> bool {
    if !old.host.eq_ignore_ascii_case(&new.host) {
        return true;
    }
    let old_scheme = if old.https { "https" } else { "http" };
    let new_scheme = if new.https { "https" } else { "http" };
    if old_scheme == "http"
        && matches!(old.port, 80)
        && new_scheme == "https"
        && matches!(new.port, 443)
    {
        return false;
    }
    let old_default = (!old.https && old.port == 80) || (old.https && old.port == 443);
    let new_default = (!new.https && new.port == 80) || (new.https && new.port == 443);
    if !old_default || !new_default || old_scheme != new_scheme {
        let changed_port = old.port != new.port;
        let changed_scheme = old_scheme != new_scheme;
        return changed_port || changed_scheme;
    }
    false
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// `requests.utils.requote_uri`: unquote `%XX` triplets that decode to
/// unreserved characters, then quote every byte outside the safe set.
/// Invalid hex triplets fall back to quoting with `%` unsafe.
pub fn requote_uri(uri: &str) -> String {
    // `unquote_unreserved`: split on `%`, decode two following chars when
    // both are ASCII alphanumerics (Python tests Unicode alphanumerics —
    // non-ASCII there keeps the `%` literally here, an unreachable edge
    // for real `Location` values).
    let mut invalid = false;
    let mut parts = uri.split('%');
    let mut unquoted = parts.next().unwrap_or("").to_owned();
    for part in parts {
        let mut chars = part.chars();
        let head: String = chars.by_ref().take(2).collect();
        if head.len() == 2 && head.bytes().all(|b| b.is_ascii_alphanumeric()) {
            match u8::from_str_radix(&head, 16) {
                Ok(byte) if is_unreserved(byte) => {
                    unquoted.push(byte as char);
                    unquoted.extend(chars);
                    continue;
                }
                Ok(_) => {
                    unquoted.push('%');
                    unquoted.push_str(part);
                    continue;
                }
                Err(_) => {
                    invalid = true;
                    break;
                }
            }
        }
        unquoted.push('%');
        unquoted.push_str(part);
    }
    // `quote(..., safe=...)`: alphanumerics and `-_.~` plus the safe set
    // stay bare; `%` only when the first pass was valid; everything else
    // percent-encodes as UTF-8 with uppercase hex.
    const SAFE_WITH_PERCENT: &str = "!#$%&'()*+,/:;=?@[]~";
    const SAFE_WITHOUT_PERCENT: &str = "!#$&'()*+,/:;=?@[]~";
    let (source, safe) = if invalid {
        (uri, SAFE_WITHOUT_PERCENT)
    } else {
        (unquoted.as_str(), SAFE_WITH_PERCENT)
    };
    let mut out = String::with_capacity(source.len());
    for byte in source.bytes() {
        if is_unreserved(byte) || safe.bytes().any(|s| s == byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Resolve a redirect `Location` against the fetch URL (`urljoin` +
/// `requote_uri`, previous fragment inherited when the target has none).
pub fn resolve_url(base: &str, location: &str) -> String {
    let location = requote_uri(location);
    let base_head = base.split_once('#').map_or(base, |(head, _)| head);
    if location.contains("://") {
        return inherit_fragment(&location, base);
    }
    let (scheme, rest) = base_head.split_once("://").unwrap_or(("http", base_head));
    if let Some(network) = location.strip_prefix("//") {
        return inherit_fragment(&format!("{scheme}://{network}"), base);
    }
    let authority = rest.split_once('/').map_or(rest, |(a, _)| a);
    if location.starts_with('/') {
        // Absolute path (possibly with a new query).
        return inherit_fragment(&format!("{scheme}://{authority}{location}"), base);
    }
    if location.starts_with('?') {
        // Query-only reference keeps the base path.
        let path_end = rest.find('?').unwrap_or(rest.len());
        let base_no_query = &rest[..path_end];
        return inherit_fragment(&format!("{scheme}://{base_no_query}{location}"), base);
    }
    // Relative path: merge with the base directory, dot-normalize.
    let base_no_query = rest.find('?').map_or(rest, |q| &rest[..q]);
    let base_path = base_no_query
        .strip_prefix(authority)
        .unwrap_or(base_no_query);
    let base_dir = base_path
        .rsplit_once('/')
        .map_or("/", |(dir, _)| if dir.is_empty() { "/" } else { dir });
    let (loc_path, loc_query) = location
        .split_once('?')
        .map_or((location.as_str(), ""), |(p, q)| (p, q));
    let merged = if base_dir == "/" {
        format!("/{loc_path}")
    } else {
        format!("{base_dir}/{loc_path}")
    };
    let normalized = normalize_path(&merged);
    let target = if loc_query.is_empty() {
        format!("{scheme}://{authority}{normalized}")
    } else {
        format!("{scheme}://{authority}{normalized}?{loc_query}")
    };
    inherit_fragment(&target, base)
}

fn inherit_fragment(target: &str, base: &str) -> String {
    if target.contains('#') {
        return target.to_owned();
    }
    match base.split_once('#') {
        Some((_, fragment)) if !fragment.is_empty() => format!("{target}#{fragment}"),
        _ => target.to_owned(),
    }
}

/// Dot-segment normalization for merged redirect paths (RFC 3986 §5.2.4
/// subset: `.` / `..` over `/`-separated segments).
fn normalize_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    let joined = segments.join("/");
    if path.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

/// Minimal cookie jar for redirect chains: the first `name=value` pair
/// of each `Set-Cookie`, resent verbatim. Domain/path/expiry scoping is
/// not enforced (documented `http.cookiejar` deviation).
#[derive(Debug, Default, Clone)]
pub struct CookieJar {
    pairs: Vec<(String, String)>,
}

impl CookieJar {
    pub fn store_response(&mut self, headers: &[(String, String)]) {
        for (name, value) in headers {
            if !name.eq_ignore_ascii_case("set-cookie") {
                continue;
            }
            // Comma-joined values may hold several cookies: a segment
            // starts a new cookie when the text before its first `;`
            // holds `=` (date continuations like an `Expires` weekday
            // carry none and are skipped).
            for segment in value.split(',') {
                let head = segment.split_once(';').map_or(segment, |(p, _)| p);
                let Some((cookie_name, cookie_value)) = head.split_once('=') else {
                    continue;
                };
                let cookie_name = cookie_name.trim().to_owned();
                let cookie_value = cookie_value.trim().to_owned();
                if cookie_name.is_empty() {
                    continue;
                }
                self.pairs.retain(|(n, _)| n != &cookie_name);
                self.pairs.push((cookie_name, cookie_value));
            }
        }
    }

    pub fn header_value(&self) -> Option<String> {
        if self.pairs.is_empty() {
            return None;
        }
        Some(
            self.pairs
                .iter()
                .map(|(n, v)| format!("{n}={v}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

// ---------------------------------------------------------------------------
// transport
// ---------------------------------------------------------------------------

fn tls_config() -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

async fn tls_connect(
    stream: tokio::net::TcpStream,
    host: &str,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, String> {
    use std::str::FromStr;
    let name = if let Ok(addr) = std::net::IpAddr::from_str(host.trim_matches(['[', ']'])) {
        rustls::pki_types::ServerName::IpAddress(addr.into())
    } else {
        rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| format!("Invalid hostname for TLS: {host:?}"))?
    };
    let config = std::sync::Arc::new(tls_config()?);
    let connector = tokio_rustls::TlsConnector::from(config);
    connector
        .connect(name, stream)
        .await
        .map_err(|e| format!("TLS handshake failed: {e}"))
}

enum Stream {
    Plain(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

async fn read_exact_timeout(
    stream: &mut Stream,
    buf: &mut [u8],
    timeout: Duration,
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;
    let step = async {
        match stream {
            Stream::Plain(s) => s.read_exact(buf).await,
            Stream::Tls(s) => s.read_exact(buf).await,
        }
    };
    tokio::time::timeout(timeout, step)
        .await
        .map_err(|_| "Read timed out".to_owned())?
        .map(|_| ())
        .map_err(|e| format!("Connection failed: {e}"))
}

async fn read_until_headers(stream: &mut Stream, timeout: Duration) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let step = async {
            match stream {
                Stream::Plain(s) => s.read(&mut chunk).await,
                Stream::Tls(s) => s.read(&mut chunk).await,
            }
        };
        let read = tokio::time::timeout(timeout, step)
            .await
            .map_err(|_| "Read timed out".to_owned())?
            .map_err(|e| format!("Connection failed: {e}"))?;
        if read == 0 {
            return Err("Connection closed before headers completed".to_owned());
        }
        buf.extend_from_slice(&chunk[..read]);
        if buf.len() > MAX_HEADER_BLOCK + 4 {
            return Err("Response header block too large".to_owned());
        }
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return Ok(buf);
        }
    }
}

async fn read_body(
    stream: &mut Stream,
    already: Vec<u8>,
    headers: &[(String, String)],
    status: u16,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    if status_has_no_body(status) {
        return Ok(Vec::new());
    }
    let transfer = header_value(headers, "transfer-encoding").unwrap_or("");
    let chunked = transfer
        .split(',')
        .any(|c| c.trim().eq_ignore_ascii_case("chunked"));
    if chunked {
        // Chunked framing must parse incrementally: read to close, then
        // decode (trailer handling needs the terminal chunk).
        let mut buf = already;
        let mut chunk = [0u8; 8192];
        loop {
            let step = async {
                match stream {
                    Stream::Plain(s) => s.read(&mut chunk).await,
                    Stream::Tls(s) => s.read(&mut chunk).await,
                }
            };
            let read = tokio::time::timeout(timeout, step)
                .await
                .map_err(|_| "Read timed out".to_owned())?
                .map_err(|e| format!("Connection failed: {e}"))?;
            if read == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..read]);
        }
        let (body, _) =
            decode_chunked(&buf).map_err(|detail| format!("Chunked encoding error: {detail}"))?;
        return Ok(body);
    }
    if let Some(length) = header_value(headers, "content-length") {
        // First value wins; garbage means close-delimited (`http.client`).
        let first = length.split(',').next().unwrap_or("").trim();
        if let Ok(wanted) = first.parse::<usize>() {
            let mut body = already;
            if body.len() > wanted {
                body.truncate(wanted);
                return Ok(body);
            }
            body.reserve(wanted.saturating_sub(body.len()));
            while body.len() < wanted {
                let mut chunk = vec![0u8; (wanted - body.len()).min(65536)];
                read_exact_timeout(stream, &mut chunk, timeout).await?;
                body.extend_from_slice(&chunk);
            }
            return Ok(body);
        }
    }
    // Close-delimited: read to EOF.
    let mut buf = already;
    let mut chunk = [0u8; 8192];
    loop {
        let step = async {
            match stream {
                Stream::Plain(s) => s.read(&mut chunk).await,
                Stream::Tls(s) => s.read(&mut chunk).await,
            }
        };
        let read = tokio::time::timeout(timeout, step)
            .await
            .map_err(|_| "Read timed out".to_owned())?
            .map_err(|e| format!("Connection failed: {e}"))?;
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    Ok(buf)
}

async fn fetch_once(
    parsed: &ParsedUrl,
    method: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<HttpResponse, String> {
    use tokio::io::AsyncWriteExt;
    let mut addrs = tokio::net::lookup_host(format!("{}:{}", parsed.host, parsed.port))
        .await
        .map_err(|e| format!("DNS failed for {}: {e}", parsed.host))?;
    let addr = addrs
        .next()
        .ok_or_else(|| format!("DNS failed for {}: no addresses", parsed.host))?;
    let tcp = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr))
        .await
        .map_err(|_| format!("Connection to {} timed out", parsed.host))?
        .map_err(|e| format!("Connection to {} failed: {e}", parsed.host))?;
    let _ = tcp.set_nodelay(true);
    let mut stream = if parsed.https {
        let host = parsed.host.trim_matches(['[', ']']);
        let tls = tokio::time::timeout(timeout, tls_connect(tcp, host))
            .await
            .map_err(|_| "TLS handshake timed out".to_owned())??;
        Stream::Tls(Box::new(tls))
    } else {
        Stream::Plain(tcp)
    };
    let request = request_bytes(method, parsed, headers, body);
    let write = async {
        match &mut stream {
            Stream::Plain(s) => s.write_all(&request).await,
            Stream::Tls(s) => s.write_all(&request).await,
        }
    };
    tokio::time::timeout(timeout, write)
        .await
        .map_err(|_| "Write timed out".to_owned())?
        .map_err(|e| format!("Connection failed: {e}"))?;
    let raw = read_until_headers(&mut stream, timeout).await?;
    let (status, headers, body_at) = parse_status_and_headers(&raw)?;
    let already = raw[body_at..].to_vec();
    let wire_body = read_body(&mut stream, already, &headers, status, timeout).await?;
    let body = decode_content(&headers, &wire_body)?;
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

/// `requests.post(url, headers=headers, body, timeout=30)` with redirect
/// following: at most [`MAX_REDIRECTS`] hops, else the
/// `TooManyRedirects` equivalent. Returns the FINAL response (headers
/// and body are what the task logs). Every `Err` is a
/// `RequestException` equivalent (log the failure row, then retry or
/// deactivate).
pub async fn post(
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
    timeout_secs: u64,
) -> Result<HttpResponse, String> {
    post_with_redirects(url, "POST", headers, Some(body), timeout_secs).await
}

/// Redirect-capable fetch shared by the initial POST and the rebuilt
/// hops (GET drops the body).
pub async fn post_with_redirects(
    url: &str,
    method: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    timeout_secs: u64,
) -> Result<HttpResponse, String> {
    let timeout = Duration::from_secs(timeout_secs);
    let mut current_url = url.to_owned();
    let mut method = method.to_owned();
    let mut headers = headers.to_vec();
    let mut body = body.map(<[u8]>::to_vec);
    let mut jar = CookieJar::default();
    let mut hops = 0u32;
    loop {
        let parsed = parse_url(&current_url)?;
        if let Some(cookie) = jar.header_value() {
            headers.retain(|(name, _)| !name.eq_ignore_ascii_case("cookie"));
            headers.push(("Cookie".to_owned(), cookie));
        }
        let response = fetch_once(&parsed, &method, &headers, body.as_deref(), timeout).await?;
        jar.store_response(&response.headers);
        let location = if is_redirect_status(response.status) {
            redirect_location(&response.headers).map(str::to_owned)
        } else {
            None
        };
        let Some(location) = location else {
            return Ok(response);
        };
        hops += 1;
        if hops > MAX_REDIRECTS {
            return Err(format!("Exceeded {MAX_REDIRECTS} redirects"));
        }
        let next_url = resolve_url(&current_url, &location);
        let next_parsed = parse_url(&next_url)?;
        // Rebuild the method/body/headers for the hop.
        let next_method = redirect_method(response.status, &method);
        if response.status == 307 || response.status == 308 {
            method = next_method;
        } else {
            method = next_method;
            body = None;
            headers.retain(|(name, _)| {
                !(name.eq_ignore_ascii_case("content-length")
                    || name.eq_ignore_ascii_case("content-type")
                    || name.eq_ignore_ascii_case("transfer-encoding"))
            });
        }
        if should_strip_auth(&parsed, &next_parsed) {
            headers.retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
        }
        current_url = next_url;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn url_parsing_matches_requests_taxonomy() {
        let parsed = parse_url("http://example.com/hook?a=1#frag").unwrap();
        assert!(!parsed.https);
        assert_eq!(parsed.host, "example.com");
        assert_eq!(parsed.port, 80);
        assert_eq!(parsed.path, "/hook?a=1");
        assert_eq!(parsed.userinfo, None);
        assert_eq!(parsed.host_header(), "example.com");

        let secure = parse_url("https://example.com:8443").unwrap();
        assert!(secure.https);
        assert_eq!(secure.port, 8443);
        assert_eq!(secure.path, "/");
        assert_eq!(secure.host_header(), "example.com:8443");

        let auth = parse_url("http://u:p@example.com/").unwrap();
        assert_eq!(auth.userinfo, Some("u:p".to_owned()));
        assert_eq!(auth.host, "example.com");
        assert_eq!(auth.auth_header().unwrap(), "Basic dTpw");

        // Requests-like failures (all `RequestException` equivalents).
        assert!(parse_url("example.com/hook").is_err());
        assert!(parse_url("ftp://example.com/").is_err());
        assert!(parse_url("http:///path").is_err());
        assert!(parse_url("http://example.com:abc/").is_err());
        assert!(parse_url("http://.example.com/").is_err());
        assert!(parse_url("  http://example.com/").is_ok());
    }

    #[test]
    fn request_bytes_match_requests_wire_order() {
        // Order pinned by the raw-socket probe: Host, caller headers,
        // Content-Length, then the body.
        let parsed = parse_url("http://127.0.0.1:43587/hook").unwrap();
        // `requests` merges defaults first with the custom User-Agent
        // overriding in place; the caller supplies that merged order.
        let merged = headers(&[
            ("User-Agent", "Autopilot"),
            ("Accept-Encoding", "gzip, deflate"),
            ("Accept", "*/*"),
            ("Connection", "keep-alive"),
            ("Content-Type", "application/json"),
            ("X-Pi Dash-Delivery", "d-123"),
            ("X-Pi Dash-Event", "issue"),
        ]);
        let body = br#"{"event": "issue", "action": "create"}"#;
        let wire = request_bytes("POST", &parsed, &merged, Some(body));
        let text = String::from_utf8(wire).unwrap();
        assert_eq!(
            text,
            "POST /hook HTTP/1.1\r\n\
             Host: 127.0.0.1:43587\r\n\
             User-Agent: Autopilot\r\n\
             Accept-Encoding: gzip, deflate\r\n\
             Accept: */*\r\n\
             Connection: keep-alive\r\n\
             Content-Type: application/json\r\n\
             X-Pi Dash-Delivery: d-123\r\n\
             X-Pi Dash-Event: issue\r\n\
             Content-Length: 38\r\n\
             \r\n\
             {\"event\": \"issue\", \"action\": \"create\"}"
        );
    }

    #[test]
    fn response_parse_joins_dupes_and_keeps_order() {
        let raw = b"HTTP/1.1 200 OK\r\n\
            Server: Test/1.0\r\n\
            X-Dup: first\r\n\
            X-DUP: second\r\n\
            Content-Length: 2\r\n\
            \r\n{}";
        let (status, head, at) = parse_status_and_headers(raw).unwrap();
        assert_eq!(status, 200);
        assert_eq!(
            head,
            headers(&[
                ("Server", "Test/1.0"),
                ("X-Dup", "first, second"),
                ("Content-Length", "2"),
            ])
        );
        assert_eq!(&raw[at..], b"{}");
        assert!(parse_status_and_headers(b"HTTP/2 200 OK\r\n\r\n").is_err());
    }

    #[test]
    fn chunked_decode_ignores_extensions_and_trailers() {
        let raw = b"4;ext=1\r\nWiki\r\n5\r\npedia\r\n0\r\nX-T: v\r\n\r\n";
        let (body, _) = decode_chunked(raw).unwrap();
        assert_eq!(body, b"Wikipedia");
    }

    #[test]
    fn content_decoding_handles_gzip_and_deflate() {
        use flate2::write::{DeflateEncoder, GzEncoder};
        use flate2::Compression;
        use std::io::Write;
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(b"hello").unwrap();
        let gzipped = gz.finish().unwrap();
        let head = headers(&[("Content-Encoding", "gzip")]);
        assert_eq!(decode_content(&head, &gzipped).unwrap(), b"hello");

        let mut z = DeflateEncoder::new(Vec::new(), Compression::default());
        z.write_all(b"world").unwrap();
        let zlibed = z.finish().unwrap();
        let head = headers(&[("Content-Encoding", "deflate")]);
        assert_eq!(decode_content(&head, &zlibed).unwrap(), b"world");

        // Unknown encodings pass through raw.
        let head = headers(&[("Content-Encoding", "br")]);
        assert_eq!(decode_content(&head, b"raw").unwrap(), b"raw");
    }

    #[test]
    fn response_text_matches_requests_charset_rules() {
        let head = headers(&[("Content-Type", "text/plain; charset=latin-1")]);
        assert_eq!(response_text(&head, b"caf\xe9"), "café");
        let head = headers(&[("Content-Type", "text/plain; charset=windows-1252")]);
        assert_eq!(response_text(&head, b"\x80\x81"), "€\u{FFFD}");
        let head = headers(&[("Content-Type", "application/json")]);
        assert_eq!(response_text(&head, b"{\"a\": 1}"), "{\"a\": 1}");
        // Unknown charset falls back like `LookupError`.
        let head = headers(&[("Content-Type", "text/plain; charset=bogus-9")]);
        assert_eq!(response_text(&head, b"ok"), "ok");
    }

    #[test]
    fn headers_repr_matches_case_insensitive_dict_str() {
        let head = headers(&[
            ("Server", "Test/1.0"),
            ("Content-Type", "application/json"),
            ("Content-Length", "12"),
        ]);
        assert_eq!(
            headers_repr(&head),
            "{'Server': 'Test/1.0', 'Content-Type': 'application/json', 'Content-Length': '12'}"
        );
    }

    #[test]
    fn redirect_rules_match_requests() {
        assert_eq!(redirect_method(303, "POST"), "GET");
        assert_eq!(redirect_method(302, "POST"), "GET");
        assert_eq!(redirect_method(301, "POST"), "GET");
        assert_eq!(redirect_method(301, "PUT"), "PUT");
        assert_eq!(redirect_method(307, "POST"), "POST");
        assert_eq!(redirect_method(302, "HEAD"), "HEAD");
        assert!(!is_redirect_status(200));
        assert!(!is_redirect_status(500));
        assert!(is_redirect_status(308));

        assert_eq!(
            resolve_url("http://h/a/b?x=1#f", "/c?y=2"),
            "http://h/c?y=2#f"
        );
        assert_eq!(resolve_url("http://h/a/b", "c"), "http://h/a/c");
        assert_eq!(resolve_url("http://h/a/b", "../c"), "http://h/c");
        assert_eq!(resolve_url("http://h/a/b", "?y=2"), "http://h/a/b?y=2");
        assert_eq!(resolve_url("http://h/a", "//o/d"), "http://o/d");
        assert_eq!(requote_uri("/a b/%7e"), "/a%20b/~");
        assert_eq!(requote_uri("/a%2G"), "/a%252G");
    }

    #[test]
    fn auth_stripping_matches_requests() {
        let a = parse_url("http://h/a").unwrap();
        let b = parse_url("http://h/b").unwrap();
        assert!(!should_strip_auth(&a, &b));
        let other = parse_url("http://o/b").unwrap();
        assert!(should_strip_auth(&a, &other));
        let upgrade = parse_url("https://h/b").unwrap();
        assert!(!should_strip_auth(&a, &upgrade));
        let downgrade = parse_url("http://h/b").unwrap();
        assert!(should_strip_auth(&upgrade, &downgrade));
        let other_port = parse_url("http://h:8080/b").unwrap();
        assert!(should_strip_auth(&a, &other_port));
    }

    #[test]
    fn cookie_jar_collects_and_resends() {
        let mut jar = CookieJar::default();
        jar.store_response(&headers(&[(
            "Set-Cookie",
            "a=1; Expires=Wed, 21 Oct 2015 07:28:00 GMT; Path=/, b=2",
        )]));
        assert_eq!(jar.header_value().unwrap(), "a=1; b=2");
    }

    /// Live loopback round-trip: the stub asserts the exact request bytes
    /// (space headers verbatim, header order, body) and the client parses
    /// the response.
    #[tokio::test]
    async fn loopback_round_trip_matches_requests_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let read = socket.read(&mut buf).await.unwrap();
            buf.truncate(read);
            let text = String::from_utf8(buf).unwrap();
            assert!(text.starts_with("POST /hook HTTP/1.1\r\n"), "{text}");
            assert!(text.contains("X-Pi Dash-Event: issue\r\n"), "{text}");
            let body = br#"{"ok":true}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.write_all(body).await.unwrap();
        });
        let head = headers(&[
            ("User-Agent", "Autopilot"),
            ("Accept-Encoding", "gzip, deflate"),
            ("Accept", "*/*"),
            ("Connection", "keep-alive"),
            ("Content-Type", "application/json"),
            ("X-Pi Dash-Event", "issue"),
        ]);
        let response = post(
            &format!("http://127.0.0.1:{port}/hook"),
            &head,
            br#"{"event":"issue"}"#,
            30,
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, br#"{"ok":true}"#);
        assert_eq!(
            response_text(&response.headers, &response.body),
            "{\"ok\":true}"
        );
        server.await.unwrap();
    }

    /// Live loopback redirect: 302 becomes GET with the body dropped and
    /// the final response returned.
    #[tokio::test]
    async fn loopback_redirect_rebuilds_like_requests() {
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_server = seen.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let read = socket.read(&mut buf).await.unwrap();
                buf.truncate(read);
                let text = String::from_utf8(buf).unwrap();
                let first = text.lines().next().unwrap_or("").to_owned();
                let count = {
                    let mut guard = seen_server.lock().unwrap();
                    guard.push(first);
                    guard.len()
                };
                if count == 1 {
                    let response = "HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    socket.write_all(response.as_bytes()).await.unwrap();
                } else {
                    let body = b"done";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                    socket.write_all(body).await.unwrap();
                }
            }
        });
        let response = post(
            &format!("http://127.0.0.1:{port}/start"),
            &headers(&[("User-Agent", "Autopilot")]),
            b"payload",
            30,
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"done");
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].starts_with("POST /start "), "{}", seen[0]);
        assert!(seen[1].starts_with("GET /next "), "{}", seen[1]);
        server.await.unwrap();
    }
}
