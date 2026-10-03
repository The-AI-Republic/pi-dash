//! Shared request-body content negotiation + parsing (D-20, stage 5, PIDASHCONV-627).
//!
//! Ports DRF's body pipeline for the api-v1 write paths:
//! `Request._load_stream` / `_parse` (`rest_framework/request.py`), parser
//! selection (`rest_framework/negotiation.py:DefaultContentNegotiation`),
//! the three default parsers (`rest_framework/parsers.py`: `JSONParser`,
//! `FormParser`, `MultiPartParser`), Django's `QueryDict` + multipart
//! machine (`django/http/request.py`, `django/http/multipartparser.py`),
//! header splitting (`django/utils/http.py:parse_header_parameters`) and
//! charset resolution (`django/http/request.py:_set_content_type_params` +
//! `encodings` aliases). Settings pinned: no `DEFAULT_PARSER_CLASSES`
//! override, `STRICT_JSON` default true, `DEFAULT_CHARSET` utf-8,
//! `DATA_UPLOAD_MAX_NUMBER_FIELDS` 1000, `DATA_UPLOAD_MAX_NUMBER_FILES`
//! 100, `DATA_UPLOAD_MAX_MEMORY_SIZE` 5242880 (`settings/common.py:594`).
//!
//! Every owned write path parses through [`negotiate_body`]: empty (by the
//! Content-Length header, not the byte count) is `{}`, an unsupported or
//! missing content type is the 415, otherwise the negotiated parser runs.
//! JSON callers keep their existing serde + CPython-error mapping and only
//! gain the shared decode (charset + trailing-incomplete drop); form and
//! multipart callers get the [`NegotiatedBody::Form`] map with the
//! HTML-input semantics DRF applies (`get_value`, list `getlist`, the
//! blank-input rules — verified live, see the `skip_blank_fields` note on
//! [`BodySpec`]).
//!
//! Non-goals (documented, not stubbed): the 5MB body 413 lives in Django
//! middleware (`request_body_size.py`) and belongs to a serve-wide
//! follow-up; exotic Python codecs beyond [`SupportedCharset`] degrade to
//! utf-8 (bogus-charset behavior) with a follow-up filed; filename
//! sanitizing covers the reachable echo shapes (full html5-entity table
//! filed as a follow-up). Paths that never touch `request.data`
//! (GET/DELETE/archive) never call this module, so they can never 415.

use axum::http::HeaderMap;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Per-domain HTML-input shape: which form/multipart keys arrive as arrays
/// (DRF `ListField.get_value` → `getlist`) and which scalar fields treat a
/// present-but-empty value as absent (the `not required and not allow_blank`
/// blank-input rule — `fields.py:407-429`).
pub struct BodySpec {
    /// Keys whose repeated form values arrive as a JSON array (all values,
    /// text then files). Every other key arrives as its last value.
    pub list_fields: &'static [&'static str],
    /// Scalar keys where form `key=` behaves as if the key were absent
    /// (JSON `{"key": ""}` instead runs `to_internal_value` and usually
    /// 400s). Verified live per field; see the `R2`/`S` probe series.
    pub skip_blank_fields: &'static [&'static str],
}

/// Cycle write paths: no list fields; the two datetimes and the timezone
/// choice skip blank form input.
pub const CYCLE_BODY_SPEC: BodySpec = BodySpec {
    list_fields: &[],
    skip_blank_fields: &["start_date", "end_date", "timezone"],
};

/// Module write paths: `members` is a `ListField`; the two dates and the
/// status choice skip blank form input.
pub const MODULE_BODY_SPEC: BodySpec = BodySpec {
    list_fields: &["members"],
    skip_blank_fields: &["start_date", "target_date", "status"],
};

/// One uploaded file part (multipart). Content is carried for the
/// serialization-failure and empty-file arms; D-20 never reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePart {
    /// Sanitized filename (`sanitize_file_name`): path-stripped,
    /// entity-unescaped, non-printables removed.
    pub filename: String,
    /// The part's content type (empty when the part carries none).
    pub content_type: String,
    /// Raw (transfer-decoded) file bytes.
    pub bytes: Vec<u8>,
}

/// Uploads per key, in arrival order (DRF merges files into
/// `request.data`, so `in` checks consult this map too).
pub type FilesMap = BTreeMap<String, Vec<FilePart>>;

/// A negotiated form/multipart body: the text map plus uploads per key.
pub type FormMaps = (Map<String, Value>, FilesMap);

/// A negotiated non-JSON body: text values plus uploaded files.
///
/// Mirrors DRF's `_full_data` (`data.copy().update(files)` — Django's
/// `MultiValueDict.update` *extends* key lists, so per key the values are
/// the text values followed by the file values).
#[derive(Debug, Clone, Default)]
pub struct FormBody {
    /// Text values per key, in arrival order (QueryDict lists). A
    /// `BTreeMap`: `serde_json::Map`'s methods exist only on the concrete
    /// `Map<String, Value>`.
    pub texts: BTreeMap<String, Vec<String>>,
    /// File values per key, in arrival order.
    pub files: FilesMap,
}

impl FormBody {
    /// Last value wins (`QueryDict.__getitem__` / `.get`): the last file
    /// when the key carries any file, else the last text value.
    pub fn last_is_file(&self, key: &str) -> bool {
        self.files.get(key).is_some_and(|v| !v.is_empty())
    }

    /// All values for a list field (`getlist`): texts then files.
    pub fn has_key(&self, key: &str) -> bool {
        self.texts.contains_key(key) || self.files.contains_key(key)
    }
}

/// The negotiated body: empty, decoded JSON text for the caller's existing
/// serde path, or a parsed HTML form.
#[derive(Debug, Clone)]
pub enum NegotiatedBody {
    /// Content-Length says empty: `{}` whatever the content type is.
    Empty,
    /// Decoded JSON source text (charset applied, trailing incomplete
    /// sequence dropped). The caller parses it with its existing
    /// serde + CPython-error mapping.
    JsonText(String),
    /// Parsed form/multipart body with HTML-input semantics applied per
    /// `spec` (list fields as arrays, blank skips). `map` holds text-only
    /// values; `files` holds uploads per key (DRF merges files into
    /// `request.data`, so `in` checks must consult both maps).
    Form {
        map: Map<String, Value>,
        files: FilesMap,
    },
}

/// Body-layer failure, before the caller's envelope mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyError {
    /// 415 with this `detail` message (`Unsupported media type ...`).
    UnsupportedMediaType(String),
    /// 400 `{"detail": ...}` (`ParseError` text).
    ParseDetail(String),
    /// Generic 500 (`TooManyFieldsSent` / `TooManyFilesSent` /
    /// `RequestDataTooBig`, which `BaseAPIView.handle_exception` does not
    /// special-case).
    ServerError,
}

/// Negotiate + parse the request body (`Request._load_stream` +
/// `_parse` + `select_parser`).
///
/// `headers`/`body` are the raw axum inputs. `spec` carries the domain's
/// HTML-input shape. The JSON arm returns decoded text for the caller's
/// existing parser; only form/multipart produce value maps here.
pub fn negotiate_body(
    headers: &HeaderMap,
    body: &[u8],
    spec: &BodySpec,
) -> Result<NegotiatedBody, BodyError> {
    if content_length_is_empty(headers) {
        return Ok(NegotiatedBody::Empty);
    }
    let content_type = header_str(headers, "content-type");
    match select_parser(&content_type) {
        Parser::Json => {
            let text = decode_json_body(body, &content_type)?;
            Ok(NegotiatedBody::JsonText(text))
        }
        Parser::Form => {
            let form = parse_form_body(body, &content_type)?;
            Ok(build_form_body(form, spec))
        }
        Parser::Multipart => {
            let form = parse_multipart_body(body, &content_type, headers)?;
            Ok(build_form_body(form, spec))
        }
        Parser::None => Err(BodyError::UnsupportedMediaType(format!(
            "Unsupported media type \"{content_type}\" in request."
        ))),
    }
}

/// Apply the domain spec to a parsed form: list fields become arrays, blank
/// skips drop keys, every other key keeps its last text value.
fn build_form_body(form: FormBody, spec: &BodySpec) -> NegotiatedBody {
    let mut map = Map::new();
    let mut keys: Vec<&String> = form.texts.keys().collect();
    keys.sort();
    for key in keys {
        let values = &form.texts[key.as_str()];
        if spec.list_fields.contains(&key.as_str()) {
            let mut items: Vec<Value> = values.iter().map(|v| Value::String(v.clone())).collect();
            // Files extend the value list (same key) but are not JSON
            // values; the array holds texts only and the files map keeps
            // the uploads for the caller's per-field handling.
            let _ = &mut items;
            map.insert(key.clone(), Value::Array(items));
        } else if let Some(last) = values.last() {
            if last.is_empty() && spec.skip_blank_fields.contains(&key.as_str()) {
                continue;
            }
            map.insert(key.clone(), Value::String(last.clone()));
        }
    }
    // List fields whose only values are files still arrive as arrays (an
    // empty array here; the caller reads the files map for the items).
    for key in spec.list_fields {
        if !map.contains_key(*key) && form.files.contains_key(*key) {
            map.insert((*key).to_owned(), Value::Array(Vec::new()));
        }
    }
    NegotiatedBody::Form {
        map,
        files: form.files,
    }
}

