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
//! follow-up; filename sanitizing is the exact `sanitize_file_name` port
//! (full html5 table + CPython `isprintable`, PIDASHCONV-694). Paths that
//! never touch `request.data` (GET/DELETE/archive) never call this module,
//! so they can never 415.
//
//! Exotic codecs (PIDASHCONV-693): every other `codecs.lookup`-reachable
//! module dispatches through [`SupportedCharset::Exotic`] to
//! `body_decoders` (same URL paths, same JSON, same SQL semantics).

use axum::http::HeaderMap;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

use super::sanitize_data::{
    HTML5_ENTITIES, INVALID_CHARREFS, INVALID_CODEPOINT_RANGES, NONPRINTABLE_RANGES,
};

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

/// Cycle write paths: no list fields; only the timezone choice skips
/// blank form input (`ChoiceField`: no `allow_blank`, not required).
/// The datetimes keep a blank `''` in the map (`Field.get_value` maps
/// present-`''` + `allow_null` to `None`): the create gate sees `''` as
/// present, and coercion turns HTML `''` into the null arm.
pub const CYCLE_BODY_SPEC: BodySpec = BodySpec {
    list_fields: &[],
    skip_blank_fields: &["timezone"],
};

/// Module write paths: `members` is a `ListField`; only the status choice
/// skips blank form input. The dates keep `''` (same `get_value` rule).
pub const MODULE_BODY_SPEC: BodySpec = BodySpec {
    list_fields: &["members"],
    skip_blank_fields: &["status"],
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
    /// Whether Django held this upload in memory (`InMemoryUploadedFile`)
    /// rather than spilling to temp storage (`TemporaryUploadedFile`):
    /// the whole request body fit in `FILE_UPLOAD_MAX_MEMORY_SIZE`
    /// (2621440). Only the indexed-dict echo renders the class name.
    pub in_memory: bool,
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
/// One form/multipart text value: decoded text (U+FFFD placeholders where
/// the codec emitted lone surrogates) plus the placeholder spans (byte
/// offset, surrogate value) for the coerce layer's surrogate check.
#[derive(Debug, Clone, Default)]
pub struct FormValue {
    pub text: String,
    pub surr: Vec<(usize, u16)>,
}

#[derive(Debug, Clone, Default)]
pub struct FormBody {
    /// Text values per key, in arrival order (QueryDict lists). A
    /// `BTreeMap`: `serde_json::Map`'s methods exist only on the concrete
    /// `Map<String, Value>`.
    pub texts: BTreeMap<String, Vec<FormValue>>,
    /// File values per key, in arrival order.
    pub files: FilesMap,
    /// Text keys in first-seen order (QueryDict key order).
    pub text_order: Vec<String>,
    /// File keys in first-seen order. The merged `_full_data` key order
    /// is the text keys followed by the files-only keys (`copy().update`
    /// keeps data positions and appends new file keys).
    pub file_order: Vec<String>,
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
    /// sequence dropped) plus lone-surrogate spans: byte offset of a
    /// U+FFFD placeholder in `text` plus the surrogate value. The caller
    /// parses with its existing serde + CPython-error mapping, swapping
    /// placeholders for dirty units.
    JsonText {
        text: String,
        surr: Vec<(usize, u16)>,
    },
    /// Parsed form/multipart body with HTML-input semantics applied per
    /// `spec` (list fields as arrays, blank skips). `map` holds text-only
    /// values; `files` holds uploads per key (DRF merges files into
    /// `request.data`, so `in` checks must consult both maps); `surr`
    /// holds the lone-surrogate spans per key, aligned with the selected
    /// values (scalars: one entry; list fields: one per array item).
    Form {
        map: Map<String, Value>,
        files: FilesMap,
        surr: BTreeMap<String, Vec<Vec<(usize, u16)>>>,
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
    match select_parser(&content_type)? {
        Parser::Json => {
            let (text, surr) = decode_json_body(body, &content_type)?;
            Ok(NegotiatedBody::JsonText { text, surr })
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
fn build_form_body(mut form: FormBody, spec: &BodySpec) -> NegotiatedBody {
    let mut map = Map::new();
    let mut surr: BTreeMap<String, Vec<Vec<(usize, u16)>>> = BTreeMap::new();
    let mut keys: Vec<&String> = form.texts.keys().collect();
    keys.sort();
    for key in keys {
        let values = &form.texts[key.as_str()];
        if spec.list_fields.contains(&key.as_str()) {
            let mut items: Vec<Value> = values
                .iter()
                .map(|v| Value::String(v.text.clone()))
                .collect();
            // Files extend the value list (same key) but are not JSON
            // values; the array holds texts only and the files map keeps
            // the uploads for the caller's per-field handling.
            let _ = &mut items;
            map.insert(key.clone(), Value::Array(items));
            surr.insert(key.clone(), values.iter().map(|v| v.surr.clone()).collect());
        } else if let Some(last) = values.last() {
            if last.text.is_empty() && spec.skip_blank_fields.contains(&key.as_str()) {
                continue;
            }
            map.insert(key.clone(), Value::String(last.text.clone()));
            surr.insert(key.clone(), vec![last.surr.clone()]);
        }
    }
    // List fields whose only values are files still arrive as arrays (an
    // empty array here; the caller reads the files map for the items).
    for key in spec.list_fields {
        if !map.contains_key(*key) && form.files.contains_key(*key) {
            map.insert((*key).to_owned(), Value::Array(Vec::new()));
        }
    }
    // Indexed list keys (DRF `parse_html_list`, `utils/html.py`): when the
    // exact key is absent from both texts and files, `members[N]` /
    // `members[N]suffix` keys assemble the array instead (exact-key
    // `getlist` wins when both are present). Plain file values arrive as
    // JSON nulls (placeholders — form parsing otherwise never emits null)
    // with the uploads moved under the field name in arrival order; the
    // caller resolves them positionally. Dict-form values arrive as arrays
    // of single-key objects (`{suffix: [value]}`, first-seen suffix order)
    // so the caller can render the `MultiValueDict` echo exactly.
    for field in spec.list_fields {
        if map.contains_key(*field) {
            continue;
        }
        if let Some(entries) = scan_indexed_list(&form, field) {
            let mut items = Vec::with_capacity(entries.len());
            let mut spans = Vec::with_capacity(entries.len());
            let mut uploads = Vec::new();
            for entry in entries {
                match entry {
                    IndexedEntry::Text(value) => {
                        spans.push(value.surr.clone());
                        items.push(Value::String(value.text));
                    }
                    IndexedEntry::File(key) => {
                        if let Some(part) = form.files.get_mut(&key).and_then(Vec::pop) {
                            uploads.push(part);
                        }
                        spans.push(Vec::new());
                        items.push(Value::Null);
                    }
                    IndexedEntry::Dict(pairs) => {
                        let mut rendered = Vec::with_capacity(pairs.len());
                        for (suffix, value) in pairs {
                            let item = match value {
                                IndexedValue::Text(value) => Value::String(value.text),
                                IndexedValue::File(key) => {
                                    if let Some(part) = form.files.get_mut(&key).and_then(Vec::pop)
                                    {
                                        uploads.push(part);
                                    }
                                    Value::Null
                                }
                            };
                            let mut pair = Map::new();
                            pair.insert(suffix, Value::Array(vec![item]));
                            rendered.push(Value::Object(pair));
                        }
                        // Dict items pack several values into one array
                        // slot; spans stay empty (only module list fields
                        // assemble indexed entries, and module drops the
                        // surr map — the alignment slot is still pushed).
                        spans.push(Vec::new());
                        items.push(Value::Array(rendered));
                    }
                }
            }
            map.insert((*field).to_owned(), Value::Array(items));
            surr.insert((*field).to_owned(), spans);
            if !uploads.is_empty() {
                form.files.insert((*field).to_owned(), uploads);
            }
        }
    }
    NegotiatedBody::Form {
        map,
        files: form.files,
        surr,
    }
}

/// One assembled indexed-list entry, in index order.
enum IndexedEntry {
    Text(FormValue),
    /// Key holding the file (the last upload wins, like `.items()`).
    File(String),
    /// Dict-form pairs in first-seen suffix order.
    Dict(Vec<(String, IndexedValue)>),
}

/// A scanned indexed value: the last text, or the key of the last file.
#[derive(Debug, Clone)]
enum IndexedValue {
    Text(FormValue),
    File(String),
}

/// Scan `_full_data` for `prefix[N]` / `prefix[N]suffix` keys (`re`:
/// `^prefix\[([0-9]+)\](.*)$` — literal prefix, ASCII digits, any suffix
/// without a newline). Merged key order is the text keys then the
/// files-only keys; per key the value is the last file when the key
/// carries any, else the last text. Plain entries overwrite (even dicts),
/// dict entries merge by suffix with setitem (last wins) semantics.
/// Returns the entries sorted by numeric index, or `None` when no key
/// matched (the field stays missing).
fn scan_indexed_list(form: &FormBody, field: &str) -> Option<Vec<IndexedEntry>> {
    // (key, value) in merged `_full_data` order: text keys first (data
    // positions win for keys carrying both), then files-only keys.
    let mut merged: Vec<(&str, IndexedValue)> = Vec::new();
    for key in &form.text_order {
        if form.files.contains_key(key) {
            merged.push((key, IndexedValue::File(key.clone())));
        } else if let Some(last) = form.texts.get(key).and_then(|v| v.last()) {
            merged.push((key, IndexedValue::Text(last.clone())));
        }
    }
    for key in &form.file_order {
        if !form.texts.contains_key(key) && form.files.contains_key(key) {
            merged.push((key, IndexedValue::File(key.clone())));
        }
    }
    let mut hits: Vec<(String, String, IndexedValue)> = Vec::new();
    for (key, value) in merged {
        if let Some((index, suffix)) = split_indexed_key(key, field) {
            // DRF keys `ret` by `int(index)`: `007` and `7` collide.
            hits.push((normalize_index_text(index), suffix.to_owned(), value));
        }
    }
    if hits.is_empty() {
        return None;
    }
    // Arrival-order overwrite rules, then numeric-index sort.
    let mut ret: Vec<(String, IndexedEntry)> = Vec::new();
    for (index, suffix, value) in hits {
        let slot = ret.iter_mut().find(|(at, _)| *at == index);
        if suffix.is_empty() {
            let entry = match value {
                IndexedValue::Text(text) => IndexedEntry::Text(text),
                IndexedValue::File(key) => IndexedEntry::File(key),
            };
            match slot {
                Some((_, at)) => *at = entry,
                None => ret.push((index, entry)),
            }
        } else {
            match slot {
                Some((_, IndexedEntry::Dict(pairs))) => {
                    if let Some(at) = pairs.iter_mut().find(|(at, _)| *at == suffix) {
                        at.1 = value;
                    } else {
                        pairs.push((suffix, value));
                    }
                }
                Some((_, at)) => {
                    *at = IndexedEntry::Dict(vec![(suffix, value)]);
                }
                None => ret.push((index, IndexedEntry::Dict(vec![(suffix, value)]))),
            }
        }
    }
    ret.sort_by(|a, b| cmp_index_text(&a.0, &b.0));
    Some(ret.into_iter().map(|(_, entry)| entry).collect())
}

/// Canonical index text for identity + sort (leading zeros dropped).
fn normalize_index_text(index: &str) -> String {
    let stripped = index.trim_start_matches('0');
    if stripped.is_empty() {
        "0".to_owned()
    } else {
        stripped.to_owned()
    }
}

/// Split `prefix[N]suffix` (`N` one or more ASCII digits, suffix without
/// `\n` — Python `.` never matches a newline).
fn split_indexed_key<'a>(key: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let rest = key.strip_prefix(prefix)?.strip_prefix('[')?;
    let close = rest.find(']')?;
    let (digits, suffix) = rest.split_at(close);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let suffix = &suffix[1..];
    if suffix.contains('\n') {
        return None;
    }
    Some((digits, suffix))
}

/// Numeric index order over digit strings of any length (Python `int`
/// sort: leading zeros ignored, then length, then lexicographic).
fn cmp_index_text(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (normalize_index_text(a), normalize_index_text(b));
    (a.len(), a).cmp(&(b.len(), b))
}

/// Raw header value as sent (`HeaderMap` strips only surrounding whitespace,
/// like the WSGI servers do). Missing headers are the empty string, which
/// is what Django sees under prod (uvicorn/ASGI omits the key); runserver's
/// `text/plain` default for a missing content type is a wsgiref artifact
/// the port deliberately does not reproduce (same JSON as prod Django).
/// Bytes decode as latin-1 (total over what hyper delivers): uvicorn and
/// wsgiref both surface obs-text bytes as latin-1 chars, so Django's 415
/// echoes them verbatim (live: `text/pl\xe9in` echoes U+00E9).
fn header_str(headers: &HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .map(|v| v.as_bytes().iter().map(|b| *b as char).collect())
        .unwrap_or_default()
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
fn select_parser(content_type: &str) -> Result<Parser, BodyError> {
    let (base, _) = parse_header_parameters(content_type)?;
    let (main, sub) = split_media_type(&base);
    for parser in [
        "application/json",
        "application/x-www-form-urlencoded",
        "multipart/form-data",
    ] {
        let (pmain, psub) = split_media_type(parser);
        if media_type_matches((pmain, psub), (main.clone(), sub.clone())) {
            return Ok(match parser {
                "application/json" => Parser::Json,
                "application/x-www-form-urlencoded" => Parser::Form,
                _ => Parser::Multipart,
            });
        }
    }
    Ok(Parser::None)
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
/// decoding. An RFC 2231 inline charset that rejects `replace` fails
/// with the pinned 500 (the exception escapes request setup into
/// `BaseAPIView.handle_exception`).
fn parse_header_parameters(line: &str) -> Result<(String, Vec<(String, String)>), BodyError> {
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
                // Header values never validate: surrogate spans drop.
                value = percent_decode_str(raw, &charset_to_supported(charset), &[])?.0;
            }
        }
        params.push((name, value));
    }
    Ok((main, params))
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

/// Charsets the port resolves (`codecs.lookup` over all 118 importable
/// codec modules; rejected names degrade to utf-8, exactly like a bogus
/// name does under Django). The nine 627 decoders stay inline; every
/// other module dispatches to `body_decoders` by module name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SupportedCharset {
    Utf8,
    Ascii,
    Latin1,
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
    Exotic(&'static str),
}

/// Resolve a `charset=` parameter the way `request._set_content_type_params`
/// plus `codecs.lookup` do: byte-oriented `normalize_encoding`, then the
/// full `encodings.aliases` table, then the dotless importable module
/// itself. Anything else is utf-8.
pub(crate) fn charset_to_supported(raw: &str) -> SupportedCharset {
    match super::body_decoders::resolve_module(raw) {
        Some("utf_8") => SupportedCharset::Utf8,
        Some("ascii") => SupportedCharset::Ascii,
        Some("latin_1") => SupportedCharset::Latin1,
        Some("utf_16") => SupportedCharset::Utf16,
        Some("utf_16_le") => SupportedCharset::Utf16Le,
        Some("utf_16_be") => SupportedCharset::Utf16Be,
        Some("utf_32") => SupportedCharset::Utf32,
        Some("utf_32_le") => SupportedCharset::Utf32Le,
        Some("utf_32_be") => SupportedCharset::Utf32Be,
        Some(module) => SupportedCharset::Exotic(module),
        None => SupportedCharset::Utf8,
    }
}

/// The charset parameter of a content type (verbatim value; matching is
/// case-insensitive on the parameter name via `parse_header_parameters`).
fn content_type_charset(content_type: &str) -> Result<SupportedCharset, BodyError> {
    let (_, params) = parse_header_parameters(content_type)?;
    // Duplicate params: last wins (`parse_header_parameters` returns a
    // dict in Django, so later pairs overwrite earlier ones).
    Ok(params
        .iter()
        .rfind(|(name, _)| name == "charset")
        .map(|(_, value)| charset_to_supported(value))
        .unwrap_or(SupportedCharset::Utf8))
}

/// Decode a JSON body: `codecs.getreader(charset)(stream)` semantics. The
/// stream decode drops a trailing *incomplete* sequence (utf-8 lead
/// prefix, utf-16 odd byte or pending high surrogate, utf-32 short tail)
/// and raises on everything else, with CPython's exact texts. Exotic
/// charsets (including the bytes transforms) dispatch to `body_decoders`.
/// Returns the text plus lone-surrogate spans (always empty for the nine
/// 627 charsets, which cannot emit surrogates).
fn decode_json_body(
    body: &[u8],
    content_type: &str,
) -> Result<(String, Vec<(usize, u16)>), BodyError> {
    let charset = content_type_charset(content_type)?;
    if let SupportedCharset::Exotic(_) = charset {
        return super::body_decoders::decode_json_exotic(body, &charset);
    }
    decode_stream_body(body, charset)
        .map(|text| (text, Vec::new()))
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
        SupportedCharset::Exotic(_) => {
            unreachable!("exotic JSON decodes dispatch in decode_json_body")
        }
    }
}

/// Strict one-shot decode for the form layer-1 (`QueryDict(bytes)` calls
/// `bytes.decode(encoding)`): tails fail here (no stream drop). `Ok(None)`
/// is the `UnicodeDecodeError` whole-body latin-1 fallback; `Err` is any
/// other exception (the pinned 500). Surrogate spans survive: a raw
/// `+2AE-` in a utf-7 form body must 400 at the CharField, exactly as a
/// percent-encoded one does (live probe).
fn decode_oneshot_strict(
    body: &[u8],
    charset: SupportedCharset,
) -> Result<Option<super::body_decoders::Decoded>, BodyError> {
    if let SupportedCharset::Exotic(_) = charset {
        use super::body_decoders::OneshotFail;
        return match super::body_decoders::decode_oneshot_strict(body, &charset) {
            Ok(decoded) => Ok(Some(decoded)),
            Err(OneshotFail::Fallback) => Ok(None),
            Err(OneshotFail::Server) => Err(BodyError::ServerError),
        };
    }
    // The nine 627 charsets never emit lone surrogates in strict mode
    // (unpaired surrogates are errors), so spans stay empty.
    let text = match charset {
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
        SupportedCharset::Exotic(_) => {
            unreachable!("handled above")
        }
    };
    Ok(text.map(|text| super::body_decoders::Decoded {
        text,
        surr: Vec::new(),
    }))
}

/// Lossy one-shot decode (`force_str(..., errors="replace")`, `unquote`
/// runs, RFC 2231 values): undecodable spans become U+FFFD. The BOM
/// codecs assume little-endian when no BOM is present (verified against
/// CPython) and incomplete tails become a single U+FFFD. Exotic codecs
/// that reject `replace` fail with the pinned 500. The spans are always
/// empty for the nine 627 charsets.
fn decode_oneshot_replace(
    bytes: &[u8],
    charset: SupportedCharset,
) -> Result<super::body_decoders::Decoded, BodyError> {
    if let SupportedCharset::Exotic(_) = charset {
        return super::body_decoders::decode_oneshot_replace(bytes, &charset)
            .map_err(|_| BodyError::ServerError);
    }
    Ok(super::body_decoders::Decoded {
        text: match charset {
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
            SupportedCharset::Exotic(_) => {
                unreachable!("handled above")
            }
        },
        surr: Vec::new(),
    })
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
/// The E0/F0/F4 second-byte ranges fire eagerly (a truncated tail with an
/// out-of-range second byte still errors), but the ED surrogate range is
/// only checked at assembly: `\xed\xa0` + EOF is dropped, not an error
/// (stdlib-probed).
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
    let specified = big_endian.is_some();
    let (units, big_endian, bom_seen) = split_utf16_units(body, big_endian);
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
    // No-BOM validity rule (stdlib-probed): without a BOM the stream
    // decodes native-LE and the first codec error wins; only a clean
    // decode that emitted at least one char is the BOM error (empty
    // output decodes to `""`).
    if !specified && !bom_seen && !out.is_empty() {
        return Err("UTF-16 stream does not start with BOM".to_owned());
    }
    Ok(out)
}

/// Split off an optional BOM (consumed when present, little-endian assumed
/// when `big_endian` is `None` and no BOM leads) and pair the rest into
/// units with absolute byte offsets; a trailing odd byte is dropped.
/// Returns the units, the resolved byte order, and whether a BOM led.
fn split_utf16_units(body: &[u8], big_endian: Option<bool>) -> (Vec<(u16, usize)>, bool, bool) {
    // Error positions are absolute in the stream: a consumed BOM still
    // counts (CPython reports the lone-low after a BOM at 2-3, not 0-1).
    let (body, big_endian, bom_seen) = match big_endian {
        Some(big_endian) => (body, big_endian, false),
        None => {
            if let Some(rest) = body.strip_prefix(b"\xFF\xFE") {
                (rest, false, true)
            } else if let Some(rest) = body.strip_prefix(b"\xFE\xFF") {
                (rest, true, true)
            } else {
                (body, false, false)
            }
        }
    };
    let base = if bom_seen { 2 } else { 0 };
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
    (units, big_endian, bom_seen)
}

fn decode_stream_utf32(body: &[u8], big_endian: Option<bool>) -> Result<String, String> {
    let specified = big_endian.is_some();
    let (words, big_endian, bom_seen) = split_utf32_units(body, big_endian);
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
    // Same no-BOM validity rule as utf-16 (stdlib-probed).
    if !specified && !bom_seen && !out.is_empty() {
        return Err("UTF-32 stream does not start with BOM".to_owned());
    }
    Ok(out)
}

fn split_utf32_units(body: &[u8], big_endian: Option<bool>) -> (Vec<(u32, usize)>, bool, bool) {
    // Absolute stream positions: a consumed BOM still counts.
    let (body, big_endian, bom_seen) = match big_endian {
        Some(big_endian) => (body, big_endian, false),
        None => {
            if let Some(rest) = body.strip_prefix(b"\xFF\xFE\x00\x00") {
                (rest, false, true)
            } else if let Some(rest) = body.strip_prefix(b"\x00\x00\xFE\xFF") {
                (rest, true, true)
            } else {
                (body, false, false)
            }
        }
    };
    let base = if bom_seen { 4 } else { 0 };
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
    (words, big_endian, bom_seen)
}

fn decode_oneshot_utf16(body: &[u8], big_endian: Option<bool>) -> Option<String> {
    // Strict one-shot (`bytes.decode`: no BOM requirement, native-LE
    // default, BOM consumed when present): whole units must consume every
    // byte; odd tails and lone surrogates fail (and the form layer falls
    // back to latin-1).
    let (body, big_endian) = match big_endian {
        Some(big_endian) => (body, big_endian),
        None => {
            if let Some(rest) = body.strip_prefix(b"\xFF\xFE") {
                (rest, false)
            } else if let Some(rest) = body.strip_prefix(b"\xFE\xFF") {
                (rest, true)
            } else {
                (body, false)
            }
        }
    };
    if !body.len().is_multiple_of(2) {
        return None;
    }
    let mut units = Vec::with_capacity(body.len() / 2);
    for pair in body.as_chunks::<2>().0 {
        let value = if big_endian {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from_le_bytes([pair[0], pair[1]])
        };
        units.push(value);
    }
    let mut out = String::new();
    let mut index = 0;
    while index < units.len() {
        let value = units[index];
        if (0xD800..0xDC00).contains(&value) {
            match units.get(index + 1) {
                Some(low) if (0xDC00..0xE000).contains(low) => {
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
    // Strict one-shot, same BOM rules as utf-16 (native-LE default).
    let (body, big_endian) = match big_endian {
        Some(big_endian) => (body, big_endian),
        None => {
            if let Some(rest) = body.strip_prefix(b"\xFF\xFE\x00\x00") {
                (rest, false)
            } else if let Some(rest) = body.strip_prefix(b"\x00\x00\xFE\xFF") {
                (rest, true)
            } else {
                (body, false)
            }
        }
    };
    if !body.len().is_multiple_of(4) {
        return None;
    }
    let mut out = String::new();
    for word in body.as_chunks::<4>().0 {
        let value = if big_endian {
            u32::from_be_bytes([word[0], word[1], word[2], word[3]])
        } else {
            u32::from_le_bytes([word[0], word[1], word[2], word[3]])
        };
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
/// chars pass through verbatim (`_generate_unquoted_parts`). `incoming`
/// carries layer-1 surrogate spans (byte offsets in `raw`); they can
/// only sit on non-ASCII placeholders, which pass through 1:1, so each
/// one re-attaches at the output byte length when its char is copied.
fn percent_decode_str(
    raw: &str,
    charset: &SupportedCharset,
    incoming: &[(usize, u16)],
) -> Result<(String, Vec<(usize, u16)>), BodyError> {
    // `unquote` fast path (`urllib/parse.py`): no `%` anywhere means the
    // value passes through untouched — the charset decoder never runs.
    // Without this, `idna` and the bytes transforms (which reject plain
    // ASCII under `replace`) 500 on %-less values, and punycode mangles
    // %-less keys (`name` -> controls); verified live against Django.
    if !raw.contains('%') {
        return Ok((raw.to_owned(), incoming.to_vec()));
    }
    let mut out = String::new();
    let mut surr = Vec::new();
    let mut run = Vec::new();
    // One ASCII run: decode, append, shift its spans to output offsets.
    let flush = |out: &mut String,
                 surr: &mut Vec<(usize, u16)>,
                 run: &mut Vec<u8>|
     -> Result<(), BodyError> {
        if run.is_empty() {
            return Ok(());
        }
        let base = out.len();
        let decoded = decode_oneshot_replace(&percent_unescape(run), *charset)?;
        out.push_str(&decoded.text);
        surr.extend(decoded.surr.iter().map(|(off, v)| (base + off, *v)));
        run.clear();
        Ok(())
    };
    let mut inc = incoming.iter().peekable();
    for (at, c) in raw.char_indices() {
        if c.is_ascii() {
            run.push(c as u8);
            continue;
        }
        flush(&mut out, &mut surr, &mut run)?;
        // Spans arrive in offset order; stale ones (no char starts
        // here — unreachable from strict decoders) drop.
        while let Some((off, v)) = inc.peek() {
            if *off > at {
                break;
            }
            if *off == at {
                surr.push((out.len(), *v));
            }
            inc.next();
        }
        out.push(c);
    }
    flush(&mut out, &mut surr, &mut run)?;
    Ok((out, surr))
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
    let charset = content_type_charset(content_type)?;
    let layer1 =
        decode_oneshot_strict(body, charset)?.unwrap_or_else(|| super::body_decoders::Decoded {
            text: body.iter().map(|b| *b as char).collect(),
            surr: Vec::new(),
        });
    let text = layer1.text;
    if text.is_empty() {
        return Ok(FormBody::default());
    }
    // `parse_qsl` counts segments before parsing: 1 + separator count.
    if 1 + text.bytes().filter(|b| *b == b'&').count() > 1000 {
        return Err(BodyError::ServerError);
    }
    let mut form = FormBody::default();
    let mut seg_start = 0;
    for segment in text.split('&') {
        let seg_end = seg_start + segment.len();
        if !segment.is_empty() {
            let (name, value) = match segment.split_once('=') {
                Some((name, value)) => (name, value),
                // No `=`: kept with a blank value (`keep_blank_values`).
                None => (segment, ""),
            };
            // Layer-1 spans inside the value re-base to the value start
            // (`+`→space is byte-preserving, so they stay valid through
            // the replacement); spans in keys drop (unknown keys never
            // validate, so a mangled key behaves identically).
            let val_start = seg_start + name.len() + usize::from(segment.contains('='));
            let incoming: Vec<(usize, u16)> = layer1
                .surr
                .iter()
                .filter(|(off, _)| *off >= val_start && *off < seg_end)
                .map(|(off, v)| (off - val_start, *v))
                .collect();
            let key = percent_decode_str(&name.replace('+', " "), &charset, &[])?.0;
            let (text, surr) = percent_decode_str(&value.replace('+', " "), &charset, &incoming)?;
            let slot = form.texts.entry(key.clone()).or_default();
            if slot.is_empty() {
                form.text_order.push(key);
            }
            slot.push(FormValue { text, surr });
        }
        seg_start = seg_end + 1;
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
    // Check order mirrors `MultipartParser.__init__` (4.2.30): the
    // case-sensitive `multipart/` prefix first, then the ASCII check.
    if !content_type.starts_with("multipart/") {
        // Case-sensitive, on the full header (`P19` probe).
        return Err(BodyError::ParseDetail(format!(
            "Multipart form parse error - Invalid Content-Type: {content_type}"
        )));
    }
    if !content_type.is_ascii() {
        return Err(BodyError::ParseDetail(format!(
            "Multipart form parse error - Invalid non-ASCII Content-Type in multipart: {content_type}"
        )));
    }
    let (_, params) = parse_header_parameters(content_type)?;
    // Duplicate params: last wins (Django's params dict overwrites).
    let boundary = params
        .iter()
        .rfind(|(name, _)| name == "boundary")
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
    let charset = content_type_charset(content_type)?;
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
    // `BoundaryIter.__init__` needs one remaining byte per sub-stream: a
    // body ending exactly at a separator (or an empty body under a lying
    // Content-Length) yields no trailing item. Leading/middle empties
    // (preamble, adjacent separators) still count.
    if chunks.last().is_some_and(|chunk| chunk.is_empty()) {
        chunks.pop();
    }
    let mut form = FormBody::default();
    let mut num_post_keys: usize = 0;
    let mut num_files: usize = 0;
    let mut num_bytes_read: usize = 0;
    // Every chunk (preamble, parts, epilogue) runs the item loop; the
    // preamble/epilogue only ever yield RAW items, which still count.
    for chunk in chunks {
        let content = strip_one_crlf(chunk);
        let item = classify_part(content)?;
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
                let key = decode_oneshot_replace(&name, *charset)?.text;
                let mut value = data;
                if item_transfer_is_base64(content)? {
                    // Fields are lenient: undecodable base64 keeps the
                    // raw bytes (`P33` probe).
                    if let Ok(decoded) = binascii_b64decode(&value) {
                        value = decoded;
                    }
                }
                let decoded = decode_oneshot_replace(&value, *charset)?;
                let slot = form.texts.entry(key.clone()).or_default();
                if slot.is_empty() {
                    form.text_order.push(key);
                }
                slot.push(FormValue {
                    text: decoded.text,
                    surr: decoded.surr,
                });
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
                // Filenames never validate (and sanitize strips the
                // U+FFFD placeholders as non-printable): spans drop.
                let filename =
                    sanitize_file_name(&decode_oneshot_replace(&filename, *charset)?.text)?;
                let Some(filename) = filename else { continue };
                let filename = truncate_uploaded_name(&filename);
                let mut value = data;
                if item_transfer_is_base64(content)? {
                    value = binascii_b64decode(&value).map_err(|_| {
                        BodyError::ParseDetail(
                            "Multipart form parse error - Could not decode base64 data.".to_owned(),
                        )
                    })?;
                }
                let key = decode_oneshot_replace(&name, *charset)?.text;
                // `MemoryFileUploadHandler.activated`: the whole body fits
                // in `FILE_UPLOAD_MAX_MEMORY_SIZE` (2621440).
                let in_memory = body.len() <= 2_621_440;
                let slot = form.files.entry(key.clone()).or_default();
                if slot.is_empty() {
                    form.file_order.push(key);
                }
                slot.push(FilePart {
                    filename,
                    content_type: String::from_utf8_lossy(&content_type).into_owned(),
                    bytes: value,
                    in_memory,
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
fn classify_part(content: &[u8]) -> Result<PartItem, BodyError> {
    let window = content.len().min(1024);
    let Some(end) = find_subsequence(&content[..window], b"\r\n\r\n") else {
        return Ok(PartItem::Raw);
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
        let (name, main_value, params) = parse_header_line(line)?;
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
        return Ok(PartItem::Raw);
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
        return Ok(PartItem::Nameless);
    };
    let name = strip_ascii_whitespace(&name);
    Ok(match filename {
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
    })
}

/// Whether the part declares `Content-Transfer-Encoding: base64` (the
/// header line is lowercased by `parse_header_parameters`, so the match
/// is case-insensitive; the last such line wins).
fn item_transfer_is_base64(content: &[u8]) -> Result<bool, BodyError> {
    let window = content.len().min(1024);
    let Some(end) = find_subsequence(&content[..window], b"\r\n\r\n") else {
        return Ok(false);
    };
    let mut encoding = String::new();
    for line in content[..end].split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Ok(line) = std::str::from_utf8(line) else {
            continue;
        };
        let (name, main_value, _) = parse_header_line(line)?;
        if name.eq_ignore_ascii_case("content-transfer-encoding") {
            encoding = String::from_utf8_lossy(&main_value).into_owned();
        }
    }
    Ok(encoding == "base64")
}

/// One header line: `parse_header_parameters` lowercases the whole
/// `name: value` main segment, so the returned main value is lowercase
/// (matching Django's stored `meta_data`); param values keep their case.
/// Lines without a colon yield the empty name (skipped by the caller).
/// The triple is (field name, raw value, parameters).
type ParsedHeaderLine = (String, Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>);

fn parse_header_line(line: &str) -> Result<ParsedHeaderLine, BodyError> {
    let (main, params) = parse_header_parameters(line)?;
    let Some((name, value)) = main.split_once(':') else {
        return Ok((String::new(), Vec::new(), Vec::new()));
    };
    // Param values here are raw bytes of the (already unquoted) text.
    let params = params
        .into_iter()
        .map(|(key, value)| (key.into_bytes(), value.into_bytes()))
        .collect();
    // The field name is NOT stripped (4.2 compares it verbatim, so
    // `Name : v` never matches); the value trims like Django's per-use
    // `.strip()` calls (N3/N5).
    Ok((name.to_owned(), value.trim().as_bytes().to_vec(), params))
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

/// `MultiPartParser.sanitize_file_name` (`django/http/multipartparser.py`):
/// unescape HTML entities first, strip directories, drop non-printables;
/// empty results mean "no file here" (the part is skipped). Returns `None`
/// for empty/`.`/`..` names.
///
/// A decimal ref with more than 4300 digits raises `ValueError` in CPython
/// (the `int()` digit cap, 3.11+), which Django lets escape as the generic
/// 500; that surfaces here as `Err(BodyError::ServerError)`.
fn sanitize_file_name(name: &str) -> Result<Option<String>, BodyError> {
    let name = html_unescape(name)?;
    let name = name.rsplit('/').next().unwrap_or("");
    let name = name.rsplit('\\').next().unwrap_or("");
    let name: String = name.chars().filter(|c| is_printable(*c)).collect();
    if name.is_empty() || name == "." || name == ".." {
        return Ok(None);
    }
    Ok(Some(name))
}

/// `UploadedFile._set_name` truncation (`django/core/files/uploadedfile.py`):
/// names longer than 255 chars are cut to exactly 255, keeping the
/// `os.path.splitext` extension and cutting the root to fit. All lengths and
/// slices are char-counted. The sibling steps need no code: `basename` is
/// identity (no `/` survives the strip above) and `validate_file_name` is
/// identity (no slashes; the name is never `""`, `"."` or `".."`).
fn truncate_uploaded_name(name: &str) -> String {
    if name.chars().count() <= 255 {
        return name.to_owned();
    }
    let (root, ext) = splitext_no_sep(name);
    let ext: String = ext.chars().take(255).collect();
    let root: String = root.chars().take(255 - ext.chars().count()).collect();
    format!("{root}{ext}")
}

/// `genericpath._splitext` with no separator present: split at the last `.`
/// unless every char before it is a dot (leading-dot names keep the dots in
/// the root, e.g. `('.bashrc', '')`). The cut is at an ASCII byte, so both
/// slices are char boundaries.
fn splitext_no_sep(name: &str) -> (&str, &str) {
    if let Some(dot) = name.rfind('.') {
        if name[..dot].chars().any(|c| c != '.') {
            return (&name[..dot], &name[dot..]);
        }
    }
    (name, "")
}

/// `html.unescape` (`CPython/Lib/html/__init__.py`): the
/// `&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\x0C <&#;]{1,32};?)` scan with
/// the html5 table (`sanitize_data`), the WHATWG invalid-ref rules, and
/// the decimal digit-cap error. Every slice below cuts at ASCII matches,
/// so byte offsets are always char boundaries.
fn html_unescape(text: &str) -> Result<String, BodyError> {
    if !text.contains('&') {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        match match_charref(rest)? {
            Some((replacement, consumed)) => {
                out.push_str(&replacement);
                rest = &rest[consumed..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// One `_charref` match at `&...`: `Some((replacement, bytes))` when the
/// regex matches (even when the replacement is the literal text — CPython
/// consumes the group either way), `None` when it does not match, `Err`
/// on the decimal digit-cap `ValueError`.
fn match_charref(text: &str) -> Result<Option<(String, usize)>, BodyError> {
    debug_assert!(text.starts_with('&'));
    let after = &text[1..];
    if let Some(body) = after.strip_prefix('#') {
        return match_numeric_ref(body);
    }
    // Named ref: 1..=32 class chars (counted in chars, like the `re`
    // engine), then one optional `;`.
    let mut end = 1;
    let mut count = 0;
    for (offset, c) in after.char_indices() {
        if count == 32 || !is_charref_char(c) {
            break;
        }
        count += 1;
        end = 1 + offset + c.len_utf8();
    }
    if count == 0 {
        return Ok(None);
    }
    let mut group = &text[1..end];
    let mut consumed = end;
    if text[end..].starts_with(';') {
        group = &text[1..end + 1];
        consumed = end + 1;
    }
    Ok(Some((replace_named_ref(group), consumed)))
}

/// The named-ref class `[^\\t\\n\\f <&#;]` (note: `\r` and non-ASCII pass).
fn is_charref_char(c: char) -> bool {
    !matches!(c, '\t' | '\n' | '\x0C' | ' ' | '<' | '&' | '#' | ';')
}

/// `_replace_charref` named arm: the full key, then the longest `len >= 2`
/// prefix in the table (the remainder stays literal), else the literal
/// text. The prefix fallback also applies to `;`-terminated groups
/// (`&notanentity;` → `¬anentity;`).
fn replace_named_ref(group: &str) -> String {
    if let Some(hit) = lookup_entity(group) {
        return hit.to_owned();
    }
    let chars: Vec<char> = group.chars().collect();
    for end in (2..chars.len()).rev() {
        let prefix: String = chars[..end].iter().collect();
        if let Some(hit) = lookup_entity(&prefix) {
            let rest: String = chars[end..].iter().collect();
            return format!("{hit}{rest}");
        }
    }
    format!("&{group}")
}

fn lookup_entity(key: &str) -> Option<&'static str> {
    HTML5_ENTITIES
        .binary_search_by(|probe| probe.0.cmp(key))
        .ok()
        .map(|index| HTML5_ENTITIES[index].1)
}

/// The numeric alternatives `#[0-9]+;?` / `#[xX][0-9a-fA-F]+;?`; `body` is
/// the text after `&#`. All-ASCII, so byte offsets are char boundaries.
fn match_numeric_ref(body: &str) -> Result<Option<(String, usize)>, BodyError> {
    let (hex, digits) = match body.strip_prefix(['x', 'X']) {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let is_digit = |b: &u8| {
        if hex {
            b.is_ascii_hexdigit()
        } else {
            b.is_ascii_digit()
        }
    };
    let digit_len = digits.bytes().take_while(is_digit).count();
    if digit_len == 0 {
        return Ok(None);
    }
    let mut consumed = 2 + digit_len + usize::from(hex);
    if digits[digit_len..].starts_with(';') {
        consumed += 1;
    }
    let digits = &digits[..digit_len];
    // CPython converts the full digit run with `int()`: more than 4300
    // decimal digits raises `ValueError` (the count, not the value —
    // 5000 zeros raise too). Hex parsing is linear-time and unbounded.
    if !hex && digits.len() > 4300 {
        return Err(BodyError::ServerError);
    }
    let radix = if hex { 16 } else { 10 };
    let value = u64::from_str_radix(digits, radix).unwrap_or(u64::MAX);
    Ok(Some((replace_numeric_ref(value), consumed)))
}

/// `_replace_charref` numeric arm: the remap table first (it shadows the
/// codepoint set on 0x80-0x9F), then surrogates/out-of-range → U+FFFD,
/// then invalid codepoints → the empty string, else the char.
fn replace_numeric_ref(value: u64) -> String {
    if value <= 0x9F {
        if let Ok(index) = INVALID_CHARREFS.binary_search_by(|probe| (probe.0 as u64).cmp(&value)) {
            return INVALID_CHARREFS[index].1.to_owned();
        }
    }
    if value > 0x10FFFF || (0xD800..0xE000).contains(&value) {
        return "\u{FFFD}".to_owned();
    }
    let value = value as u32;
    if in_ranges(value, INVALID_CODEPOINT_RANGES) {
        return String::new();
    }
    char::from_u32(value).map_or_else(|| "\u{FFFD}".to_owned(), |c| c.to_string())
}

/// CPython `str.isprintable` per char: false exactly on general categories
/// C*/Z* other than U+0020 (`sanitize_data::NONPRINTABLE_RANGES`, verified
/// against the interpreter for every codepoint).
fn is_printable(c: char) -> bool {
    !in_ranges(c as u32, NONPRINTABLE_RANGES)
}

/// Point-in-sorted-disjoint-ranges.
fn in_ranges(value: u32, ranges: &[(u32, u32)]) -> bool {
    let index = ranges.partition_point(|&(lo, _)| lo <= value);
    index > 0 && value <= ranges[index - 1].1
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
        // Bogus, empty, and punctuation-only degrade to utf-8
        // (exotic codecs resolve since PIDASHCONV-693 — pinned by
        // `body_battery::battery_resolution` — so they left this list).
        // Windows-only aliases (`ansi`/`dbcs` -> `mbcs`) reject like
        // CPython's LookupError on Linux (review fix: the engine-less
        // module used to reach the dispatcher `todo!` and panic).
        for name in ["bogus", "", "---", "utf 8x", "ansi", "dbcs", "ANSI", "oem"] {
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
        // No-BOM validity rule: the first codec error wins over the BOM
        // check (`abcd` is a utf-32-LE range error, not a BOM error).
        assert_eq!(
            decode_stream_body(b"abcd", SupportedCharset::Utf32).unwrap_err(),
            "'utf-32-le' codec can't decode bytes in position 0-3: code point not in range(0x110000)"
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
        // The ED surrogate range is assembly-checked: a truncated
        // ED tail drops (stdlib-probed), unlike E0/F0/F4.
        assert_eq!(
            decode_stream_body(b"\xed\xa0", SupportedCharset::Utf8).unwrap(),
            ""
        );
        assert_eq!(
            decode_stream_body(b"a\xed\xa0", SupportedCharset::Utf8).unwrap(),
            "a"
        );
        // E0/F0/F4 second-byte ranges fire eagerly, even truncated.
        for tail in [b"\xe0\x80".as_slice(), b"\xf0\x80", b"\xf4\x90"] {
            let detail = decode_stream_body(tail, SupportedCharset::Utf8).unwrap_err();
            assert!(detail.ends_with("invalid continuation byte"), "{detail:?}");
        }
        // Bare leads drop (nothing to range-check yet).
        for tail in [b"\xe0".as_slice(), b"\xed", b"\xf0", b"\xf4", b"\xc2"] {
            assert_eq!(
                decode_stream_body(tail, SupportedCharset::Utf8).unwrap(),
                ""
            );
        }
        // Valid two-byte prefix at EOF drops.
        assert_eq!(
            decode_stream_body(b"\xe4\xb8", SupportedCharset::Utf8).unwrap(),
            ""
        );
    }

    #[test]
    fn utf16_utf32_no_bom_validity() {
        // Under two bytes without a BOM: dropped, not a BOM error (F7).
        assert_eq!(
            decode_stream_body(b"", SupportedCharset::Utf16).unwrap(),
            ""
        );
        assert_eq!(
            decode_stream_body(b"a", SupportedCharset::Utf16).unwrap(),
            ""
        );
        assert_eq!(
            decode_stream_body(b"", SupportedCharset::Utf32).unwrap(),
            ""
        );
        assert_eq!(
            decode_stream_body(b"a", SupportedCharset::Utf32).unwrap(),
            ""
        );
        assert_eq!(
            decode_stream_body(b"abc", SupportedCharset::Utf32).unwrap(),
            ""
        );
        // Clean decode emitting >= 1 char without a BOM: BOM error (F8).
        assert_eq!(
            decode_stream_body(b"ab", SupportedCharset::Utf16).unwrap_err(),
            "UTF-16 stream does not start with BOM"
        );
        assert_eq!(
            decode_stream_body(b"A\x00\x00\x00", SupportedCharset::Utf32).unwrap_err(),
            "UTF-32 stream does not start with BOM"
        );
        assert_eq!(
            decode_stream_body(b"A\x00\xd8", SupportedCharset::Utf16).unwrap_err(),
            "UTF-16 stream does not start with BOM"
        );
        // A pending high surrogate drops to zero chars: empty, no BOM error.
        assert_eq!(
            decode_stream_body(b"\x00\xd8", SupportedCharset::Utf16).unwrap(),
            ""
        );
        // First codec error wins (utf-16-le names, absolute positions).
        assert_eq!(
            decode_stream_body(b"\x00\xdc", SupportedCharset::Utf16).unwrap_err(),
            "'utf-16-le' codec can't decode bytes in position 0-1: illegal encoding"
        );
        assert_eq!(
            decode_stream_body(b"A\x00\x00\xdc", SupportedCharset::Utf16).unwrap_err(),
            "'utf-16-le' codec can't decode bytes in position 2-3: illegal encoding"
        );
        assert_eq!(
            decode_stream_body(b"\x00\xd8\x00\xd8", SupportedCharset::Utf16).unwrap_err(),
            "'utf-16-le' codec can't decode bytes in position 0-1: illegal UTF-16 surrogate"
        );
        assert_eq!(
            decode_stream_body(b"A\x00\x00\x00abcd", SupportedCharset::Utf32).unwrap_err(),
            "'utf-32-le' codec can't decode bytes in position 4-7: code point not in range(0x110000)"
        );
        // Explicit-endian paths are unchanged (no BOM check, odd tails drop).
        assert_eq!(
            decode_stream_body(b"abc", SupportedCharset::Utf16Be).unwrap(),
            "\u{6162}"
        );
        assert_eq!(
            decode_stream_body(b"abc", SupportedCharset::Utf32Le).unwrap(),
            ""
        );
        assert_eq!(
            decode_stream_body(b"\xdc\x00", SupportedCharset::Utf16Be).unwrap_err(),
            "'utf-16-be' codec can't decode bytes in position 0-1: illegal encoding"
        );
    }

    #[test]
    fn oneshot_utf16_utf32_no_bom_needed() {
        // One-shot (`bytes.decode`) has no BOM requirement: native-LE
        // default, BOM consumed when present (F9).
        let one = |bytes: &[u8], cs: SupportedCharset| {
            decode_oneshot_strict(bytes, cs)
                .expect("oneshot")
                .map(|d| d.text)
        };
        assert_eq!(
            one(b"abcd", SupportedCharset::Utf16).as_deref(),
            Some("\u{6261}\u{6463}")
        );
        assert_eq!(one(b"", SupportedCharset::Utf16).as_deref(), Some(""));
        assert_eq!(
            one(b"\xff\xfe", SupportedCharset::Utf16).as_deref(),
            Some("")
        );
        assert_eq!(
            one(b"\xfe\xff\x00A", SupportedCharset::Utf16).as_deref(),
            Some("A")
        );
        // Odd tails and lone surrogates fail (form falls back to latin-1).
        assert_eq!(one(b"abc", SupportedCharset::Utf16), None);
        assert_eq!(one(b"\x00\xd8", SupportedCharset::Utf16), None);
        // utf-32 one-shot: `abcd` is LE 0x64636261, out of range.
        assert_eq!(one(b"abcd", SupportedCharset::Utf32), None);
        assert_eq!(one(b"abc", SupportedCharset::Utf32), None);
        assert_eq!(one(b"", SupportedCharset::Utf32).as_deref(), Some(""));
        assert_eq!(
            one(b"A\x00\x00\x00", SupportedCharset::Utf32).as_deref(),
            Some("A")
        );
        assert_eq!(
            one(b"\xff\xfe\x00\x00A\x00\x00\x00", SupportedCharset::Utf32).as_deref(),
            Some("A")
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
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
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
        // Only the choice skips (S6/S7/S8 probes): dates keep `''` so
        // the gate sees presence and coercion maps HTML `''` to None.
        let map = form(b"name=x&timezone=&start_date=&end_date=");
        assert_eq!(map["name"], Value::String("x".to_owned()));
        assert!(!map.contains_key("timezone"));
        assert_eq!(map["start_date"], Value::String(String::new()));
        assert_eq!(map["end_date"], Value::String(String::new()));
        // Non-skipped blanks flow through as empty strings.
        let map = form(b"name=&description=&owned_by=");
        assert_eq!(map["name"], Value::String(String::new()));
        assert_eq!(map["description"], Value::String(String::new()));
        assert_eq!(map["owned_by"], Value::String(String::new()));
    }

    #[test]
    fn form_unquote_fast_path() {
        // `unquote` never runs the charset decoder on %-less values
        // (PIDASHCONV-693): `idna` accepts plain ASCII, and punycode
        // falls non-ASCII bodies back to latin-1 without mangling the
        // `name` key (contract TestCharsetExotic693 pins the 201s).
        let form = |ct: &str, body: &[u8]| match negotiate(ct, body, &CYCLE_BODY_SPEC).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert!(files.is_empty());
                map
            }
            other => panic!("expected form, got {other:?}"),
        };
        let map = form(
            "application/x-www-form-urlencoded; charset=idna",
            b"name=FIDNA693",
        );
        assert_eq!(map["name"], Value::String("FIDNA693".to_owned()));
        let map = form(
            "application/x-www-form-urlencoded; charset=punycode",
            b"name=\xff",
        );
        assert_eq!(map["name"], Value::String("ÿ".to_owned()));
        // With a `%` present the decoder runs, and `idna` rejects.
        assert_eq!(
            negotiate(
                "application/x-www-form-urlencoded; charset=idna",
                b"a%20=b",
                &CYCLE_BODY_SPEC,
            )
            .unwrap_err(),
            BodyError::ServerError,
        );
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
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => (map, files),
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
    fn indexed_list_keys() {
        let form = |body: &[u8]| match negotiate(
            "application/x-www-form-urlencoded",
            body,
            &MODULE_BODY_SPEC,
        )
        .unwrap()
        {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => (map, files),
            other => panic!("expected form, got {other:?}"),
        };
        let strs = |items: &[&str]| {
            Value::Array(
                items
                    .iter()
                    .map(|v| Value::String((*v).to_owned()))
                    .collect(),
            )
        };
        // Sparse indexes sort numerically, not lexicographically (F5a).
        let (map, files) = form(b"name=m&members%5B10%5D=j&members%5B2%5D=b");
        assert_eq!(map["members"], strs(&["b", "j"]));
        assert!(files.is_empty());
        // `007` and `7` are the same index; last in arrival wins.
        let (map, _) = form(b"members%5B007%5D=a&members%5B7%5D=b");
        assert_eq!(map["members"], strs(&["b"]));
        // Exact key wins over indexed keys (F5b).
        let (map, _) = form(b"members=exact&members%5B0%5D=idx");
        assert_eq!(map["members"], strs(&["exact"]));
        // No indexed keys and no exact key: the field stays missing.
        let (map, _) = form(b"name=m&other%5B0%5D=x");
        assert!(!map.contains_key("members"));
        // Non-digit, empty, and unclosed indexes are ignored; a newline
        // suffix never matches (Python `.`).
        let (map, _) = form(b"members%5Ba%5D=x&members%5B%5D=y&members%5B0=z");
        assert!(!map.contains_key("members"));
        let (map, _) = form(b"members%5B0%5D%0A=x&members%5B1%5D=ok");
        assert_eq!(map["members"], strs(&["ok"]));
        // Dict-form arrives as single-key `{suffix: [value]}` pairs in
        // first-seen suffix order (P5).
        let (map, _) = form(b"members%5B0%5Dy=2&members%5B0%5Dx=1");
        let mut first = Map::new();
        first.insert(
            "y".to_owned(),
            Value::Array(vec![Value::String("2".to_owned())]),
        );
        let mut second = Map::new();
        second.insert(
            "x".to_owned(),
            Value::Array(vec![Value::String("1".to_owned())]),
        );
        assert_eq!(
            map["members"],
            Value::Array(vec![Value::Array(vec![
                Value::Object(first),
                Value::Object(second)
            ])])
        );
        // Same suffix twice: setitem replaces (last wins).
        let (map, _) = form(b"members%5B0%5Dx=1&members%5B0%5Dx=2");
        let mut pair = Map::new();
        pair.insert(
            "x".to_owned(),
            Value::Array(vec![Value::String("2".to_owned())]),
        );
        assert_eq!(
            map["members"],
            Value::Array(vec![Value::Array(vec![Value::Object(pair)])])
        );
        // Plain entries overwrite dicts in arrival order and back (P6).
        let (map, _) = form(b"members%5B0%5Dx=1&members%5B0%5D=plain");
        assert_eq!(map["members"], strs(&["plain"]));
        let (map, _) = form(b"members%5B0%5D=plain&members%5B0%5Dx=1");
        let mut pair = Map::new();
        pair.insert(
            "x".to_owned(),
            Value::Array(vec![Value::String("1".to_owned())]),
        );
        assert_eq!(
            map["members"],
            Value::Array(vec![Value::Array(vec![Value::Object(pair)])])
        );
        // Dotted suffixes keep the dot (no dot-stripping in DRF 3.15).
        let (map, _) = form(b"members%5B0%5D.x=1");
        let mut pair = Map::new();
        pair.insert(
            ".x".to_owned(),
            Value::Array(vec![Value::String("1".to_owned())]),
        );
        assert_eq!(
            map["members"],
            Value::Array(vec![Value::Array(vec![Value::Object(pair)])])
        );
    }

    #[test]
    fn indexed_file_placeholders() {
        // A file under an indexed key arrives as a null placeholder with
        // the upload moved under the field name, in index order (P1/P4).
        let ct = "multipart/form-data; boundary=----b";
        let body = b"------b\r\nContent-Disposition: form-data; name=\"members[1]\"\r\n\r\nnot-a-uuid\r\n------b\r\nContent-Disposition: form-data; name=\"members[0]\"; filename=\"i.txt\"\r\nContent-Type: text/plain\r\n\r\nhi\r\n------b--\r\n";
        match negotiate(ct, body, &MODULE_BODY_SPEC).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert_eq!(
                    map["members"],
                    Value::Array(vec![Value::Null, Value::String("not-a-uuid".to_owned())])
                );
                assert_eq!(files["members"].len(), 1);
                assert_eq!(files["members"][0].filename, "i.txt");
                assert!(files["members"][0].in_memory);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multipart_basics_and_errors() {
        let ct = "multipart/form-data; boundary=----b";
        let mp = |body: &[u8]| negotiate(ct, body, &CYCLE_BODY_SPEC);
        // Text field round-trips (C01).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nmpcycle\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert_eq!(map["name"], Value::String("mpcycle".to_owned()));
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Unknown file is carried with bytes (C02).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nv\r\n------b\r\nContent-Disposition: form-data; name=\"att\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhi\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
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
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert!(map.is_empty());
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Empty filename arrives as a text field (P23).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"name\"; filename=\"\"\r\n\r\nefv\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
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
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert_eq!(map["name"], Value::String("abcde".to_owned()));
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Non-UTF-8 header line is skipped, killing the part (P22).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"na\xffme\"\r\n\r\nv\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert!(map.is_empty());
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Duplicate disposition lines: last wins (R9).
        let body = b"------b\r\nContent-Disposition: form-data; name=\"nope\"\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\ndup1\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert_eq!(map["name"], Value::String("dup1".to_owned()));
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // A space before the colon kills the match (4.2 compares the name
        // verbatim — N3).
        let body =
            b"------b\r\nContent-Disposition : form-data; name=\"name\"\r\n\r\nv\r\n------b--\r\n";
        match mp(body).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert!(map.is_empty());
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        // Duplicate boundary params: last wins (F13).
        let body =
            b"------b\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nv\r\n------b--\r\n";
        match negotiate(
            "multipart/form-data; boundary=nope; boundary=----b",
            body,
            &CYCLE_BODY_SPEC,
        )
        .unwrap()
        {
            NegotiatedBody::Form { map, .. } => {
                assert_eq!(map["name"], Value::String("v".to_owned()));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            negotiate(
                "multipart/form-data; boundary=----b; boundary=",
                body,
                &CYCLE_BODY_SPEC
            )
            .unwrap_err(),
            BodyError::ParseDetail(
                "Multipart form parse error - Invalid boundary in multipart: ".to_owned()
            )
        );
        // Duplicate charset params: last wins (F13). (The bad byte sits
        // mid-stream: a trailing `\xe9` would drop, not error.)
        match negotiate(
            "application/json; charset=utf-8; charset=latin-1",
            b"{\"a\":\"\xe9\"}",
            &CYCLE_BODY_SPEC,
        )
        .unwrap()
        {
            NegotiatedBody::JsonText { text, surr: _ } => assert_eq!(text, "{\"a\":\"é\"}"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            negotiate(
                "application/json; charset=latin-1; charset=utf-8",
                b"{\"a\":\"\xe9\"}",
                &CYCLE_BODY_SPEC
            )
            .unwrap_err(),
            BodyError::ParseDetail(_)
        ));
        // A body ending exactly at a separator yields no trailing item,
        // and an empty body under a lying Content-Length yields nothing
        // at all (F14) — neither trips the field budget.
        let body = b"------b\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nv\r\n------b";
        match negotiate(ct, body, &CYCLE_BODY_SPEC).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert_eq!(map["a"], Value::String("v".to_owned()));
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
        let mut headers = HeaderMap::new();
        headers.insert("content-type", ct.parse().unwrap());
        headers.insert("content-length", "64".parse().unwrap());
        match negotiate_body(&headers, b"", &CYCLE_BODY_SPEC).unwrap() {
            NegotiatedBody::Form {
                map,
                files,
                surr: _,
            } => {
                assert!(map.is_empty());
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multipart_check_order_and_obs_text() {
        // `Multipart/...` (capital M) selects the parser (case-insensitive
        // base match) but fails its case-sensitive prefix check — before
        // the ASCII check even runs (F12).
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            axum::http::HeaderValue::from_bytes(
                b"Multipart/Form-Data; boundary=x; note=\"F\xc3\xb6\"",
            )
            .unwrap(),
        );
        headers.insert("content-length", "1".parse().unwrap());
        assert_eq!(
            negotiate_body(&headers, b"x", &CYCLE_BODY_SPEC).unwrap_err(),
            BodyError::ParseDetail(
                "Multipart form parse error - Invalid Content-Type: Multipart/Form-Data; boundary=x; note=\"FÃ\u{b6}\""
                    .to_owned()
            )
        );
        // Obs-text 415 echoes decode latin-1, like uvicorn/wsgiref (F15).
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            axum::http::HeaderValue::from_bytes(b"text/pl\xe9in").unwrap(),
        );
        headers.insert("content-length", "3".parse().unwrap());
        assert_eq!(
            negotiate_body(&headers, b"xxx", &CYCLE_BODY_SPEC).unwrap_err(),
            BodyError::UnsupportedMediaType(
                "Unsupported media type \"text/pl\u{e9}in\" in request.".to_owned()
            )
        );
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
        // Unescape runs BEFORE the directory strip (F10).
        assert_eq!(filename_of(&part(b"..&#x2F;evil")), "evil");
        // Non-ASCII names survive whole (F11).
        assert_eq!(filename_of(&part("café.txt".as_bytes())), "café.txt");
    }

    #[test]
    fn filename_sanitize_exact_table() {
        // Every html5 key resolves to its value (table integrity; live
        // Django agreement is pinned by the contract suite).
        for (key, value) in HTML5_ENTITIES.iter() {
            assert_eq!(html_unescape(&format!("&{key}")).unwrap(), *value, "{key}");
        }
        // Legacy no-semicolon forms resolve; non-legacy names stay literal.
        for (entity, expected) in [
            ("&amp", "&"),
            ("&lt", "<"),
            ("&gt", ">"),
            ("&quot", "\""),
            ("&nbsp", "\u{A0}"),
            ("&copy", "©"),
            ("&reg", "®"),
            ("&not", "¬"),
            ("&apos", "&apos"),
            ("&hellip", "&hellip"),
            ("&mdash", "&mdash"),
            ("&sol", "&sol"),
        ] {
            assert_eq!(&html_unescape(entity).unwrap(), expected, "{entity}");
        }
        // Longest-prefix fallback, also for `;`-terminated groups.
        for (entity, expected) in [
            ("&ampx", "&x"),
            ("&notanentity;", "¬anentity;"),
            ("&foobar;", "&foobar;"),
            ("&", "&"),
            ("&;", "&;"),
            ("&&amp;", "&&"),
            ("&am\rp;", "&am\rp;"),
        ] {
            assert_eq!(&html_unescape(entity).unwrap(), expected, "{entity:?}");
        }
        // Past the 32-char greedy window the match cannot reach `;`.
        let long = format!("&{};b", "a".repeat(40));
        assert_eq!(&html_unescape(&long).unwrap(), &long);
    }

    #[test]
    fn filename_sanitize_exact_numeric() {
        for (entity, expected) in [
            ("&#65", "A"),
            ("&#65;", "A"),
            ("&#00065;", "A"),
            ("&#X41;", "A"),
            ("&#x41;", "A"),
            ("&#x80;", "€"),
            ("&#128;", "€"),
            ("&#13;", "\r"),
            ("&#0;", "\u{FFFD}"),
            ("&#xD800;", "\u{FFFD}"),
            ("&#xDFFF;", "\u{FFFD}"),
            ("&#x110000;", "\u{FFFD}"),
            ("&#99999999999999999999999999;", "\u{FFFD}"),
            ("&#x1;", ""),
            ("&#xB;", ""),
            ("&#xC;", "\u{C}"),
            ("&#x1F;", ""),
            ("&#x7F;", ""),
            ("&#xFDD0;", ""),
            ("&#xFFFE;", ""),
            ("&#x10FFFF;", ""),
            ("&#;", "&#;"),
            ("&#x;", "&#x;"),
            ("&#xg;", "&#xg;"),
        ] {
            assert_eq!(&html_unescape(entity).unwrap(), expected, "{entity}");
        }
        // The full WHATWG remap table.
        for (value, expected) in INVALID_CHARREFS.iter() {
            for entity in [format!("&#{value};"), format!("&#x{value:X};")] {
                assert_eq!(html_unescape(&entity).unwrap(), *expected, "{entity}");
            }
        }
        // The decimal digit cap is a count, not a value (CPython 3.11+).
        assert_eq!(
            &html_unescape(&format!("&#{};", "9".repeat(4300))).unwrap(),
            "\u{FFFD}"
        );
        assert_eq!(
            html_unescape(&format!("&#{};", "9".repeat(4301))).unwrap_err(),
            BodyError::ServerError
        );
        assert_eq!(
            html_unescape(&format!("&#{};", "0".repeat(5000))).unwrap_err(),
            BodyError::ServerError
        );
        // Hex parsing is unbounded.
        assert_eq!(
            &html_unescape(&format!("&#x{};", "F".repeat(5000))).unwrap(),
            "\u{FFFD}"
        );
    }

    #[test]
    fn filename_sanitize_exact_order_and_printable() {
        let san = |name: &str| sanitize_file_name(name).unwrap();
        // Unescape runs BEFORE the strip: decoded separators still strip.
        assert_eq!(san("..&#x2F;evil"), Some("evil".to_owned()));
        assert_eq!(san("a/b&#x5C;c"), Some("c".to_owned()));
        assert_eq!(san("&sol;..&sol;..&sol;x"), Some("x".to_owned()));
        assert_eq!(san("&#46;&#46;"), None);
        assert_eq!(san("a/.."), None);
        // `isprintable` edges: space survives, other separators/format do not.
        assert_eq!(san("a b"), Some("a b".to_owned()));
        assert_eq!(san("a\u{A0}b"), Some("ab".to_owned()));
        assert_eq!(san("a\u{2028}.txt"), Some("a.txt".to_owned()));
        assert_eq!(san("a\u{200E}b"), Some("ab".to_owned()));
        assert_eq!(san("café.txt"), Some("café.txt".to_owned()));
        assert_eq!(san("a😀b"), Some("a😀b".to_owned()));
        assert_eq!(san("e\u{301}"), Some("e\u{301}".to_owned()));
        assert_eq!(san("C:some_file.txt"), Some("C:some_file.txt".to_owned()));
        assert_eq!(san("&#xD800;.txt"), Some("\u{FFFD}.txt".to_owned()));
        assert_eq!(san("&#13;.txt"), Some(".txt".to_owned()));
        assert_eq!(san("&notanentity;.txt"), Some("¬anentity;.txt".to_owned()));
        assert_eq!(san("&#x80;.txt"), Some("€.txt".to_owned()));
        assert_eq!(san(""), None);
        assert_eq!(san("."), None);
        assert_eq!(san(".."), None);
        // The digit cap is unreachable via multipart: headers past the
        // 1024-byte window are RAW-skipped (`parse_boundary_stream`,
        // Django 4.2.30), so a 4301-digit ref never reaches `sanitize`
        // (the cap itself is covered unit-only above, like R5a).
        let ct = "multipart/form-data; boundary=----b";
        let nines = "9".repeat(4301);
        let bad = [
            b"------b\r\nContent-Disposition: form-data; name=\"att\"; filename=\"&#".as_slice(),
            nines.as_bytes(),
            b";\"\r\n\r\nx\r\n------b--\r\n".as_slice(),
        ]
        .concat();
        match negotiate(ct, &bad, &CYCLE_BODY_SPEC).unwrap() {
            NegotiatedBody::Form { map, files, .. } => {
                assert!(map.is_empty());
                assert!(files.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn uploaded_name_truncation() {
        // `UploadedFile._set_name` (Django 4.2.30): names longer than 255
        // chars are cut to exactly 255, extension-preserving; 255 and below
        // pass through untouched. Pinned against the live backends by
        // `test_filename_truncate.py`.
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
        // Boundary: 254/255 untouched, 256 cut to 255.
        assert_eq!(filename_of(&part(&vec![b'a'; 254])), "a".repeat(254));
        assert_eq!(filename_of(&part(&vec![b'a'; 255])), "a".repeat(255));
        assert_eq!(filename_of(&part(&vec![b'a'; 256])), "a".repeat(255));
        // Extension preserved; root absorbs the cut.
        let long_ext = [vec![b'a'; 900], b".txt".to_vec()].concat();
        assert_eq!(
            filename_of(&part(&long_ext)),
            format!("{}.txt", "a".repeat(251))
        );
        // Leading-dot names keep the dots in the root (no extension).
        let leading_dot = [b".".to_vec(), vec![b'b'; 300]].concat();
        assert_eq!(
            filename_of(&part(&leading_dot)),
            format!(".{}", "b".repeat(254))
        );
        // A root of only dots never splits either.
        assert_eq!(filename_of(&part(&vec![b'.'; 300])), ".".repeat(255));
        // Trailing-dot root with a 300-char tail: the tail is the extension.
        let trailing = [b"name.".to_vec(), vec![b'f'; 300]].concat();
        assert_eq!(
            filename_of(&part(&trailing)),
            format!(".{}", "f".repeat(254))
        );
        // Bare trailing dot: the dot is a 1-char extension.
        let bare_dot = [vec![b'g'; 300], b".".to_vec()].concat();
        assert_eq!(
            filename_of(&part(&bare_dot)),
            format!("{}.", "g".repeat(254))
        );
        // An extension past 255 chars is itself cut first; the root vanishes.
        let long_ext_only = [b"jk.".to_vec(), vec![b'l'; 300]].concat();
        assert_eq!(
            filename_of(&part(&long_ext_only)),
            format!(".{}", "l".repeat(254))
        );
        // Interior dots: the last dot wins.
        let multi_dot = [vec![b'h'; 200], b".mid.".to_vec(), vec![b'i'; 100]].concat();
        assert_eq!(
            filename_of(&part(&multi_dot)),
            format!("{}.{}", "h".repeat(154), "i".repeat(100))
        );
        // Leading dots with a real extension still split at the last dot.
        let dotdot_ext = [b"..".to_vec(), vec![b'd'; 300], b".txt".to_vec()].concat();
        assert_eq!(
            filename_of(&part(&dotdot_ext)),
            format!("..{}.txt", "d".repeat(249))
        );
        // Char-counted, not byte-counted.
        assert_eq!(
            filename_of(&part("é".repeat(200).as_bytes())),
            "é".repeat(200)
        );
        assert_eq!(
            filename_of(&part("é".repeat(300).as_bytes())),
            "é".repeat(255)
        );
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
        let parse = |content_type: &str| select_parser(content_type).expect("parse params");
        assert_eq!(parse("application/json"), Parser::Json);
        assert_eq!(parse("Application/JSON"), Parser::Json);
        assert_eq!(parse("application/json; charset=utf-8"), Parser::Json);
        assert_eq!(parse("application/x-www-form-urlencoded"), Parser::Form);
        assert_eq!(parse("multipart/form-data; boundary=x"), Parser::Multipart);
        assert_eq!(parse("*/*"), Parser::Json);
        assert_eq!(parse("application/*"), Parser::Json);
        assert_eq!(parse("text/plain"), Parser::None);
        assert_eq!(parse("text/*"), Parser::None);
        assert_eq!(parse(""), Parser::None);
        assert_eq!(parse("application/jsonn"), Parser::None);
    }

    #[test]
    fn header_params_shapes() {
        let params_of = |line: &str| parse_header_parameters(line).expect("parse header params");
        let (main, params) = params_of("Multipart/Form-Data; boundary=----x");
        assert_eq!(main, "multipart/form-data");
        assert_eq!(params, vec![("boundary".to_owned(), "----x".to_owned())]);
        let (main, params) = params_of("multipart/form-data; boundary=\"a;b\"");
        assert_eq!(main, "multipart/form-data");
        assert_eq!(params, vec![("boundary".to_owned(), "a;b".to_owned())]);
        let (main, params) = params_of("application/x-www-form-urlencoded; Charset=\"latin-1\"");
        assert_eq!(main, "application/x-www-form-urlencoded");
        assert_eq!(params, vec![("charset".to_owned(), "latin-1".to_owned())]);
        let (main, params) = params_of("text/plain");
        assert_eq!(main, "text/plain");
        assert!(params.is_empty());
    }
}