/// Raw header value as sent (`HeaderMap` strips only surrounding whitespace,
/// like the WSGI servers do). Missing headers are the empty string, which
/// is what Django sees under prod (uvicorn/ASGI omits the key); runserver's
/// `text/plain` default for a missing content type is a wsgiref artifact
/// the port deliberately does not reproduce (same JSON as prod Django).
fn header_str(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

/// `_load_stream`: empty unless the Content-Length header parses (Python
/// `int` semantics) to a nonzero value. The actual byte count is
/// irrelevant — Django keys off the header alone.
fn content_length_is_empty(headers: &HeaderMap) -> bool {
    let Some(raw) = headers.get("content-length").and_then(|v| v.to_str().ok()) else {
        return true;
    };
    !python_int_is_nonzero(raw)
}

/// Python `int(s)` success + nonzero check, for header values: ASCII
/// whitespace stripped, one optional sign, ASCII digits, at least one
/// nonzero digit. (Unicodedigits would parse under CPython but hyper
/// rejects non-ASCII headers before the app ever sees them.)
fn python_int_is_nonzero(raw: &str) -> bool {
    let trimmed = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.bytes().any(|b| b != b'0')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Parser {
    Json,
    Form,
    Multipart,
    None,
}

/// `DefaultContentNegotiation.select_parser` over the default parser order
/// (JSON, form, multipart): first `media_type_matches` win. The match is on
/// the lowercased base type with `*` wildcards honored on the request side
/// (`*/*` and `application/*` select the JSON parser — verified live).
fn select_parser(content_type: &str) -> Parser {
    let (base, _) = parse_header_parameters(content_type);
    let (main, sub) = split_media_type(&base);
    for parser in [
        "application/json",
        "application/x-www-form-urlencoded",
        "multipart/form-data",
    ] {
        let (pmain, psub) = split_media_type(parser);
        if media_type_matches((pmain, psub), (main.clone(), sub.clone())) {
            return match parser {
                "application/json" => Parser::Json,
                "application/x-www-form-urlencoded" => Parser::Form,
                _ => Parser::Multipart,
            };
        }
    }
    Parser::None
}

fn split_media_type(base: &str) -> (String, String) {
    match base.split_once('/') {
        Some((main, sub)) => (main.to_owned(), sub.to_owned()),
        None => (base.to_owned(), String::new()),
    }
}

/// `_MediaType.match` with the parser side holding no params and no
/// wildcards: every parser param (none) must match, and each request side
/// accepts `*`.
fn media_type_matches(parser: (String, String), request: (String, String)) -> bool {
    let (pmain, psub) = parser;
    let (rmain, rsub) = request;
    if psub != "*" && rsub != "*" && rsub != psub {
        return false;
    }
    if pmain != "*" && rmain != "*" && rmain != pmain {
        return false;
    }
    true
}

/// `django/utils/http.py:parse_header_parameters`: quote-aware `;` split,
/// lowercased main value, lowercased param names, verbatim values with
/// quotes stripped and backslash-escapes collapsed, RFC 2231 `name*`
/// decoding.
fn parse_header_parameters(line: &str) -> (String, Vec<(String, String)>) {
    let mut parts = split_header_params(line);
    let main = parts.next().unwrap_or_default().to_lowercase();
    let mut params = Vec::new();
    for part in parts {
        let Some(eq) = part.find('=') else { continue };
        let mut name = part[..eq].trim().to_lowercase();
        let mut encoded = false;
        if let Some(stripped) = name.strip_suffix('*') {
            name = stripped.to_owned();
            if part.matches('\'').count() == 2 {
                encoded = true;
            }
        }
        let mut value = part[eq + 1..].trim().to_owned();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = value[1..value.len() - 1].to_owned();
            value = value.replace("\\\\", "\\").replace("\\\"", "\"");
        }
        if encoded {
            let mut pieces = value.splitn(3, '\'');
            if let (Some(charset), Some(_lang), Some(raw)) =
                (pieces.next(), pieces.next(), pieces.next())
            {
                value = percent_decode_str(raw, &charset_to_supported(charset));
            }
        }
        params.push((name, value));
    }
    (main, params)
}

/// `django/utils/http.py:_parseparam`: split on `;` that is outside an odd
/// quote run, strip each piece.
fn split_header_params(line: &str) -> impl Iterator<Item = String> + '_ {
    let mut rest = line;
    let mut done = rest.is_empty();
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        if rest.is_empty() {
            done = true;
            return Some(String::new());
        }
        let bytes = rest.as_bytes();
        let mut end = bytes.iter().position(|b| *b == b';').unwrap_or(bytes.len());
        loop {
            let head = &rest[..end];
            let quotes = head.bytes().filter(|b| *b == b'"').count();
            let escaped = head.as_bytes().windows(2).filter(|w| w == b"\\\"").count();
            if (quotes - escaped) % 2 == 0 || end >= bytes.len() {
                break;
            }
            match bytes[end + 1..].iter().position(|b| *b == b';') {
                Some(rel) => end = end + 1 + rel,
                None => {
                    end = bytes.len();
                    break;
                }
            }
        }
        let piece = rest[..end].trim().to_owned();
        rest = if end < bytes.len() {
            &rest[end + 1..]
        } else {
            ""
        };
        if rest.is_empty() && end >= bytes.len() {
            done = true;
        }
        Some(piece)
    })
}

/// Charsets the port resolves (`codecs.lookup` over the C builtins; every
/// other valid-or-bogus name degrades to utf-8, exactly like a bogus name
/// does under Django — exotic codecs are a filed follow-up, not a stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupportedCharset {
    Utf8,
    Ascii,
    Latin1,
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
}

/// Resolve a `charset=` parameter the way `request._set_content_type_params`
/// plus `codecs.lookup` do: `normalize_encoding` (non-alphanumerics collapse
/// to `_`, non-ASCII alphanumerics vanish), lowercase (the C builtins match
/// case-insensitively on every platform), then the `encodings.aliases`
/// table for the five supported families. Anything else is utf-8.
fn charset_to_supported(raw: &str) -> SupportedCharset {
    let mut normalized = String::with_capacity(raw.len());
    let mut punct = false;
    for c in raw.chars() {
        if c.is_alphanumeric() || c == '.' {
            if punct && !normalized.is_empty() {
                normalized.push('_');
            }
            if c.is_ascii() {
                normalized.push(c.to_ascii_lowercase());
            }
            punct = false;
        } else {
            punct = true;
        }
    }
    if normalized.is_empty() {
        return SupportedCharset::Utf8;
    }
    let dotted = normalized.replace('.', "_");
    let key = |candidate: &str| {
        if let Some(hit) = supported_alias(candidate) {
            return Some(hit);
        }
        None
    };
    key(&normalized)
        .or_else(|| key(&dotted))
        .unwrap_or(SupportedCharset::Utf8)
}

/// Canonical names + `encodings.aliases` entries for the five families.
fn supported_alias(normalized: &str) -> Option<SupportedCharset> {
    Some(match normalized {
        "utf_8" | "u8" | "utf" | "utf8" | "utf8_ucs2" | "utf8_ucs4" | "cp65001" => {
            SupportedCharset::Utf8
        }
        "ascii" | "646" | "ansi_x3_4_1968" | "ansi_x3_4_1986" | "cp367" | "csascii" | "ibm367"
        | "iso646_us" | "iso_646_irv_1991" | "iso_ir_6" | "us" | "us_ascii" => {
            SupportedCharset::Ascii
        }
        "latin_1" | "8859" | "cp819" | "csisolatin1" | "ibm819" | "iso8859" | "iso8859_1"
        | "iso_8859_1" | "iso_8859_1_1987" | "iso_ir_100" | "l1" | "latin" | "latin1" => {
            SupportedCharset::Latin1
        }
        "utf_16" | "u16" | "utf16" => SupportedCharset::Utf16,
        "utf_16_le" | "unicodelittleunmarked" | "utf_16le" => SupportedCharset::Utf16Le,
        "utf_16_be" | "unicodebigunmarked" | "utf_16be" => SupportedCharset::Utf16Be,
        "utf_32" | "u32" | "utf32" => SupportedCharset::Utf32,
        "utf_32_le" | "utf_32le" => SupportedCharset::Utf32Le,
        "utf_32_be" | "utf_32be" => SupportedCharset::Utf32Be,
        _ => return None,
    })
}

/// The charset parameter of a content type (verbatim value; matching is
/// case-insensitive on the parameter name via `parse_header_parameters`).
fn content_type_charset(content_type: &str) -> SupportedCharset {
    let (_, params) = parse_header_parameters(content_type);
    params
        .iter()
        .find(|(name, _)| name == "charset")
        .map(|(_, value)| charset_to_supported(value))
        .unwrap_or(SupportedCharset::Utf8)
}

/// Decode a JSON body: `codecs.getreader(charset)(stream)` semantics. The
/// stream decode drops a trailing *incomplete* sequence (utf-8 lead
/// prefix, utf-16 odd byte or pending high surrogate, utf-32 short tail)
/// and raises on everything else, with CPython's exact texts.
fn decode_json_body(body: &[u8], content_type: &str) -> Result<String, BodyError> {
    decode_stream_body(body, content_type_charset(content_type))
        .map_err(|detail| BodyError::ParseDetail(format!("JSON parse error - {detail}")))
}

fn decode_stream_body(body: &[u8], charset: SupportedCharset) -> Result<String, String> {
    match charset {
        SupportedCharset::Utf8 => decode_stream_utf8(body),
        SupportedCharset::Ascii => decode_stream_ascii(body),
        SupportedCharset::Latin1 => Ok(body.iter().map(|b| *b as char).collect()),
        SupportedCharset::Utf16 => decode_stream_utf16(body, None),
        SupportedCharset::Utf16Le => decode_stream_utf16(body, Some(false)),
        SupportedCharset::Utf16Be => decode_stream_utf16(body, Some(true)),
        SupportedCharset::Utf32 => decode_stream_utf32(body, None),
        SupportedCharset::Utf32Le => decode_stream_utf32(body, Some(false)),
        SupportedCharset::Utf32Be => decode_stream_utf32(body, Some(true)),
    }
}

/// Strict one-shot decode for the form layer-1 (`QueryDict(bytes)` calls
/// `bytes.decode(encoding)`): tails fail here (no stream drop); only
/// success/failure matters because any failure falls back to latin-1.
fn decode_oneshot_strict(body: &[u8], charset: SupportedCharset) -> Option<String> {
    match charset {
        SupportedCharset::Utf8 => std::str::from_utf8(body).ok().map(str::to_owned),
        SupportedCharset::Ascii => {
            if body.is_ascii() {
                Some(body.iter().map(|b| *b as char).collect())
            } else {
                None
            }
        }
        SupportedCharset::Latin1 => Some(body.iter().map(|b| *b as char).collect()),
        SupportedCharset::Utf16 => decode_oneshot_utf16(body, None),
        SupportedCharset::Utf16Le => decode_oneshot_utf16(body, Some(false)),
        SupportedCharset::Utf16Be => decode_oneshot_utf16(body, Some(true)),
        SupportedCharset::Utf32 => decode_oneshot_utf32(body, None),
        SupportedCharset::Utf32Le => decode_oneshot_utf32(body, Some(false)),
        SupportedCharset::Utf32Be => decode_oneshot_utf32(body, Some(true)),
    }
}

/// Lossy one-shot decode (`force_str(..., errors="replace")`, `unquote`
/// runs, RFC 2231 values): undecodable spans become U+FFFD. The BOM
/// codecs assume little-endian when no BOM is present (verified against
/// CPython) and incomplete tails become a single U+FFFD.
fn decode_oneshot_replace(bytes: &[u8], charset: SupportedCharset) -> String {
    match charset {
        SupportedCharset::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        SupportedCharset::Ascii => bytes
            .iter()
            .map(|b| if b.is_ascii() { *b as char } else { '\u{FFFD}' })
            .collect(),
        SupportedCharset::Latin1 => bytes.iter().map(|b| *b as char).collect(),
        SupportedCharset::Utf16 => decode_replace_utf16(bytes, None),
        SupportedCharset::Utf16Le => decode_replace_utf16(bytes, Some(false)),
        SupportedCharset::Utf16Be => decode_replace_utf16(bytes, Some(true)),
        SupportedCharset::Utf32 => decode_replace_utf32(bytes, None),
        SupportedCharset::Utf32Le => decode_replace_utf32(bytes, Some(false)),
        SupportedCharset::Utf32Be => decode_replace_utf32(bytes, Some(true)),
    }
}

fn decode_stream_utf8(body: &[u8]) -> Result<String, String> {
    match std::str::from_utf8(body) {
        Ok(text) => Ok(text.to_owned()),
        Err(error) => {
            if utf8_trailing_valid_prefix(body, error.valid_up_to()) {
                // The stream decode holds an incomplete-but-valid tail and
                // never flushes it: the bytes vanish (W2 probe).
                Ok(String::from_utf8_lossy(&body[..error.valid_up_to()]).into_owned())
            } else {
                Err(super::json_cpython::utf8_decode_detail(body))
            }
        }
    }
}

/// Whether the bytes at `start..` are a strict prefix of a valid sequence
/// (lead + well-ranged continuations so far, missing at least one byte).
/// Range-invalid truncations (`\xed\xa0` + EOF) still error — the range
/// check fires before EOF matters (same rules as `utf8_decode_detail`).
fn utf8_trailing_valid_prefix(body: &[u8], start: usize) -> bool {
    let lead = body[start];
    let expected: Option<usize> = match lead {
        0xC2..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
    };
    let Some(expected) = expected else {
        return false;
    };
    let mut run = 1;
    while run < expected && start + run < body.len() && (0x80..=0xBF).contains(&body[start + run]) {
        run += 1;
    }
    if run == expected || start + run != body.len() {
        return false;
    }
    if run >= 2 {
        let second = body[start + 1];
        let in_range = match lead {
            0xE0 => (0xA0..=0xBF).contains(&second),
            0xED => (0x80..=0x9F).contains(&second),
            0xF0 => (0x90..=0xBF).contains(&second),
            0xF4 => (0x80..=0x8F).contains(&second),
            _ => true,
        };
        if !in_range {
            return false;
        }
    }
    true
}

fn decode_stream_ascii(body: &[u8]) -> Result<String, String> {
    match body.iter().position(|b| !b.is_ascii()) {
        None => Ok(body.iter().map(|b| *b as char).collect()),
        Some(position) => Err(format!(
            "'ascii' codec can't decode byte 0x{:02x} in position {position}: ordinal not in range(128)",
            body[position]
        )),
    }
}

fn decode_stream_utf16(body: &[u8], big_endian: Option<bool>) -> Result<String, String> {
    let (units, big_endian) = split_utf16_units(body, big_endian)?;
    let codec = if big_endian { "utf-16-be" } else { "utf-16-le" };
    let mut out = String::new();
    let mut index = 0;
    while index < units.len() {
        let (value, at) = units[index];
        if (0xD800..0xDC00).contains(&value) {
            match units.get(index + 1) {
                None => break, // Pending high surrogate at end: dropped.
                Some((low, _)) if (0xDC00..0xE000).contains(low) => {
                    let high = (value - 0xD800) as u32;
                    let low = (low - 0xDC00) as u32;
                    out.push(char::from_u32(0x10000 + (high << 10) + low).expect("astral"));
                    index += 2;
                }
                _ => {
                    return Err(format!(
                        "'{codec}' codec can't decode bytes in position {at}-{}: illegal UTF-16 surrogate",
                        at + 1
                    ));
                }
            }
        } else if (0xDC00..0xE000).contains(&value) {
            return Err(format!(
                "'{codec}' codec can't decode bytes in position {at}-{}: illegal encoding",
                at + 1
            ));
        } else {
            out.push(char::from_u32(value as u32).expect("BMP scalar"));
            index += 1;
        }
    }
    Ok(out)
}

/// Split off the BOM (required when `big_endian` is `None`) and pair the
/// rest into units with byte offsets; a trailing odd byte is dropped.
fn split_utf16_units(
    body: &[u8],
    big_endian: Option<bool>,
) -> Result<(Vec<(u16, usize)>, bool), String> {
    // Error positions are absolute in the stream: the consumed BOM still
    // counts (CPython reports the lone-low after a BOM at 4-5, not 2-3).
    let base = if big_endian.is_none() { 2 } else { 0 };
    let (body, big_endian) = match big_endian {
        Some(big_endian) => (body, big_endian),
        None => {
            if let Some(rest) = body.strip_prefix(b"\xFF\xFE") {
                (rest, false)
            } else if let Some(rest) = body.strip_prefix(b"\xFE\xFF") {
                (rest, true)
            } else {
                return Err("UTF-16 stream does not start with BOM".to_owned());
            }
        }
    };
    let mut units = Vec::with_capacity(body.len() / 2);
    let mut offset = 0;
    while offset + 1 < body.len() {
        let pair = [body[offset], body[offset + 1]];
        let value = if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        };
        units.push((value, base + offset));
        offset += 2;
    }
    Ok((units, big_endian))
}

fn decode_stream_utf32(body: &[u8], big_endian: Option<bool>) -> Result<String, String> {
    let (words, big_endian) = split_utf32_units(body, big_endian)?;
    let codec = if big_endian { "utf-32-be" } else { "utf-32-le" };
    let mut out = String::new();
    for (value, at) in words {
        if (0xD800..0xE000).contains(&value) {
            return Err(format!(
                "'{codec}' codec can't decode bytes in position {at}-{}: code point in surrogate code point range(0xd800, 0xe000)",
                at + 3
            ));
        }
        if value > 0x10FFFF {
            return Err(format!(
                "'{codec}' codec can't decode bytes in position {at}-{}: code point not in range(0x110000)",
                at + 3
            ));
        }
        out.push(char::from_u32(value).expect("valid scalar"));
    }
    Ok(out)
}

fn split_utf32_units(
    body: &[u8],
    big_endian: Option<bool>,
) -> Result<(Vec<(u32, usize)>, bool), String> {
    // Absolute stream positions: the consumed BOM still counts.
    let base = if big_endian.is_none() { 4 } else { 0 };
    let (body, big_endian) = match big_endian {
        Some(big_endian) => (body, big_endian),
        None => {
            if let Some(rest) = body.strip_prefix(b"\xFF\xFE\x00\x00") {
                (rest, false)
            } else if let Some(rest) = body.strip_prefix(b"\x00\x00\xFE\xFF") {
                (rest, true)
            } else {
                return Err("UTF-32 stream does not start with BOM".to_owned());
            }
        }
    };
    let mut words = Vec::with_capacity(body.len() / 4);
    let mut offset = 0;
    while offset + 3 < body.len() {
        let word = [
            body[offset],
            body[offset + 1],
            body[offset + 2],
            body[offset + 3],
        ];
        let value = if big_endian {
            u32::from_be_bytes(word)
        } else {
            u32::from_le_bytes(word)
        };
        words.push((value, base + offset));
        offset += 4;
    }
    Ok((words, big_endian))
}

fn decode_oneshot_utf16(body: &[u8], big_endian: Option<bool>) -> Option<String> {
    // Strict one-shot: the BOM (when required) plus whole units must
    // consume every byte; odd tails and lone surrogates fail (and the
    // form layer falls back to latin-1).
    let bom_len = if big_endian.is_some() { 0 } else { 2 };
    if body.len() < bom_len || !(body.len() - bom_len).is_multiple_of(2) {
        return None;
    }
    let (units, _) = split_utf16_units(body, big_endian).ok()?;
    let mut out = String::new();
    let mut index = 0;
    while index < units.len() {
        let (value, _) = units[index];
        if (0xD800..0xDC00).contains(&value) {
            match units.get(index + 1) {
                Some((low, _)) if (0xDC00..0xE000).contains(low) => {
                    let high = (value - 0xD800) as u32;
                    let low = (low - 0xDC00) as u32;
                    out.push(char::from_u32(0x10000 + (high << 10) + low)?);
                    index += 2;
                }
                _ => return None,
            }
        } else if (0xDC00..0xE000).contains(&value) {
            return None;
        } else {
            out.push(char::from_u32(value as u32)?);
            index += 1;
        }
    }
    Some(out)
}

fn decode_oneshot_utf32(body: &[u8], big_endian: Option<bool>) -> Option<String> {
    let (words, _) = split_utf32_units(body, big_endian).ok()?;
    let bom = if big_endian.is_some() { 0 } else { 4 };
    if bom + words.len() * 4 != body.len() {
        return None;
    }
    let mut out = String::new();
    for (value, _) in words {
        // `from_u32` rejects surrogates and out-of-range values alike.
        out.push(char::from_u32(value)?);
    }
    Some(out)
}

fn decode_replace_utf16(bytes: &[u8], big_endian: Option<bool>) -> String {
    let (body, big_endian) = match big_endian {
        Some(big_endian) => (bytes, big_endian),
        None => {
            if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
                (rest, false)
            } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
                (rest, true)
            } else {
                (bytes, false)
            }
        }
    };
    let mut out = String::new();
    let mut offset = 0;
    while offset < body.len() {
        if offset + 1 >= body.len() {
            out.push('\u{FFFD}');
            break;
        }
        let pair = [body[offset], body[offset + 1]];
        let value = if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        };
        if (0xD800..0xDC00).contains(&value) {
            if offset + 3 < body.len() {
                let next = [body[offset + 2], body[offset + 3]];
                let low = if big_endian {
                    u16::from_be_bytes(next)
                } else {
                    u16::from_le_bytes(next)
                };
                if (0xDC00..0xE000).contains(&low) {
                    let high = (value - 0xD800) as u32;
                    let low = (low - 0xDC00) as u32;
                    out.push(char::from_u32(0x10000 + (high << 10) + low).expect("astral"));
                    offset += 4;
                    continue;
                }
            }
            out.push('\u{FFFD}');
            offset += 2;
        } else if (0xDC00..0xE000).contains(&value) {
            out.push('\u{FFFD}');
            offset += 2;
        } else {
            out.push(char::from_u32(value as u32).expect("BMP scalar"));
            offset += 2;
        }
    }
    out
}

fn decode_replace_utf32(bytes: &[u8], big_endian: Option<bool>) -> String {
    let (body, big_endian) = match big_endian {
        Some(big_endian) => (bytes, big_endian),
        None => {
            if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE\x00\x00") {
                (rest, false)
            } else if let Some(rest) = bytes.strip_prefix(b"\x00\x00\xFE\xFF") {
                (rest, true)
            } else {
                (bytes, false)
            }
        }
    };
    let mut out = String::new();
    let mut offset = 0;
    while offset < body.len() {
        if offset + 3 >= body.len() {
            out.push('\u{FFFD}');
            break;
        }
        let word = [
            body[offset],
            body[offset + 1],
            body[offset + 2],
            body[offset + 3],
        ];
        let value = if big_endian {
            u32::from_be_bytes(word)
        } else {
            u32::from_le_bytes(word)
        };
        match char::from_u32(value) {
            Some(c) => out.push(c),
            None => out.push('\u{FFFD}'),
        }
        offset += 4;
    }
    out
}

/// Percent-decode with a charset (`unquote(..., errors="replace")`):
/// `+` is already handled by the caller for form data. ASCII runs go
/// through `%XX` unescaping + `decode(encoding, replace)`; non-ASCII
/// chars pass through verbatim (`_generate_unquoted_parts`).
fn percent_decode_str(raw: &str, charset: &SupportedCharset) -> String {
    let mut out = String::new();
    let mut run = Vec::new();
    for c in raw.chars() {
        if c.is_ascii() {
            run.push(c as u8);
        } else {
            if !run.is_empty() {
                out.push_str(&decode_oneshot_replace(&percent_unescape(&run), *charset));
                run.clear();
            }
            out.push(c);
        }
    }
    if !run.is_empty() {
        out.push_str(&decode_oneshot_replace(&percent_unescape(&run), *charset));
    }
    out
}

/// `_unquote_impl`: `%` + two hex digits becomes the byte; anything else
/// (short tail, non-hex) stays literal. No double-decoding.
fn percent_unescape(run: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(run.len());
    let mut index = 0;
    while index < run.len() {
        if run[index] == b'%' && index + 2 < run.len() + 1 {
            if let (Some(high), Some(low)) = (
                hex_val(*run.get(index + 1).unwrap_or(&b' ')),
                hex_val(*run.get(index + 2).unwrap_or(&b' ')),
            ) {
                out.push(high << 4 | low);
                index += 3;
                continue;
            }
        }
        out.push(run[index]);
        index += 1;
    }
    out
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `FormParser.parse`: `QueryDict(raw, encoding)` — strict layer-1
/// decode with whole-body latin-1 fallback, then `parse_qsl` (`&`-only
/// split, first-`=` partition, `+`→space, `%XX` with `errors="replace"`).
/// More than `DATA_UPLOAD_MAX_NUMBER_FIELDS` (1000) `&`-segments is the
/// generic 500 (`TooManyFieldsSent`).
fn parse_form_body(body: &[u8], content_type: &str) -> Result<FormBody, BodyError> {
    let charset = content_type_charset(content_type);
    let text = decode_oneshot_strict(body, charset)
        .unwrap_or_else(|| body.iter().map(|b| *b as char).collect());
    if text.is_empty() {
        return Ok(FormBody::default());
    }
    // `parse_qsl` counts segments before parsing: 1 + separator count.
    if 1 + text.bytes().filter(|b| *b == b'&').count() > 1000 {
        return Err(BodyError::ServerError);
    }
    let mut form = FormBody::default();
    for segment in text.split('&') {
        if segment.is_empty() {
            continue;
        }
        let (name, value) = match segment.split_once('=') {
            Some((name, value)) => (name, value),
            // No `=`: kept with a blank value (`keep_blank_values`).
            None => (segment, ""),
        };
        let key = percent_decode_str(&name.replace('+', " "), &charset);
        let val = percent_decode_str(&value.replace('+', " "), &charset);
        form.texts.entry(key).or_default().push(val);
    }
    Ok(form)
}

/// `MultiPartParser.parse` + Django's `MultipartParser`: boundary checks,
/// naive substring part splitting, upload-handler-neutral buffering (file
/// bytes are carried, never spooled), field/file assembly with the exact
/// limits and error texts.
fn parse_multipart_body(
    body: &[u8],
    content_type: &str,
    headers: &HeaderMap,
) -> Result<FormBody, BodyError> {
    if !content_type.is_ascii() {
        return Err(BodyError::ParseDetail(format!(
            "Multipart form parse error - Invalid non-ASCII Content-Type in multipart: {content_type}"
        )));
    }
    if !content_type.starts_with("multipart/") {
        // Case-sensitive, on the full header (`P19` probe).
        return Err(BodyError::ParseDetail(format!(
            "Multipart form parse error - Invalid Content-Type: {content_type}"
        )));
    }
    let (_, params) = parse_header_parameters(content_type);
    let boundary = params
        .iter()
        .find(|(name, _)| name == "boundary")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    if boundary.is_empty() && !params.iter().any(|(name, _)| name == "boundary") {
        return Err(BodyError::ParseDetail(
            "Multipart form parse error - Invalid boundary in multipart: None".to_owned(),
        ));
    }
    if !valid_multipart_boundary(&boundary) {
        return Err(BodyError::ParseDetail(format!(
            "Multipart form parse error - Invalid boundary in multipart: {boundary}"
        )));
    }
    if let Some(cl) = headers.get("content-length").and_then(|v| v.to_str().ok()) {
        // Unreachable through hyper (it rejects such lengths first), but
        // the check is cheap and the text is pinned by a unit test.
        if let Ok(length) = cl.trim().parse::<i128>() {
            if length < 0 {
                return Err(BodyError::ParseDetail(format!(
                    "Multipart form parse error - Invalid content length: {length}"
                )));
            }
        }
    }
    let charset = content_type_charset(content_type);
    parse_multipart_parts(body, &boundary, &charset)
}

/// `boundary_re = [ -~]{0,200}[!-~]` full match (printable ASCII, 1..=201
/// bytes, no trailing space).
fn valid_multipart_boundary(boundary: &str) -> bool {
    let bytes = boundary.as_bytes();
    if bytes.is_empty() || bytes.len() > 201 {
        return false;
    }
    if !bytes.iter().all(|b| (0x20..=0x7E).contains(b)) {
        return false;
    }
    *bytes.last().expect("nonempty checked") != b' '
}

/// The part loop (`MultipartParser.parse` + `Parser` + `InterBoundaryIter`
/// plus `BoundaryIter`): split the stream at every `--boundary` occurrence,
/// strip one trailing CRLF from each part, parse `<=1024`-byte headers,
/// and assemble fields/files with Django's counting.
fn parse_multipart_parts(
    body: &[u8],
    boundary: &str,
    charset: &SupportedCharset,
) -> Result<FormBody, BodyError> {
    let separator = [b"--", boundary.as_bytes()].concat();
    // `BoundaryIter`: raw substring search (no CRLF-prefix requirement).
    let mut chunks: Vec<&[u8]> = Vec::new();
    let mut rest = body;
    loop {
        match find_subsequence(rest, &separator) {
            None => {
                chunks.push(rest);
                break;
            }
            Some(at) => {
                chunks.push(&rest[..at]);
                rest = &rest[at + separator.len()..];
            }
        }
    }
    let mut form = FormBody::default();
    let mut num_post_keys: usize = 0;
    let mut num_files: usize = 0;
    let mut num_bytes_read: usize = 0;
    // Every chunk (preamble, parts, epilogue) runs the item loop; the
    // preamble/epilogue only ever yield RAW items, which still count.
    for chunk in chunks {
        let content = strip_one_crlf(chunk);
        let item = classify_part(content);
        match item {
            PartItem::Raw | PartItem::Nameless => {
                num_post_keys += 1;
                if 1000 + 2 < num_post_keys {
                    return Err(BodyError::ServerError);
                }
            }
            PartItem::Field { name, data } => {
                num_post_keys += 1;
                if 1000 + 2 < num_post_keys {
                    return Err(BodyError::ServerError);
                }
                num_bytes_read += data.len() + name.len() + 2;
                if num_bytes_read > 5_242_880 {
                    return Err(BodyError::ServerError);
                }
                let key = decode_oneshot_replace(&name, *charset);
                let mut value = data;
                if item_transfer_is_base64(content) {
                    // Fields are lenient: undecodable base64 keeps the
                    // raw bytes (`P33` probe).
                    if let Ok(decoded) = binascii_b64decode(&value) {
                        value = decoded;
                    }
                }
                form.texts
                    .entry(key)
                    .or_default()
                    .push(decode_oneshot_replace(&value, *charset));
            }
            PartItem::File {
                name,
                filename,
                content_type,
                data,
            } => {
                num_files += 1;
                if 100 < num_files {
                    return Err(BodyError::ServerError);
                }
                let filename = sanitize_file_name(&decode_oneshot_replace(&filename, *charset));
                let Some(filename) = filename else { continue };
                let mut value = data;
                if item_transfer_is_base64(content) {
                    value = binascii_b64decode(&value).map_err(|_| {
                        BodyError::ParseDetail(
                            "Multipart form parse error - Could not decode base64 data.".to_owned(),
                        )
                    })?;
                }
                let key = decode_oneshot_replace(&name, *charset);
                form.files.entry(key).or_default().push(FilePart {
                    filename,
                    content_type: String::from_utf8_lossy(&content_type).into_owned(),
                    bytes: value,
                });
            }
        }
    }
    Ok(form)
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|at| &haystack[*at..*at + needle.len()] == needle)
}

/// `BoundaryIter` CRLF backup: strip one trailing `\n`, then one
/// trailing `\r` (independently).
fn strip_one_crlf(content: &[u8]) -> &[u8] {
    let mut end = content.len();
    if end > 0 && content[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && content[end - 1] == b'\r' {
        end -= 1;
    }
    &content[..end]
}

enum PartItem {
    /// No content-disposition (preamble/epilogue/garbage): skipped, but
    /// counted toward the field limit.
    Raw,
    /// Disposition without a name: skipped, but still counted as a FIELD
    /// item (the increment precedes the name check in the Python loop).
    Nameless,
    Field {
        name: Vec<u8>,
        data: Vec<u8>,
    },
    File {
        name: Vec<u8>,
        filename: Vec<u8>,
        content_type: Vec<u8>,
        data: Vec<u8>,
    },
}

/// `Parser.parse_boundary_stream` + the disposition split: headers end at
/// the first `\r\n\r\n` inside a 1024-byte window; files are parts whose
/// disposition carries a nonempty filename. Later duplicate lines
/// overwrite earlier ones.
fn classify_part(content: &[u8]) -> PartItem {
    let window = content.len().min(1024);
    let Some(end) = find_subsequence(&content[..window], b"\r\n\r\n") else {
        return PartItem::Raw;
    };
    let (head, data) = (&content[..end], &content[end + 4..]);
    let mut disposition: Option<Vec<(Vec<u8>, Vec<u8>)>> = None;
    let mut content_type: Vec<u8> = Vec::new();
    for line in head.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Ok(line) = std::str::from_utf8(line) else {
            // `UnicodeDecodeError` is a `ValueError`: the line is skipped
            // (`P22` probe).
            continue;
        };
        let (name, main_value, params) = parse_header_line(line);
        if name.is_empty() {
            continue;
        }
        if name.eq_ignore_ascii_case("content-disposition") {
            disposition = Some(params);
        } else if name.eq_ignore_ascii_case("content-type") {
            content_type = main_value;
        }
    }
    let Some(dispo_params) = disposition else {
        return PartItem::Raw;
    };
    let mut name: Option<Vec<u8>> = None;
    let mut filename: Option<Vec<u8>> = None;
    for (key, value) in dispo_params {
        if key.eq_ignore_ascii_case(b"name") {
            name = Some(value);
        } else if key.eq_ignore_ascii_case(b"filename") {
            filename = Some(value);
        }
    }
    let Some(name) = name else {
        return PartItem::Nameless;
    };
    let name = strip_ascii_whitespace(&name);
    match filename {
        Some(filename) if !filename.is_empty() => PartItem::File {
            name,
            filename,
            content_type,
            data: data.to_vec(),
        },
        // Empty filename arrives as a text field (`P23` probe).
        _ => PartItem::Field {
            name,
            data: data.to_vec(),
        },
    }
}

/// Whether the part declares `Content-Transfer-Encoding: base64` (the
/// header line is lowercased by `parse_header_parameters`, so the match
/// is case-insensitive; the last such line wins).
fn item_transfer_is_base64(content: &[u8]) -> bool {
    let window = content.len().min(1024);
    let Some(end) = find_subsequence(&content[..window], b"\r\n\r\n") else {
        return false;
    };
    let mut encoding = String::new();
    for line in content[..end].split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Ok(line) = std::str::from_utf8(line) else {
            continue;
        };
        let (name, main_value, _) = parse_header_line(line);
        if name.eq_ignore_ascii_case("content-transfer-encoding") {
            encoding = String::from_utf8_lossy(&main_value).into_owned();
        }
    }
    encoding == "base64"
}

/// One header line: `parse_header_parameters` lowercases the whole
/// `name: value` main segment, so the returned main value is lowercase
/// (matching Django's stored `meta_data`); param values keep their case.
/// Lines without a colon yield the empty name (skipped by the caller).
/// The triple is (field name, raw value, parameters).
type ParsedHeaderLine = (String, Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>);

fn parse_header_line(line: &str) -> ParsedHeaderLine {
    let (main, params) = parse_header_parameters(line);
    let Some((name, value)) = main.split_once(':') else {
        return (String::new(), Vec::new(), Vec::new());
    };
    // Param values here are raw bytes of the (already unquoted) text.
    let params = params
        .into_iter()
        .map(|(key, value)| (key.into_bytes(), value.into_bytes()))
        .collect();
    (
        name.trim().to_owned(),
        value.trim().as_bytes().to_vec(),
        params,
    )
}

fn strip_ascii_whitespace(bytes: &[u8]) -> Vec<u8> {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    bytes[start..end].to_vec()
}

/// `binascii.a2b_base64` (`validate=False`): non-alphabet bytes are
/// discarded, then the padding structure must hold. Returns the decoded
/// bytes or fails (fields keep raw on failure; files 400).
fn binascii_b64decode(data: &[u8]) -> Result<Vec<u8>, ()> {
    // Strip ASCII whitespace first (binascii skips it with the rest).
    let clean: Vec<u8> = data
        .iter()
        .copied()
        .filter(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'='))
        .collect();
    // Pad-placement rules (reverse-engineered against CPython 3.12
    // `a2b_base64`, 70+ probes): strip the trailing pad run, then scan the
    // core counting data chars. A mid-core pad truncates the data iff the
    // data count so far is 3 mod 4, or the count is nonzero mod 4 and the
    // next char is also a pad; every other mid-core pad is silently
    // skipped (so `ab=cd` decodes as `abcd`, but `ab==cd` truncates to
    // `ab`, and `zab=c=` truncates to `zab`). Everything from a
    // truncation point on is ignored. A trailing partial quantum needs
    // its pads present (`ab=` fails, `ab==` passes, `ab=c=` passes).
    let stripped = clean.iter().rev().take_while(|b| **b == b'=').count();
    let core = &clean[..clean.len() - stripped];
    let mut data: Vec<u8> = Vec::with_capacity(core.len());
    let mut trunc_at: Option<usize> = None;
    for (index, byte) in core.iter().enumerate() {
        if *byte != b'=' {
            data.push(*byte);
            continue;
        }
        let seen = data.len() % 4;
        if seen == 3 || (seen != 0 && core.get(index + 1) == Some(&b'=')) {
            trunc_at = Some(index);
            break;
        }
    }
    if data.len() % 4 == 1 {
        return Err(());
    }
    let need = (4 - data.len() % 4) % 4;
    let mut pads_avail = stripped;
    if let Some(at) = trunc_at {
        pads_avail += core[at..].iter().filter(|b| **b == b'=').count();
    }
    if pads_avail < need {
        return Err(());
    }
    let data_end = data.len();
    let clean = &data;
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    let sextet = |b: u8| -> u32 {
        match b {
            b'A'..=b'Z' => (b - b'A') as u32,
            b'a'..=b'z' => (b - b'a' + 26) as u32,
            b'0'..=b'9' => (b - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    };
    let full_quanta = data_end / 4;
    for quantum in 0..full_quanta {
        let chunk = &clean[quantum * 4..quantum * 4 + 4];
        let group = (sextet(chunk[0]) << 18)
            | (sextet(chunk[1]) << 12)
            | (sextet(chunk[2]) << 6)
            | sextet(chunk[3]);
        out.push((group >> 16) as u8);
        out.push((group >> 8) as u8);
        out.push(group as u8);
    }
    let tail = &clean[full_quanta * 4..data_end];
    if !tail.is_empty() {
        let mut group = 0u32;
        for (index, byte) in tail.iter().enumerate() {
            group |= sextet(*byte) << (18 - index * 6);
        }
        for index in 0..tail.len() - 1 {
            out.push((group >> (16 - index * 8)) as u8);
        }
    }
    Ok(out)
}

/// `django/utils/text.py:sanitize_file_name` (reachable subset): strip
/// directories, unescape a practical entity set + all numeric refs,
/// drop non-printables; empty results mean "no file here" (the part is
/// skipped). Returns `None` for empty/`.`/`..` names.
fn sanitize_file_name(name: &str) -> Option<String> {
    let mut name = name.rsplit(['/', '\\']).next().unwrap_or("").to_owned();
    if name.is_empty() {
        return None;
    }
    name = html_unescape_practical(&name);
    name = name
        .chars()
        .filter(|c| is_printable_ascii_plus(*c))
        .collect();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(name)
}

fn html_unescape_practical(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'&' {
            if let Some(semi) = text[index..].find(';') {
                let entity = &text[index + 1..index + semi];
                if let Some(decoded) = decode_entity(entity) {
                    out.push_str(&decoded);
                    index += semi + 1;
                    continue;
                }
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

/// Named entities (the XML predeclared five + the latin-1 block Django
/// tests exercise most) plus full decimal/hex numeric refs. The remaining
/// html5 table is a filed follow-up; unlisted names stay literal.
fn decode_entity(entity: &str) -> Option<String> {
    if let Some(named) = match entity {
        "amp" => Some("&"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "quot" => Some("\""),
        "apos" => Some("'"),
        "nbsp" => Some("\u{00A0}"),
        "copy" => Some("©"),
        "reg" => Some("®"),
        "hellip" => Some("…"),
        "mdash" => Some("—"),
        "ndash" => Some("–"),
        "lsquo" => Some("‘"),
        "rsquo" => Some("’"),
        "ldquo" => Some("“"),
        "rdquo" => Some("”"),
        _ => None,
    } {
        return Some(named.to_owned());
    }
    let digits = entity.strip_prefix('#')?;
    let value = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
        u32::from_str_radix(hex, 16).ok()?
    } else if digits.bytes().all(|b| b.is_ascii_digit()) && !digits.is_empty() {
        digits.parse::<u32>().ok()?
    } else {
        return None;
    };
    // `html.unescape` maps invalid ref values per the WHATWG table
    // (surrogates/0/out-of-range become U+FFFD).
    if value == 0 || (0xD800..0xE000).contains(&value) || value > 0x10FFFF {
        return Some("\u{FFFD}".to_owned());
    }
    char::from_u32(value).map(|c| c.to_string())
}

/// Python `str.isprintable` over the reachable range: ASCII printables +
/// space pass; C0/C1 controls fail; other Unicode passes unless it is a
/// space separator, line/paragraph separator, or non-character. (Full
/// `Other`-category tables are a filed follow-up with the entity table.)
fn is_printable_ascii_plus(c: char) -> bool {
    if c == ' ' || c.is_ascii_graphic() {
        return true;
    }
    if c.is_ascii() {
        return false;
    }
    if c.is_control() {
        return false;
    }
    // Space/line/paragraph separators (Zs/Zl/Zp) except ASCII space.
    if matches!(
        c,
        '\u{00A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    ) {
        return false;
    }
    // Non-characters U+FDD0..U+FDEF + U+xxFFFE/F.
    if matches!(c, '\u{FDD0}'..='\u{FDEF}') || (c as u32 & 0xFFFF) >= 0xFFFE {
        return false;
    }
    true
}

#[cfg(test)]
mod codec_tests {
    use super::*;

    fn negotiate(ct: &str, body: &[u8], spec: &BodySpec) -> Result<NegotiatedBody, BodyError> {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", ct.parse().unwrap());
        headers.insert("content-length", body.len().to_string().parse().unwrap());
        negotiate_body(&headers, body, spec)
    }

    #[test]
    fn charset_aliases() {
        for name in ["utf-8", "UTF-8", "utf_8", "UTF8", "u8", "utf", "cp65001"] {
            assert_eq!(charset_to_supported(name), SupportedCharset::Utf8, "{name}");
        }
        for name in ["ascii", "us-ascii", "646", "ANSI_X3.4-1968"] {
            assert_eq!(
                charset_to_supported(name),
                SupportedCharset::Ascii,
                "{name}"
            );
        }
        for name in [
            "latin-1",
            "LATIN-1",
            "latin1",
            "latin",
            "l1",
            "iso-8859-1",
            "iso8859-1",
            "8859",
            "cp819",
        ] {
            assert_eq!(
                charset_to_supported(name),
                SupportedCharset::Latin1,
                "{name}"
            );
        }
        for name in ["utf-16", "UTF-16", "utf16", "u16"] {
            assert_eq!(
                charset_to_supported(name),
                SupportedCharset::Utf16,
                "{name}"
            );
        }
        for name in ["utf-16-le", "utf_16le", "unicodelittleunmarked"] {
            assert_eq!(
                charset_to_supported(name),
                SupportedCharset::Utf16Le,
                "{name}"
            );
        }
        for name in ["utf-16-be", "utf_16be"] {
            assert_eq!(
                charset_to_supported(name),
                SupportedCharset::Utf16Be,
                "{name}"
            );
        }
        for name in ["utf-32", "utf32"] {
            assert_eq!(
                charset_to_supported(name),
                SupportedCharset::Utf32,
                "{name}"
            );
        }
        for name in ["utf-32-le", "utf_32le"] {
            assert_eq!(
                charset_to_supported(name),
                SupportedCharset::Utf32Le,
                "{name}"
            );
        }
        {
            let name = "utf-32-be";
            assert_eq!(
                charset_to_supported(name),
                SupportedCharset::Utf32Be,
                "{name}"
            );
        }
        // Bogus, empty, exotic, and punctuation-only degrade to utf-8.
        for name in [
            "bogus",
            "",
            "cp1252",
            "windows-1252",
            "iso-8859-2",
            "---",
            "utf 8x",
        ] {
            assert_eq!(charset_to_supported(name), SupportedCharset::Utf8, "{name}");
        }
        // Interior space collapses to `_`, like CPython (`utf 8` is utf-8).
        assert_eq!(charset_to_supported("utf 8"), SupportedCharset::Utf8);
    }

    #[test]
    fn json_tail_drop_and_codec_errors() {
        // Trailing incomplete sequences are dropped (W2/T5/U4 probes).
        assert_eq!(
            decode_stream_body(b"{\"a\":1}\xe9", SupportedCharset::Utf8).unwrap(),
            "{\"a\":1}"
        );
        assert_eq!(
            decode_stream_body(b"\xff\xfe{\x00\"", SupportedCharset::Utf16).unwrap(),
            "{"
        );
        assert_eq!(
            decode_stream_body(b"\xff\xfe\x00\x00AB", SupportedCharset::Utf32).unwrap(),
            ""
        );
        assert_eq!(
            decode_stream_body(b"\xff\xfe", SupportedCharset::Utf16).unwrap(),
            ""
        );
        // Complete-invalid units error even at the very end (W1/W6/U3).
        assert_eq!(
            decode_stream_body(b"{\"a\":1}\xff", SupportedCharset::Utf8).unwrap_err(),
            "'utf-8' codec can't decode byte 0xff in position 7: invalid start byte"
        );
        assert_eq!(
            decode_stream_body(b"\xff\xfeA\x00\x00\xdc", SupportedCharset::Utf16).unwrap_err(),
            "'utf-16-le' codec can't decode bytes in position 4-5: illegal encoding"
        );
        assert_eq!(
            decode_stream_body(b"\x00\xd8\x00\xd8", SupportedCharset::Utf16Le).unwrap_err(),
            "'utf-16-le' codec can't decode bytes in position 0-1: illegal UTF-16 surrogate"
        );
        assert_eq!(
            decode_stream_body(b"\xe9", SupportedCharset::Ascii).unwrap_err(),
            "'ascii' codec can't decode byte 0xe9 in position 0: ordinal not in range(128)"
        );
        assert_eq!(
            decode_stream_body(b"abc", SupportedCharset::Utf16).unwrap_err(),
            "UTF-16 stream does not start with BOM"
        );
        assert_eq!(
            decode_stream_body(b"abcd", SupportedCharset::Utf32).unwrap_err(),
            "UTF-32 stream does not start with BOM"
        );
        assert_eq!(
            decode_stream_body(b"\xff\xfe\x00\x00\x00\xd8\x00\x00", SupportedCharset::Utf32).unwrap_err(),
            "'utf-32-le' codec can't decode bytes in position 4-7: code point in surrogate code point range(0xd800, 0xe000)"
        );
        assert_eq!(
            decode_stream_body(b"\xff\xfe\x00\x00\x00\x00\x11\x00", SupportedCharset::Utf32).unwrap_err(),
            "'utf-32-le' codec can't decode bytes in position 4-7: code point not in range(0x110000)"
        );
        // latin-1 is total, including the C1 controls (P10g).
        assert_eq!(
            decode_stream_body(b"\x80\xff", SupportedCharset::Latin1).unwrap(),
            "\u{80}\u{FF}"
        );
        // Mid-stream codec errors keep CPython's texts (V3/P10l).
        assert_eq!(
            decode_stream_body(b"{\"a\":\"\xe9\"}", SupportedCharset::Utf8).unwrap_err(),
            "'utf-8' codec can't decode byte 0xe9 in position 6: invalid continuation byte"
        );
        // Range-invalid truncations still error (cycle battery note).
        assert_eq!(
            decode_stream_body(b"\xed\xa0", SupportedCharset::Utf8).unwrap_err(),
            "'utf-8' codec can't decode byte 0xed in position 0: invalid continuation byte"
        );
        // Valid two-byte prefix at EOF drops.
        assert_eq!(
            decode_stream_body(b"\xe4\xb8", SupportedCharset::Utf8).unwrap(),
            ""
        );
    }

    #[test]
    fn negotiate_415_shapes() {
        let err = negotiate("text/plain", b"{\"name\":\"x\"}", &CYCLE_BODY_SPEC).unwrap_err();
        assert_eq!(
            err,
            BodyError::UnsupportedMediaType(
                "Unsupported media type \"text/plain\" in request.".to_owned()
            )
        );
        // Params and case ride along verbatim in the echo (A09 probe).
        let err = negotiate("text/plain; charset=utf-8", b"x", &CYCLE_BODY_SPEC).unwrap_err();
        assert!(
            matches!(err, BodyError::UnsupportedMediaType(detail) if detail == "Unsupported media type \"text/plain; charset=utf-8\" in request.")
        );
        // Missing content type renders the empty Django value (uvicorn).
        let mut headers = HeaderMap::new();
        headers.insert("content-length", "3".parse().unwrap());
        let err = negotiate_body(&headers, b"{\"a\":1}", &CYCLE_BODY_SPEC).unwrap_err();
        assert_eq!(
            err,
            BodyError::UnsupportedMediaType("Unsupported media type \"\" in request.".to_owned())
        );
    }

    #[test]
    fn negotiate_empty_matrix() {
        for ct in [
            "application/json",
            "text/plain",
            "application/x-www-form-urlencoded",
            "multipart/form-data; boundary=x",
            "",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert("content-type", ct.parse().unwrap());
            headers.insert("content-length", "0".parse().unwrap());
            assert!(
                matches!(
                    negotiate_body(&headers, b"", &CYCLE_BODY_SPEC),
                    Ok(NegotiatedBody::Empty)
                ),
                "{ct}"
            );
        }
        // Missing Content-Length is empty too, even with bytes present.
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "text/plain".parse().unwrap());
        assert!(matches!(
            negotiate_body(&headers, b"xxx", &CYCLE_BODY_SPEC),
            Ok(NegotiatedBody::Empty)
        ));
    }

    #[test]
    fn form_querydict_shapes() {
        let form = |body: &[u8]| match negotiate(
            "application/x-www-form-urlencoded",
            body,
            &CYCLE_BODY_SPEC,
        )
        .unwrap()
        {
            NegotiatedBody::Form { map, files } => {
                assert!(files.is_empty());
                map
            }
            other => panic!("expected form, got {other:?}"),
        };
        // Last value wins; `+`/escapes decode (B01/B05/B09 probes).
        let map = form(b"name=first&name=second&description=a+b%26c");
        assert_eq!(map["name"], Value::String("second".to_owned()));
        assert_eq!(map["description"], Value::String("a b&c".to_owned()));
        // No `=` keeps a blank value; empty segments skipped.
        let map = form(b"flag&&name=x");
        assert_eq!(map["flag"], Value::String(String::new()));
        assert_eq!(map["name"], Value::String("x".to_owned()));
        // `;` is not a separator; only the first `=` splits (P10i/R11).
        let map = form(b"name=a;b&k=a=b=c");
        assert_eq!(map["name"], Value::String("a;b".to_owned()));
        assert_eq!(map["k"], Value::String("a=b=c".to_owned()));
        // Bad escapes stay literal; bad bytes become U+FFFD (B10).
        let map = form(b"a=%zz%2&b=%FF%FE");
        assert_eq!(map["a"], Value::String("%zz%2".to_owned()));
        assert_eq!(map["b"], Value::String("\u{FFFD}\u{FFFD}".to_owned()));
        // Raw non-UTF-8 bytes fall the whole body back to latin-1 (P10c).
        let map = form(b"name=\xe9");
        assert_eq!(map["name"], Value::String("é".to_owned()));
        // Blank skips per spec (S6/S7/S8 probes).
        let map = form(b"name=x&timezone=&start_date=&end_date=");
        assert_eq!(map["name"], Value::String("x".to_owned()));
        assert!(!map.contains_key("timezone"));
        assert!(!map.contains_key("start_date"));
        assert!(!map.contains_key("end_date"));
        // Non-skipped blanks flow through as empty strings.
        let map = form(b"name=&description=&owned_by=");
        assert_eq!(map["name"], Value::String(String::new()));
        assert_eq!(map["description"], Value::String(String::new()));
        assert_eq!(map["owned_by"], Value::String(String::new()));
    }

    #[test]
    fn form_field_limit() {
        let pairs: Vec<String> = (0..1000).map(|i| format!("k{i}=v")).collect();
        let ok_body = pairs.join("&");
        assert_eq!(ok_body.split('&').count(), 1000);
        assert!(negotiate(
            "application/x-www-form-urlencoded",
            ok_body.as_bytes(),
            &CYCLE_BODY_SPEC
        )
        .is_ok());
        // 1001 segments (trailing `&` counts!) is the generic 500 (P9b).
        let over_body = format!("{ok_body}&");
        assert_eq!(
            negotiate(
                "application/x-www-form-urlencoded",
                over_body.as_bytes(),
                &CYCLE_BODY_SPEC
            )
            .unwrap_err(),
            BodyError::ServerError
        );
    }

    #[test]
    fn module_members_arrays() {
        let form = |body: &[u8]| match negotiate(
            "application/x-www-form-urlencoded",
            body,
            &MODULE_BODY_SPEC,
        )
        .unwrap()
        {
            NegotiatedBody::Form { map, files } => (map, files),
            other => panic!("expected form, got {other:?}"),
        };
        // Single member still arrives as an array (P24a2).
        let (map, _) = form(b"name=m&members=11111111-1111-1111-1111-111111111111");
        assert_eq!(
            map["members"],
            Value::Array(vec![Value::String(
                "11111111-1111-1111-1111-111111111111".to_owned()
            )])
        );
        // Repeated keys arrive in order (P24b).
        let (map, _) = form(b"members=a&name=m&members=b");
        assert_eq!(
            map["members"],
            Value::Array(vec![
                Value::String("a".to_owned()),
                Value::String("b".to_owned())
            ])
        );
        // Empty member is an empty-string item, like JSON (P24c/R1).
        let (map, _) = form(b"name=m&members=");
        assert_eq!(
            map["members"],
            Value::Array(vec![Value::String(String::new())])
        );
    }

    #[test]
    fn multipart_basics_and_errors() {
        let ct = "multipart/form-data; boundary=----b";
        let mp = |body: &[u8]| negotiate(ct, body, &CYCLE_BODY_SPEC);
        // Text field round-trips (C01).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nmpcycle\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form { map, files } => {
                assert_eq!(map["name"], Value::String("mpcycle".to_owned()));
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Unknown file is carried with bytes (C02).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nv\r\n------b\r\nContent-Disposition: form-data; name=\"att\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhi\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form { map, files } => {
                assert_eq!(map["name"], Value::String("v".to_owned()));
                assert_eq!(files["att"][0].filename, "a.txt");
                assert_eq!(files["att"][0].bytes, b"hi");
            }
            other => panic!("{other:?}"),
        }
        // No boundary (C04).
        assert_eq!(
            negotiate("multipart/form-data", b"garbage", &CYCLE_BODY_SPEC).unwrap_err(),
            BodyError::ParseDetail(
                "Multipart form parse error - Invalid boundary in multipart: None".to_owned()
            )
        );
        // Empty boundary echoes empty (P15a).
        assert_eq!(
            negotiate("multipart/form-data; boundary=", b"x", &CYCLE_BODY_SPEC).unwrap_err(),
            BodyError::ParseDetail(
                "Multipart form parse error - Invalid boundary in multipart: ".to_owned()
            )
        );
        // Case-sensitive multipart prefix echoes the full header (P19).
        assert_eq!(
            negotiate("Multipart/Form-Data; boundary=----b", b"x", &CYCLE_BODY_SPEC).unwrap_err(),
            BodyError::ParseDetail(
                "Multipart form parse error - Invalid Content-Type: Multipart/Form-Data; boundary=----b"
                    .to_owned()
            )
        );
        // Garbage with a valid boundary is empty data, not an error (C05).
        match mp(b"this is not multipart").unwrap() {
            NegotiatedBody::Form { map, files } => {
                assert!(map.is_empty());
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Empty filename arrives as a text field (P23).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"name\"; filename=\"\"\r\n\r\nefv\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form { map, files } => {
                assert_eq!(map["name"], Value::String("efv".to_owned()));
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Bad base64 file is the fixed 400 (P32); bad base64 field keeps
        // raw bytes (P33).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"att\"; filename=\"a.bin\"\r\nContent-Transfer-Encoding: base64\r\n\r\nabcde\r\n------b--\r\n";
        assert_eq!(
            mp(body).unwrap_err(),
            BodyError::ParseDetail(
                "Multipart form parse error - Could not decode base64 data.".to_owned()
            )
        );
        let body = b"------b\r\nContent-Disposition: form-data; name=\"name\"\r\nContent-Transfer-Encoding: base64\r\n\r\nabcde\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form { map, files } => {
                assert_eq!(map["name"], Value::String("abcde".to_owned()));
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Non-UTF-8 header line is skipped, killing the part (P22).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"na\xffme\"\r\n\r\nv\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form { map, files } => {
                assert!(map.is_empty());
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Duplicate disposition lines: last wins (R9).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"nope\"\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\ndup1\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form { map, files } => {
                assert_eq!(map["name"], Value::String("dup1".to_owned()));
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multipart_filename_sanitize() {
        let ct = "multipart/form-data; boundary=----b";
        let filename_of = |dispo: &[u8]| match negotiate(ct, dispo, &CYCLE_BODY_SPEC).unwrap() {
            NegotiatedBody::Form { files, .. } => files
                .get("att")
                .map(|v| v[0].filename.clone())
                .unwrap_or_else(|| "<skipped>".to_owned()),
            other => panic!("{other:?}"),
        };
        let part = |d: &[u8]| {
            [
                b"------b\r\nContent-Disposition: form-data; name=\"att\"; filename=\"".as_slice(),
                d,
                b"\"\r\n\r\nx\r\n------b--\r\n".as_slice(),
            ]
            .concat()
        };
        // Path prefixes are stripped.
        assert_eq!(filename_of(&part(b"a/b/c.txt")), "c.txt");
        // Entities unescape (practical subset + numeric refs).
        assert_eq!(filename_of(&part(b"a&amp;b.txt")), "a&b.txt");
        assert_eq!(filename_of(&part(b"a&#x41;b.txt")), "aAb.txt");
        // Controls are dropped; dot-names vanish the part.
        assert_eq!(filename_of(&part(b"a\x01b.txt")), "ab.txt");
        assert_eq!(filename_of(&part(b"..")), "<skipped>");
    }

    #[test]
    fn multipart_limits() {
        let ct = "multipart/form-data; boundary=----b";
        let many = |n: usize, file: bool| {
            let mut body = Vec::new();
            for i in 0..n {
                body.extend_from_slice(b"------b\r\nContent-Disposition: form-data; name=\"k");
                body.extend_from_slice(i.to_string().as_bytes());
                body.extend_from_slice(b"\"");
                if file {
                    body.extend_from_slice(b"; filename=\"f.txt\"");
                }
                body.extend_from_slice(b"\r\n\r\nv\r\n");
            }
            body.extend_from_slice(b"------b--\r\n");
            body
        };
        // 100 files pass; the 101st is the generic 500 (P9c/P9d).
        assert!(negotiate(ct, &many(100, true), &CYCLE_BODY_SPEC).is_ok());
        assert_eq!(
            negotiate(ct, &many(101, true), &CYCLE_BODY_SPEC).unwrap_err(),
            BodyError::ServerError
        );
        // Field items (fields + RAW preamble/epilogue) share the
        // 1000+2 budget: 1000 fields pass, 1001 fail (V1a/V1c).
        assert!(negotiate(ct, &many(1000, false), &CYCLE_BODY_SPEC).is_ok());
        assert_eq!(
            negotiate(ct, &many(1001, false), &CYCLE_BODY_SPEC).unwrap_err(),
            BodyError::ServerError
        );
    }

    #[test]
    fn base64_binascii_shapes() {
        // Quantum-streaming rules, vectors verified against CPython 3.12
        // `binascii.a2b_base64` (`b"abcd"` is `[105, 183, 29]`, not `b"abc"`).
        assert_eq!(binascii_b64decode(b"YWJj").unwrap(), b"abc");
        assert_eq!(binascii_b64decode(b"abcd").unwrap(), b"i\xb7\x1d");
        assert_eq!(binascii_b64decode(b"abc=").unwrap(), b"i\xb7");
        assert_eq!(binascii_b64decode(b"ab==").unwrap(), b"i");
        assert_eq!(binascii_b64decode(b"abcdef==").unwrap(), b"i\xb7\x1dy");
        assert_eq!(binascii_b64decode(b"ab==cd").unwrap(), b"i");
        assert_eq!(binascii_b64decode(b"abcd=").unwrap(), b"i\xb7\x1d");
        assert_eq!(binascii_b64decode(b"abcd====").unwrap(), b"i\xb7\x1d");
        assert_eq!(binascii_b64decode(b"abc=def=").unwrap(), b"i\xb7");
        assert_eq!(binascii_b64decode(b"abcdefg=").unwrap(), b"i\xb7\x1dy\xf8");
        assert_eq!(binascii_b64decode(b"!!!").unwrap(), b"");
        assert_eq!(binascii_b64decode(b"").unwrap(), b"");
        assert_eq!(binascii_b64decode(b"====").unwrap(), b"");
        assert!(binascii_b64decode(b"abcdef").is_err());
        assert!(binascii_b64decode(b"abc").is_err());
        assert!(binascii_b64decode(b"abcde").is_err());
        assert!(binascii_b64decode(b"ab=").is_err());
        assert!(binascii_b64decode(b"abcdef=").is_err());
        assert!(binascii_b64decode(b"ab=c").is_err());
        assert!(binascii_b64decode(b"abcd=efg").is_err());
        assert!(binascii_b64decode(b"a===").is_err());
        assert!(binascii_b64decode(b"a=b=c").is_err());
        assert!(binascii_b64decode(b"ab=cdef").is_err());
        assert!(binascii_b64decode(b"abcd====e").is_err());
        assert_eq!(binascii_b64decode(b"a=b=c=").unwrap(), b"i\xb7");
        assert_eq!(binascii_b64decode(b"=").unwrap(), b"");
        assert_eq!(binascii_b64decode(b"==").unwrap(), b"");
        assert_eq!(binascii_b64decode(b"ab=c=").unwrap(), b"i\xb7");
        assert_eq!(binascii_b64decode(b"ab=cd").unwrap(), b"i\xb7\x1d");
        assert_eq!(binascii_b64decode(b"a=bcd").unwrap(), b"i\xb7\x1d");
        assert_eq!(binascii_b64decode(b"a=bc=").unwrap(), b"i\xb7");
        assert_eq!(binascii_b64decode(b"ab=c=d").unwrap(), b"i\xb7");
        assert_eq!(binascii_b64decode(b"zab=c=").unwrap(), b"\xcd\xa6");
        assert_eq!(binascii_b64decode(b"ab====cd").unwrap(), b"i");
        assert_eq!(binascii_b64decode(b"ab=c==d").unwrap(), b"i\xb7");
        assert_eq!(binascii_b64decode(b"a=b=c==").unwrap(), b"i\xb7");
        // Non-alphabet bytes are discarded before the structure check.
        assert_eq!(binascii_b64decode(b"ab\ncd").unwrap(), b"i\xb7\x1d");
        assert_eq!(binascii_b64decode(b"a b\tc\nd").unwrap(), b"i\xb7\x1d");
    }
}

#[cfg(test)]
mod header_tests {

    use super::*;

    #[test]
    fn content_length_rule() {
        let headers = |cl: Option<&str>| {
            let mut headers = HeaderMap::new();
            if let Some(cl) = cl {
                headers.insert("content-length", cl.parse().unwrap());
            }
            headers
        };
        assert!(content_length_is_empty(&headers(None)));
        assert!(content_length_is_empty(&headers(Some("0"))));
        assert!(content_length_is_empty(&headers(Some("000"))));
        assert!(content_length_is_empty(&headers(Some("-0"))));
        assert!(content_length_is_empty(&headers(Some("abc"))));
        assert!(content_length_is_empty(&headers(Some("5.0"))));
        assert!(content_length_is_empty(&headers(Some(""))));
        assert!(content_length_is_empty(&headers(Some("  "))));
        assert!(!content_length_is_empty(&headers(Some("5"))));
        assert!(!content_length_is_empty(&headers(Some(" 5 "))));
        assert!(!content_length_is_empty(&headers(Some("+5"))));
        assert!(!content_length_is_empty(&headers(Some("-1"))));
        assert!(!content_length_is_empty(&headers(Some("007"))));
    }

    #[test]
    fn parser_selection() {
        assert_eq!(select_parser("application/json"), Parser::Json);
        assert_eq!(select_parser("Application/JSON"), Parser::Json);
        assert_eq!(
            select_parser("application/json; charset=utf-8"),
            Parser::Json
        );
        assert_eq!(
            select_parser("application/x-www-form-urlencoded"),
            Parser::Form
        );
        assert_eq!(
            select_parser("multipart/form-data; boundary=x"),
            Parser::Multipart
        );
        assert_eq!(select_parser("*/*"), Parser::Json);
        assert_eq!(select_parser("application/*"), Parser::Json);
        assert_eq!(select_parser("text/plain"), Parser::None);
        assert_eq!(select_parser("text/*"), Parser::None);
        assert_eq!(select_parser(""), Parser::None);
        assert_eq!(select_parser("application/jsonn"), Parser::None);
    }

    #[test]
    fn header_params_shapes() {
        let (main, params) = parse_header_parameters("Multipart/Form-Data; boundary=----x");
        assert_eq!(main, "multipart/form-data");
        assert_eq!(params, vec![("boundary".to_owned(), "----x".to_owned())]);
        let (main, params) = parse_header_parameters("multipart/form-data; boundary=\"a;b\"");
        assert_eq!(main, "multipart/form-data");
        assert_eq!(params, vec![("boundary".to_owned(), "a;b".to_owned())]);
        let (main, params) =
            parse_header_parameters("application/x-www-form-urlencoded; Charset=\"latin-1\"");
        assert_eq!(main, "application/x-www-form-urlencoded");
        assert_eq!(params, vec![("charset".to_owned(), "latin-1".to_owned())]);
        let (main, params) = parse_header_parameters("text/plain");
        assert_eq!(main, "text/plain");
        assert!(params.is_empty());
    }
}
