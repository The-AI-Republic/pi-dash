//! Form and multipart body parsing for the license write endpoints
//! (PIDASHCONV-1117).
//!
//! The five license write endpoints declare no `parser_classes`, so Django
//! serves DRF's default parsers (`JSONParser`, `FormParser`,
//! `MultiPartParser`, in that order). This module is the form/multipart
//! half: content-type routing (`select_parser`), urlencoded parsing
//! (`QueryDict` semantics) and multipart parsing (Django's
//! `multipartparser` semantics: fields + files, byte-exact `ParseError`
//! messages, `request.data` merge order).
//!
//! Python refs (pinned: DRF 3.15.2, Django 4.2.30, CPython 3.12):
//! `rest_framework/negotiation.py` (`select_parser`), `request.py`
//! (`_parse`, `_load_data_and_files`), `parsers.py` (`FormParser`,
//! `MultiPartParser`), `django/http/request.py` (`QueryDict`),
//! `django/http/multipartparser.py`, `django/utils/http.py`
//! (`parse_header_parameters`), `urllib/parse.py` (`parse_qsl`,
//! `unquote`), `urllib`-adjacent `html.unescape` (filename sanitize),
//! `django/core/files/base.py` (`File` dunders).
//!
//! Every behavior below was probed live against Django; the deliberate
//! deviations live here, not scattered through the code:
//! - Filenames: only the 9 XML/legacy HTML entities plus full numeric
//!   refs decode (`html.unescape` subset); other named entities
//!   (`&hellip;` etc.) stay literal. Exotic `filename*` codecs decode as
//!   latin-1. Otherwise-unassigned (`Cn`) code points count as
//!   printable, except the permanently-unassigned noncharacters.
//! - A base64 file part whose alphabet is valid but unpadded trips
//!   Django's stuck-parser guard (HTML 400, `SuspiciousMultipartForm`);
//!   Rust answers the documented file-decode 400 instead. The HTML page
//!   is settings-dependent and cannot be byte-matched.

use serde_json::Value;

/// Which DRF default parser a content-type selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyKind {
    Json,
    Form,
    Multipart,
}

/// DRF `select_parser` over the default parser order (`JSONParser`,
/// `FormParser`, `MultiPartParser` — `negotiation.py:25-33`,
/// `settings.py`): the first parser whose media type matches the request
/// content-type wins. Matching lowercases the base like
/// `parse_header_parameters` and ignores request params (no parser
/// declares any); `*` matches on either side (`_MediaType.match`).
/// `None` is the 415 branch (no parser matched).
pub(crate) fn select_body_kind(content_type: &str) -> Option<BodyKind> {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match base.as_str() {
        "application/json" | "*/*" | "application/*" | "*/json" => Some(BodyKind::Json),
        "application/x-www-form-urlencoded" | "*/x-www-form-urlencoded" => Some(BodyKind::Form),
        "multipart/form-data" | "multipart/*" | "*/form-data" => Some(BodyKind::Multipart),
        _ => None,
    }
}

/// One uploaded file part: the two properties the license endpoints can
/// observe (`str(file)` is the sanitized name, `len(file)` the size in
/// bytes — `django/core/files/base.py`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FormFile {
    pub filename: String,
    pub size: usize,
}

/// One parsed form value: a text field or an uploaded file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FormValue {
    Text(String),
    File(FormFile),
}

/// A merged `request.data` lookup: files win ties in either order
/// (`_load_data_and_files` appends files after fields — probed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BodyField<'a> {
    Missing,
    Json(&'a Value),
    File(&'a FormFile),
}

/// A decoded write body: the JSON value (any shape, as today) or a form
/// object of last-wins text values, plus the file parts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedBody {
    value: Value,
    files: Vec<(String, FormFile)>,
    is_form: bool,
}

impl ParsedBody {
    pub(crate) fn json(value: Value) -> Self {
        Self {
            value,
            files: Vec::new(),
            is_form: false,
        }
    }

    pub(crate) fn form(pairs: Vec<(String, FormValue)>) -> Self {
        let mut map = serde_json::Map::new();
        let mut files: Vec<(String, FormFile)> = Vec::new();
        for (name, item) in pairs {
            match item {
                FormValue::Text(text) => {
                    map.insert(name, Value::String(text));
                }
                FormValue::File(file) => {
                    if let Some(slot) = files.iter_mut().find(|(n, _)| *n == name) {
                        slot.1 = file;
                    } else {
                        files.push((name, file));
                    }
                }
            }
        }
        Self {
            value: Value::Object(map),
            files,
            is_form: true,
        }
    }

    pub(crate) fn value(&self) -> &Value {
        &self.value
    }

    pub(crate) fn object(&self) -> Option<&serde_json::Map<String, Value>> {
        self.value.as_object()
    }

    /// True when the body came from a form/multipart parser: DRF's
    /// HTML-form `get_value` rules apply (`fields.py:407-429`), which
    /// differ from JSON for empty strings on some fields.
    pub(crate) fn is_form(&self) -> bool {
        self.is_form
    }

    /// Merged `request.data.get`: the file part wins when a name has both
    /// a field and a file part, in either order (probed).
    pub(crate) fn field(&self, key: &str) -> BodyField<'_> {
        if let Some((_, file)) = self.files.iter().find(|(n, _)| n == key) {
            return BodyField::File(file);
        }
        match self.value.get(key) {
            Some(found) => BodyField::Json(found),
            None => BodyField::Missing,
        }
    }

    /// Merged `request.data.keys`: text keys first, then file-only names
    /// (`_full_data = data.copy().update(files)`).
    pub(crate) fn keys(&self) -> Vec<&str> {
        let mut out = Vec::new();
        if let Some(map) = self.value.as_object() {
            out.extend(map.keys().map(String::as_str));
        }
        for (name, _) in &self.files {
            if !out.contains(&name.as_str()) {
                out.push(name);
            }
        }
        out
    }
}

/// `FormParser.parse`: `QueryDict(stream.read(), encoding="utf-8")`
/// (`parsers.py:77-84`, `request.py:502-541`): the bytes decode as UTF-8
/// with an iso-8859-1 fallback, then `parse_qsl` with `&` only,
/// blank values kept (a segment without `=` keeps an empty value).
/// A non-empty body with more than `DATA_UPLOAD_MAX_NUMBER_FIELDS`
/// (1000) `&`-separated segments — empties counted, `parse_qsl`'s
/// `1 + qs.count(separator)` — is `TooManyFieldsSent` → generic 500
/// (probed).
pub(crate) fn parse_form_body(body: &[u8]) -> Result<Vec<(String, FormValue)>, BodyFail> {
    let text = match std::str::from_utf8(body) {
        Ok(valid) => valid.to_owned(),
        Err(_) => body.iter().map(|&b| b as char).collect(),
    };
    if text.is_empty() {
        return Ok(Vec::new());
    }
    // `&` is ASCII, so the byte count equals the decoded `str` count.
    if 1 + body.iter().filter(|b| **b == b'&').count() > MAX_FORM_FIELDS {
        return Err(BodyFail::TooMany);
    }
    let mut out = Vec::new();
    for segment in text.split('&') {
        if segment.is_empty() {
            continue;
        }
        let (key, value) = match segment.find('=') {
            Some(index) => (&segment[..index], &segment[index + 1..]),
            None => (segment, ""),
        };
        out.push((unquote_plus(key), FormValue::Text(unquote_plus(value))));
    }
    Ok(out)
}

/// `DATA_UPLOAD_MAX_NUMBER_FIELDS` (Django default 1000; the license
/// settings do not override it).
const MAX_FORM_FIELDS: usize = 1000;

/// `urllib.parse.unquote_plus` with `errors="replace"`: `+` becomes a
/// space first, then ASCII runs percent-decode as UTF-8 with replacement
/// while non-ASCII runs pass through verbatim
/// (`_generate_unquoted_parts`).
fn unquote_plus(text: &str) -> String {
    unquote_encoded(text, true, py_utf8_replace)
}

/// The ASCII-run splitter behind `unquote`/`unquote_plus`: `+` handling
/// first when asked, then each ASCII run percent-decodes and decodes
/// through `decode`, each non-ASCII char passes through.
fn unquote_encoded(text: &str, plus: bool, decode: impl Fn(&[u8]) -> String) -> String {
    let mut out = String::new();
    let mut run: Vec<u8> = Vec::new();
    let flush = |run: &mut Vec<u8>, out: &mut String| {
        if !run.is_empty() {
            out.push_str(&decode(&percent_decode(run)));
            run.clear();
        }
    };
    for mut c in text.chars() {
        if plus && c == '+' {
            c = ' ';
        }
        if c.is_ascii() {
            run.push(c as u8);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// `_unquote_impl`: split on `%`; the first piece is literal, each later
/// piece contributes one byte when it opens with two hex digits, else the
/// `%` and the piece stay literal.
fn percent_decode(ascii: &[u8]) -> Vec<u8> {
    let mut pieces = ascii.split(|b| *b == b'%');
    let mut out = pieces.next().unwrap_or(b"").to_vec();
    for item in pieces {
        if item.len() >= 2 && item[0].is_ascii_hexdigit() && item[1].is_ascii_hexdigit() {
            out.push(hex_value(item[0]) << 4 | hex_value(item[1]));
            out.extend_from_slice(&item[2..]);
        } else {
            out.push(b'%');
            out.extend_from_slice(item);
        }
    }
    out
}

fn hex_value(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        b'a'..=b'f' => digit - b'a' + 10,
        b'A'..=b'F' => digit - b'A' + 10,
        _ => 0,
    }
}

/// CPython `bytes.decode("utf-8", "replace")` exactly: valid sequences
/// decode; an invalid start byte emits U+FFFD and advances one byte; an
/// incomplete sequence at the end emits one U+FFFD for all remaining
/// bytes; a complete-but-invalid sequence (surrogate, overlong,
/// out-of-range) emits U+FFFD for the start byte and reprocesses the
/// rest — so a surrogate is three U+FFFDS and an overlong two (all
/// probed). `String::from_utf8_lossy` differs on the last case (one
/// U+FFFD per sequence), so it cannot be used here.
fn py_utf8_replace(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        let first = bytes[index];
        if first < 0x80 {
            out.push(first as char);
            index += 1;
            continue;
        }
        let width = utf8_sequence_width(first);
        if width == 0 {
            out.push('\u{FFFD}');
            index += 1;
            continue;
        }
        if index + width > bytes.len() {
            out.push('\u{FFFD}');
            break;
        }
        match std::str::from_utf8(&bytes[index..index + width]) {
            Ok(valid) => {
                out.push_str(valid);
                index += width;
            }
            Err(_) => {
                out.push('\u{FFFD}');
                index += 1;
            }
        }
    }
    out
}

/// Expected UTF-8 sequence width from the start byte, or 0 for bytes
/// that can never start a sequence (continuations, `C0`/`C1`, `F5`-`FF`).
fn utf8_sequence_width(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if (0xC2..=0xDF).contains(&first) {
        2
    } else if (0xE0..=0xEF).contains(&first) {
        3
    } else if (0xF0..=0xF4).contains(&first) {
        4
    } else {
        0
    }
}

/// How form/multipart parsing failed: a `ParseError` 400 with the full
/// `"Multipart form parse error - ..."` message, a fatal error that
/// escapes the parser into the generic 500 (an unparseable `filename*`
/// encoding declaration — `ValueError` out of `parse_header_parameters`),
/// or an upload cap exceeded (`TooManyFieldsSent` / `TooManyFilesSent` /
/// `RequestDataTooBig` — the license base view answers each with the
/// generic 500, probed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyFail {
    Parse(String),
    Fatal,
    TooMany,
}

/// `MultiPartParser.parse` (`parsers.py:93-113` over Django's
/// `multipartparser`): field parts as ordered text pairs plus file parts
/// with their sanitized name and size. Django has no closer concept —
/// every `--boundary` delimits and parts after a `--` delimiter still
/// parse (probed); garbage segments drop out.
///
/// `memory_limit` is `DATA_UPLOAD_MAX_MEMORY_SIZE` (`FILE_SIZE_LIMIT`,
/// 5242880 by default): cumulative FIELD bytes (encoded length for
/// base64 parts) plus `len(name) + 2` (chars, not bytes) per named
/// field past it is `RequestDataTooBig`. Past
/// `DATA_UPLOAD_MAX_NUMBER_FIELDS + 2`
/// (1000 + preamble/epilogue) FIELD/RAW segments, or past
/// `DATA_UPLOAD_MAX_NUMBER_FILES` (100) named files, is
/// `TooManyFieldsSent` / `TooManyFilesSent`. All three answer the
/// generic 500 (`BodyFail::TooMany`, probed). The checks run in
/// Django's per-segment order: FIELD/RAW counting first, then the
/// missing-name skip, then memory (fields) / counting (files), so a
/// 101st file with bad base64 500s instead of 400ing.
pub(crate) fn parse_multipart_body(
    content_type: &str,
    body: &[u8],
    memory_limit: i64,
) -> Result<Vec<(String, FormValue)>, BodyFail> {
    let boundary = multipart_boundary(content_type)?;
    let mut out = Vec::new();
    let mut fields = 0usize;
    let mut files = 0usize;
    let mut memory = 0usize;
    for segment in split_segments(body, &boundary) {
        let part = parse_segment(segment);
        if matches!(part.kind, SegKind::Field | SegKind::Raw) {
            fields += 1;
            if fields > MAX_FORM_FIELDS + 2 {
                return Err(BodyFail::TooMany);
            }
        }
        let Some(name) = part.name else { continue };
        match part.kind {
            SegKind::Field => {
                // The name counts in CHARS (`len()` of the decoded
                // `str`), not bytes — `multipartparser.py`.
                memory += part.content.len() + name.chars().count() + 2;
                if memory_limit < 0 || memory as u64 > memory_limit as u64 {
                    return Err(BodyFail::TooMany);
                }
                let bytes = if part.base64 {
                    decode_base64_field(part.content)
                } else {
                    part.content.to_vec()
                };
                out.push((name, FormValue::Text(py_utf8_replace(&bytes))));
            }
            SegKind::File => {
                files += 1;
                if files > MAX_UPLOAD_FILES {
                    return Err(BodyFail::TooMany);
                }
                let size = if part.base64 {
                    decode_base64_file(part.content)?.len()
                } else {
                    part.content.len()
                };
                let Some(clean) = sanitize_filename(&part.filename.unwrap_or_default()) else {
                    continue;
                };
                out.push((
                    name,
                    FormValue::File(FormFile {
                        filename: clean,
                        size,
                    }),
                ));
            }
            // RAW means no disposition header, which means no name, so
            // this arm is unreachable — kept for exhaustiveness.
            SegKind::Raw => {}
        }
    }
    Ok(out)
}

/// `DATA_UPLOAD_MAX_NUMBER_FILES` (Django default 100; the license
/// settings do not override it).
const MAX_UPLOAD_FILES: usize = 100;

/// The Django `MultiPartParser.__init__` checks, in order
/// (`multipartparser.py:56-88`): the raw content-type must start with
/// `multipart/` (case-sensitive) and be ASCII; the boundary comes from
/// `parse_header_parameters` and must match `[ -~]{0,200}[!-~]`
/// (`boundary_re`). Failures echo Django's exact messages (all probed).
fn multipart_boundary(content_type: &str) -> Result<Vec<u8>, BodyFail> {
    if !content_type.starts_with("multipart/") {
        return Err(BodyFail::Parse(format!(
            "Multipart form parse error - Invalid Content-Type: {content_type}"
        )));
    }
    if !content_type.is_ascii() {
        return Err(BodyFail::Parse(format!(
            "Multipart form parse error - Invalid non-ASCII Content-Type in multipart: {content_type}"
        )));
    }
    let (_, params) = parse_header_parameters(content_type).map_err(|()| BodyFail::Fatal)?;
    let boundary = params
        .iter()
        .rev()
        .find(|(name, _)| name == "boundary")
        .map(|(_, value)| value.clone());
    // `force_str(boundary)`: a missing param echoes `None`, a present
    // one echoes verbatim (both probed).
    let shown = boundary.clone().unwrap_or_else(|| "None".to_owned());
    let Some(boundary) = boundary.filter(|b| valid_boundary(b)) else {
        return Err(BodyFail::Parse(format!(
            "Multipart form parse error - Invalid boundary in multipart: {shown}"
        )));
    };
    Ok(boundary.into_bytes())
}

/// `boundary_re.fullmatch`: 1-201 printable ASCII chars, the last one not
/// a space. The content-type is ASCII by the time this runs, so byte
/// checks are exact.
fn valid_boundary(boundary: &str) -> bool {
    let bytes = boundary.as_bytes();
    if bytes.is_empty() || bytes.len() > 201 {
        return false;
    }
    if !(0x21..=0x7E).contains(&bytes[bytes.len() - 1]) {
        return false;
    }
    bytes[..bytes.len() - 1]
        .iter()
        .all(|b| (0x20..=0x7E).contains(b))
}

/// Django `parse_header_parameters` (`django/utils/http.py:422-449`):
/// the lowercased main value plus ordered `(lowercased-name,
/// processed-value)` params; segments without `=` are ignored and later
/// duplicates win (applied by the caller). Values unquote (only when
/// wrapped in `"`) and collapse `\\` then `\"` only. A `name*` param
/// RFC 2231-decodes into `name`; a malformed encoding declaration is the
/// `ValueError` callers handle per site. `Err(())` is that fatal case.
fn parse_header_parameters(line: &str) -> Result<(String, Vec<(String, String)>), ()> {
    let mut segments = split_header_segments(line).into_iter();
    let main = segments.next().unwrap_or_default().to_lowercase();
    let mut params = Vec::new();
    for segment in segments {
        let Some(eq) = segment.find('=') else {
            continue;
        };
        let mut name = segment[..eq].trim().to_lowercase();
        let mut value = segment[eq + 1..].trim().to_owned();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = value[1..value.len() - 1].to_owned();
            value = value.replace("\\\\", "\\").replace("\\\"", "\"");
        }
        if let Some(stripped) = name.strip_suffix('*') {
            name = stripped.to_owned();
            if segment.matches('\'').count() == 2 {
                let mut pieces = value.split('\'');
                let (Some(encoding), Some(_), Some(raw), None) =
                    (pieces.next(), pieces.next(), pieces.next(), pieces.next())
                else {
                    return Err(());
                };
                value = rfc2231_decode(raw, encoding);
            }
        }
        params.push((name, value));
    }
    Ok((main, params))
}

/// Django `_parseparam` (`django/utils/http.py:409-419`): split on `;`,
/// except a `;` preceded by an odd run of non-escaped quotes stays
/// inside the segment (a quoted `boundary="a;b"` survives — probed).
/// Segments come back stripped.
fn split_header_segments(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    loop {
        let mut end = rest.len();
        let mut search_from = 0;
        while let Some(relative) = rest[search_from..].find(';') {
            let candidate = search_from + relative;
            let before = &rest[..candidate];
            let quotes = before.chars().filter(|c| *c == '"').count();
            let escaped = before.matches("\\\"").count();
            if (quotes - escaped).is_multiple_of(2) {
                end = candidate;
                break;
            }
            search_from = candidate + 1;
        }
        out.push(rest[..end].trim().to_owned());
        if end == rest.len() {
            break;
        }
        rest = &rest[end + 1..];
    }
    out
}

/// RFC 2231 `encoding'lang'value`: `unquote(value, encoding=encoding)`
/// — percent-decoding without `+` handling, then the named codec.
/// `utf-8`, `ascii` and `latin-1` (plus common aliases) are exact;
/// anything else decodes latin-1 (documented approximation — real
/// traffic is UTF-8).
fn rfc2231_decode(raw: &str, encoding: &str) -> String {
    let normalized: String = encoding
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect::<String>()
        .to_lowercase();
    match normalized.as_str() {
        "utf8" => unquote_encoded(raw, false, py_utf8_replace),
        "ascii" | "usascii" => unquote_encoded(raw, false, ascii_replace),
        _ => unquote_encoded(raw, false, latin1_decode),
    }
}

fn ascii_replace(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if b < 0x80 { b as char } else { '\u{FFFD}' })
        .collect()
}

fn latin1_decode(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Split the body at every `--boundary` occurrence, Django's
/// `BoundaryIter`/`_find_boundary` exactly: plain substring search with
/// no line anchoring (a mid-line `--boundary` delimits — probed), each
/// match resuming after the separator. The preamble (bytes before the
/// first occurrence) is the first segment and the run after the last
/// occurrence the last: Django parses every segment including those
/// two (a wrong boundary parses the whole body as one part — probed).
/// Every segment except the last loses one trailing `\n` then one
/// `\r` (the CRLF backup); the last runs to exhaustion verbatim —
/// trailing breaks there are the field's own bytes (DRF `CharField`
/// trimming strips them later; `ChoiceField` sees them — probed).
fn split_segments<'a>(body: &'a [u8], boundary: &[u8]) -> Vec<&'a [u8]> {
    let mut delimiter = Vec::with_capacity(boundary.len() + 2);
    delimiter.extend_from_slice(b"--");
    delimiter.extend_from_slice(boundary);
    let mut offsets = Vec::new();
    let mut search = 0;
    while let Some(relative) = find_subslice(&body[search..], &delimiter) {
        let offset = search + relative;
        offsets.push(offset);
        search = offset + delimiter.len();
    }
    let mut segments = Vec::new();
    let mut start = 0;
    for offset in &offsets {
        segments.push(strip_one_break(&body[start..*offset]));
        start = offset + delimiter.len();
    }
    segments.push(&body[start..]);
    segments
}

/// Django's CRLF backup before a delimiter (`_find_boundary`): one `\n`,
/// then one `\r`.
fn strip_one_break(segment: &[u8]) -> &[u8] {
    let segment = segment.strip_suffix(b"\n").unwrap_or(segment);
    segment.strip_suffix(b"\r").unwrap_or(segment)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// One segment's Django type (`parse_boundary_stream`): a field, a
/// file, or raw bytes. The type comes from the headers alone — Django
/// counts and skips before looking at content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegKind {
    Field,
    File,
    Raw,
}

/// One parsed segment: its type, the stripped disposition `name` (if
/// any), the raw filename, the content bytes, and the base64 flag.
/// Value building (base64 decode, filename sanitize) happens in the
/// caller after the cap checks, in Django's order.
struct SegParts<'a> {
    kind: SegKind,
    name: Option<String>,
    filename: Option<String>,
    content: &'a [u8],
    base64: bool,
}

/// One segment to its parts: no header end within the first 1024 bytes
/// (`parse_boundary_stream`) is RAW. The header block splits on `\r\n`
/// pairs; a bare `\n` or `\r` stays inside its line
/// (`multipartparser.py`).
fn parse_segment(segment: &[u8]) -> SegParts<'_> {
    let raw = SegParts {
        kind: SegKind::Raw,
        name: None,
        filename: None,
        content: b"",
        base64: false,
    };
    let window = &segment[..segment.len().min(1024)];
    let Some(end) = find_subslice(window, b"\r\n\r\n") else {
        return raw;
    };
    let (raw_headers, content) = (&segment[..end], &segment[end + 4..]);
    // Each disposition line replaces the previous one whole
    // (`outdict[name] = ...`), so only the LAST one counts — a later
    // line without `filename` demotes a file back to a field.
    let mut disposition: Option<(Option<String>, Option<String>)> = None;
    let mut base64 = false;
    let mut rest = raw_headers;
    let mut lines = Vec::new();
    while let Some(pos) = find_subslice(rest, b"\r\n") {
        lines.push(&rest[..pos]);
        rest = &rest[pos + 2..];
    }
    lines.push(rest);
    for line in lines {
        // Django `line.decode()`s every header line strictly: a
        // non-UTF-8 line is skipped, never lossy-read.
        let Ok(text) = std::str::from_utf8(line) else {
            continue;
        };
        // Lines without a colon are skipped (`ValueError`).
        let Ok((main, params)) = parse_header_parameters(text) else {
            continue;
        };
        let Some((header, value)) = main.split_once(':') else {
            continue;
        };
        // The header name is compared unstripped (`multipartparser.py`).
        if header == "content-disposition" {
            let mut name = None;
            let mut filename = None;
            for (key, item) in params {
                if key == "name" {
                    name = Some(item);
                } else if key == "filename" {
                    filename = Some(item);
                }
            }
            disposition = Some((name, filename));
        } else if header == "content-transfer-encoding" {
            // The main value lowercases in `parse_header_parameters`,
            // so this match is effectively case-insensitive (probed);
            // the last such line wins (`outdict` overwrite).
            base64 = value.trim() == "base64";
        }
    }
    let Some((name, filename)) = disposition else {
        return raw;
    };
    // `disposition["name"].strip()`: ASCII whitespace, empty kept (Django
    // files it under `""` — unobservable on these endpoints, exact here).
    let name = name.map(|n| n.trim_matches(|c: char| c.is_ascii_whitespace()).to_owned());
    // `params.get("filename")` truthiness decides file vs field
    // (`multipartparser.py`): `filename=""` lands in POST (probed).
    let kind = if filename.as_deref().is_some_and(|f| !f.is_empty()) {
        SegKind::File
    } else {
        SegKind::Field
    };
    SegParts {
        kind,
        name,
        filename,
        content,
        base64,
    }
}

/// Base64 field decoding (`multipartparser.py`: `b64decode` with
/// non-alphabet bytes ignored, raw fallback on error).
fn decode_base64_field(content: &[u8]) -> Vec<u8> {
    let filtered: Vec<u8> = content
        .iter()
        .copied()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        .collect();
    binascii_b64decode(&filtered).unwrap_or_else(|()| content.to_vec())
}

/// Base64 file decoding: ASCII whitespace stripped, non-alphabet bytes
/// ignored like `binascii`, hard error on failure
/// (`multipartparser.py`: `"Could not decode base64 data."`). The
/// padding-read-across-parts in the streaming parser is not replicated
/// (documented approximation: such bodies trip Django's stuck-parser
/// guard with a settings-dependent HTML 400 instead).
fn decode_base64_file(content: &[u8]) -> Result<Vec<u8>, BodyFail> {
    let stripped: Vec<u8> = content
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let filtered: Vec<u8> = stripped
        .iter()
        .copied()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
        .collect();
    binascii_b64decode(&filtered).map_err(|()| {
        BodyFail::Parse("Multipart form parse error - Could not decode base64 data.".to_owned())
    })
}

/// `binascii.a2b_base64` over alphabet-filtered input (no strict crate
/// matches it — every rule below was probed against CPython):
/// data chars accumulate into quads (flushed at 4); a `=` pad with 0-1
/// data chars in the quad is skipped; with 2 it skips iff the next byte
/// is data, terminates with one byte on a following pad, and fails at
/// end (`"AA="` is truncated); with 3 it always terminates with two
/// bytes. A trailing short group fails (1 data char fails, 2-3 fail);
/// all-pad input decodes to empty.
fn binascii_b64decode(filtered: &[u8]) -> Result<Vec<u8>, ()> {
    fn sextet(byte: u8) -> u8 {
        match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            _ => 63,
        }
    }
    let mut out = Vec::new();
    let mut acc: Vec<u8> = Vec::new();
    let mut index = 0;
    while index < filtered.len() {
        let byte = filtered[index];
        if byte != b'=' {
            acc.push(sextet(byte));
            if acc.len() == 4 {
                out.push(acc[0] << 2 | acc[1] >> 4);
                out.push(acc[1] << 4 | acc[2] >> 2);
                out.push(acc[2] << 6 | acc[3]);
                acc.clear();
            }
            index += 1;
            continue;
        }
        if acc.len() <= 1 {
            index += 1;
            continue;
        }
        if acc.len() == 2 {
            let followed_by_data = index + 1 < filtered.len() && filtered[index + 1] != b'=';
            if followed_by_data {
                index += 1;
                continue;
            }
            if index + 1 == filtered.len() {
                return Err(());
            }
            out.push(acc[0] << 2 | acc[1] >> 4);
            return Ok(out);
        }
        out.push(acc[0] << 2 | acc[1] >> 4);
        out.push(acc[1] << 4 | acc[2] >> 2);
        return Ok(out);
    }
    if acc.is_empty() {
        Ok(out)
    } else {
        Err(())
    }
}

/// `sanitize_file_name` (`multipartparser.py:382-404`): HTML-unescape,
/// strip to the basename over both separators, drop non-printable
/// chars. `""`, `"."` and `".."` discard the part (`None` — probed).
fn sanitize_filename(raw: &str) -> Option<String> {
    let unescaped = html_unescape_subset(raw);
    let base = unescaped
        .rsplit('/')
        .next()
        .unwrap_or("")
        .rsplit('\\')
        .next()
        .unwrap_or("");
    let clean: String = base.chars().filter(|c| is_printable_py(*c)).collect();
    if clean.is_empty() || clean == "." || clean == ".." {
        None
    } else {
        Some(clean)
    }
}

/// `html.unescape` over the 9 XML/legacy named entities plus full
/// numeric refs (all pinned against CPython, see the unit tests):
/// `&amp; &lt; &gt; &quot; &apos;` and uppercase `&AMP; &LT; &GT;
/// &QUOT;`, each with or without the semicolon except `apos` (bare
/// `&apos` stays literal). Any other named entity (`&hellip;` etc.)
/// stays literal — documented approximation. Resolution is longest
/// prefix like `_replace_charref`, and a resolved-literal still consumes
/// its whole regex match.
fn html_unescape_subset(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '&' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        if let Some((decoded, used)) = numeric_charref(&chars[index..]) {
            out.push_str(&decoded);
            index += used;
            continue;
        }
        // Named run: 1-32 non-delimiter chars plus an optional `;`.
        let mut end = index + 1;
        while end < chars.len()
            && end - (index + 1) < 32
            && !matches!(
                chars[end],
                '\t' | '\n' | '\u{0C}' | ' ' | '<' | '&' | '#' | ';'
            )
        {
            end += 1;
        }
        if end == index + 1 {
            out.push('&');
            index += 1;
            continue;
        }
        let mut used = end - index;
        if end < chars.len() && chars[end] == ';' {
            used += 1;
        }
        let candidate: String = chars[index + 1..index + used].iter().collect();
        match resolve_named_entity(&candidate) {
            Some((decoded, name_len)) => {
                out.push_str(decoded);
                out.push_str(&candidate[name_len..]);
            }
            None => {
                out.push('&');
                out.push_str(&candidate);
            }
        }
        index += used;
    }
    out
}

/// Named entity resolution over the 9-entry subset: the exact candidate
/// first, then longest prefix (lengths `len-1` down to 2, like
/// `_replace_charref`). Returns the decoded char plus the consumed name
/// length; the rest of the candidate stays literal.
fn resolve_named_entity(candidate: &str) -> Option<(&'static str, usize)> {
    if let Some(decoded) = subset_entity(candidate) {
        return Some((decoded, candidate.len()));
    }
    // Longest prefix like CPython; only ASCII prefixes can hit the
    // table, so non-boundary byte positions are skipped, never cut.
    let mut end = candidate.len().saturating_sub(1);
    while end >= 2 {
        if candidate.is_char_boundary(end) {
            if let Some(decoded) = subset_entity(&candidate[..end]) {
                return Some((decoded, end));
            }
        }
        end -= 1;
    }
    None
}

/// The 9 XML/legacy entities, with-`;` and bare (bare `apos` excluded —
/// CPython leaves `&apos` literal; all pinned against CPython).
fn subset_entity(name: &str) -> Option<&'static str> {
    match name {
        "amp;" | "amp" | "AMP;" | "AMP" => Some("&"),
        "lt;" | "lt" | "LT;" | "LT" => Some("<"),
        "gt;" | "gt" | "GT;" | "GT" => Some(">"),
        "quot;" | "quot" | "QUOT;" | "QUOT" => Some("\""),
        "apos;" => Some("'"),
        _ => None,
    }
}

/// Numeric charref at a `&` position: `&#digits;?` or `&#xhex;?`.
/// Returns the decoded text plus the consumed char count.
fn numeric_charref(chars: &[char]) -> Option<(String, usize)> {
    if chars.len() < 2 || chars[0] != '&' || chars[1] != '#' {
        return None;
    }
    let (hex, mut end) = if chars.len() > 2 && (chars[2] == 'x' || chars[2] == 'X') {
        (true, 3)
    } else {
        (false, 2)
    };
    let digits_start = end;
    while end < chars.len()
        && (chars[end].is_ascii_hexdigit() && (hex || chars[end].is_ascii_digit()))
    {
        end += 1;
    }
    if end == digits_start {
        return None;
    }
    let digits: String = chars[digits_start..end].iter().collect();
    let mut used = end;
    if end < chars.len() && chars[end] == ';' {
        used += 1;
    }
    let num = if hex {
        u32::from_str_radix(&digits, 16).unwrap_or(u32::MAX)
    } else {
        digits.parse::<u32>().unwrap_or(u32::MAX)
    };
    Some((resolve_codepoint(num), used))
}

/// `_replace_charref` numeric branch: the `_invalid_charrefs` table
/// first (NUL, CR, Windows-1252 C1), then surrogates/out-of-range to
/// U+FFFD, then `_invalid_codepoints` (controls, noncharacters) removed,
/// else the char itself.
fn resolve_codepoint(num: u32) -> String {
    if let Some(mapped) = invalid_charref(num) {
        return mapped.to_owned();
    }
    if (0xD800..=0xDFFF).contains(&num) || num > 0x10FFFF {
        return "\u{FFFD}".to_owned();
    }
    if is_invalid_codepoint(num) {
        return String::new();
    }
    char::from_u32(num).map_or_else(|| "\u{FFFD}".to_owned(), |c| c.to_string())
}

/// CPython `_invalid_charrefs`: NUL to U+FFFD, CR through, C1 bytes to
/// Windows-1252 (the 5 undefined stay as the control char itself).
fn invalid_charref(num: u32) -> Option<&'static str> {
    match num {
        0 => Some("\u{FFFD}"),
        13 => Some("\r"),
        128 => Some("€"),
        129 => Some("\u{81}"),
        130 => Some("‚"),
        131 => Some("ƒ"),
        132 => Some("„"),
        133 => Some("…"),
        134 => Some("†"),
        135 => Some("‡"),
        136 => Some("ˆ"),
        137 => Some("‰"),
        138 => Some("Š"),
        139 => Some("‹"),
        140 => Some("Œ"),
        141 => Some("\u{8D}"),
        142 => Some("Ž"),
        143 => Some("\u{8F}"),
        144 => Some("\u{90}"),
        145 => Some("‘"),
        146 => Some("’"),
        147 => Some("“"),
        148 => Some("”"),
        149 => Some("•"),
        150 => Some("–"),
        151 => Some("—"),
        152 => Some("˜"),
        153 => Some("™"),
        154 => Some("š"),
        155 => Some("›"),
        156 => Some("œ"),
        157 => Some("\u{9D}"),
        158 => Some("ž"),
        159 => Some("Ÿ"),
        _ => None,
    }
}

/// CPython `_invalid_codepoints` (126 entries, dumped verbatim):
/// controls, DEL, C1, `FDD0`-`FDEF`, and the per-plane `FFFE`/`FFFF`
/// noncharacters — all removed.
fn is_invalid_codepoint(num: u32) -> bool {
    matches!(num, 0x01..=0x08 | 0x0B | 0x0E..=0x1F | 0x7F..=0x9F | 0xFDD0..=0xFDEF)
        || (num <= 0x10FFFF && matches!(num & 0xFFFF, 0xFFFE | 0xFFFF))
}

/// Python `str.isprintable`: everything except `Other`/`Separator`
/// categories, with ASCII space kept. Rust has no category API, so this
/// is control chars, DEL, non-space whitespace (covers `Zl`/`Zp` and
/// every `Zs` space — all are `White_Space`), plus the `Cf` format and
/// `Co` private-use ranges (dumped from `unicodedata`, CPython 3.12)
/// and the permanently-unassigned noncharacters (stable by Unicode
/// guarantee). Other unassigned (`Cn`) code points count as printable
/// here — documented approximation.
fn is_printable_py(c: char) -> bool {
    if c == ' ' {
        return true;
    }
    if c.is_control() || c == '\u{7F}' || c.is_whitespace() {
        return false;
    }
    let n = c as u32;
    if (0xFDD0..=0xFDEF).contains(&n) || matches!(n & 0xFFFF, 0xFFFE | 0xFFFF) {
        return false;
    }
    if CF_RANGES.iter().any(|(lo, hi)| (*lo..=*hi).contains(&n)) {
        return false;
    }
    if CO_RANGES.iter().any(|(lo, hi)| (*lo..=*hi).contains(&n)) {
        return false;
    }
    true
}

/// `Cf` ranges from CPython 3.12 `unicodedata` (21 ranges).
const CF_RANGES: [(u32, u32); 21] = [
    (0xAD, 0xAD),
    (0x600, 0x605),
    (0x61C, 0x61C),
    (0x6DD, 0x6DD),
    (0x70F, 0x70F),
    (0x890, 0x891),
    (0x8E2, 0x8E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
];

/// `Co` ranges from CPython 3.12 `unicodedata` (3 ranges).
const CO_RANGES: [(u32, u32); 3] = [(0xE000, 0xF8FF), (0xF0000, 0xFFFFD), (0x100000, 0x10FFFD)];

#[cfg(test)]
mod tests {
    use super::*;

    fn form_text(pairs: Result<Vec<(String, FormValue)>, BodyFail>) -> Vec<(String, String)> {
        pairs
            .expect("parses")
            .into_iter()
            .map(|(k, v)| match v {
                FormValue::Text(t) => (k, t),
                FormValue::File(_) => panic!("expected text pair"),
            })
            .collect()
    }

    #[test]
    fn routing_matches_select_parser_order() {
        // JSON first (PIDASHCONV-1102's allowlist, unchanged).
        for ct in [
            "application/json",
            "Application/JSON",
            "application/json; charset=utf-8",
            "*/*",
            "application/*",
            "*/json",
        ] {
            assert_eq!(select_body_kind(ct), Some(BodyKind::Json), "{ct}");
        }
        // Then form (exact, case/params, wildcard subtype).
        for ct in [
            "application/x-www-form-urlencoded",
            "Application/X-Www-Form-Urlencoded",
            "application/x-www-form-urlencoded; charset=utf-8",
            "*/x-www-form-urlencoded",
        ] {
            assert_eq!(select_body_kind(ct), Some(BodyKind::Form), "{ct}");
        }
        // Then multipart (exact, case-insensitive match, wildcards).
        for ct in [
            "multipart/form-data; boundary=B",
            "Multipart/Form-Data; boundary=B",
            "multipart/*; boundary=B",
            "*/form-data; boundary=B",
        ] {
            assert_eq!(select_body_kind(ct), Some(BodyKind::Multipart), "{ct}");
        }
        // Nothing else matches: the 415 branch.
        for ct in [
            "",
            "text/plain",
            "text/*",
            "application/xml",
            "*/xml",
            "form-data",
        ] {
            assert_eq!(select_body_kind(ct), None, "{ct}");
        }
        // `application/*` is JSON (first match wins), never form.
        assert_eq!(select_body_kind("application/*"), Some(BodyKind::Json));
    }

    #[test]
    fn form_basics_last_wins_and_plus() {
        assert_eq!(
            form_text(parse_form_body(b"instance_name=Probe")),
            vec![("instance_name".to_owned(), "Probe".to_owned())]
        );
        assert_eq!(
            form_text(parse_form_body(b"a=First&a=Second")),
            vec![
                ("a".to_owned(), "First".to_owned()),
                ("a".to_owned(), "Second".to_owned())
            ]
        );
        assert_eq!(
            form_text(parse_form_body(b"n=a+b%26c%25d")),
            vec![("n".to_owned(), "a b&c%d".to_owned())]
        );
        // `%2B` is a literal plus: `+`-to-space runs before %-decoding.
        assert_eq!(
            form_text(parse_form_body(b"n=%2B")),
            vec![("n".to_owned(), "+".to_owned())]
        );
        // `;` is data, not a separator (probed).
        assert_eq!(
            form_text(parse_form_body(b"n=X;n=Y")),
            vec![("n".to_owned(), "X;n=Y".to_owned())]
        );
    }

    #[test]
    fn form_blank_and_key_edges() {
        // No `=`: kept with an empty value (keep_blank_values).
        assert_eq!(
            form_text(parse_form_body(b"flag&n=X")),
            vec![
                ("flag".to_owned(), String::new()),
                ("n".to_owned(), "X".to_owned())
            ]
        );
        // Empty key kept; empty segments skipped.
        assert_eq!(
            form_text(parse_form_body(b"=v")),
            vec![(String::new(), "v".to_owned())]
        );
        assert_eq!(form_text(parse_form_body(b"&")), Vec::new());
        assert_eq!(
            form_text(parse_form_body(b"=")),
            vec![(String::new(), String::new())]
        );
        // First `=` splits; later ones are data.
        assert_eq!(
            form_text(parse_form_body(b"a=b=c")),
            vec![("a".to_owned(), "b=c".to_owned())]
        );
        // Empty body parses to no pairs.
        assert!(parse_form_body(b"").expect("parses").is_empty());
    }

    #[test]
    fn form_percent_decodes_utf8_with_replacement() {
        // Invalid sequences become U+FFFD (probed).
        assert_eq!(
            form_text(parse_form_body(b"n=%FF")),
            vec![("n".to_owned(), "\u{FFFD}".to_owned())]
        );
        assert_eq!(
            form_text(parse_form_body(b"n=%E9")),
            vec![("n".to_owned(), "\u{FFFD}".to_owned())]
        );
        // Bad escapes stay literal.
        assert_eq!(
            form_text(parse_form_body(b"n=%zz%")),
            vec![("n".to_owned(), "%zz%".to_owned())]
        );
        // Complete multibyte sequences decode.
        assert_eq!(
            form_text(parse_form_body("n=%E2%82%AC".as_bytes())),
            vec![("n".to_owned(), "€".to_owned())]
        );
        // Truncated at the end: one U+FFFD for the rest (probed).
        assert_eq!(
            form_text(parse_form_body(b"n=%E2%82")),
            vec![("n".to_owned(), "\u{FFFD}".to_owned())]
        );
        // Complete-but-invalid: one U+FFFD per byte (probed).
        assert_eq!(
            form_text(parse_form_body(b"n=%ED%A0%80")),
            vec![("n".to_owned(), "\u{FFFD}\u{FFFD}\u{FFFD}".to_owned())]
        );
        assert_eq!(
            form_text(parse_form_body(b"n=%C0%AF")),
            vec![("n".to_owned(), "\u{FFFD}\u{FFFD}".to_owned())]
        );
        assert_eq!(
            form_text(parse_form_body(b"n=%80")),
            vec![("n".to_owned(), "\u{FFFD}".to_owned())]
        );
    }

    #[test]
    fn form_raw_bytes_fall_back_to_latin1() {
        // Invalid UTF-8 bodies decode iso-8859-1 (QueryDict — probed).
        assert_eq!(
            form_text(parse_form_body(b"n=\xff")),
            vec![("n".to_owned(), "ÿ".to_owned())]
        );
    }

    #[test]
    fn merged_lookup_files_win_either_order() {
        let body = ParsedBody::form(vec![
            ("k".to_owned(), FormValue::Text("text".to_owned())),
            (
                "k".to_owned(),
                FormValue::File(FormFile {
                    filename: "f.txt".to_owned(),
                    size: 3,
                }),
            ),
        ]);
        assert!(matches!(body.field("k"), BodyField::File(_)));
        assert_eq!(body.keys(), vec!["k"]);
        let body = ParsedBody::form(vec![
            (
                "k".to_owned(),
                FormValue::File(FormFile {
                    filename: "f.txt".to_owned(),
                    size: 3,
                }),
            ),
            ("k".to_owned(), FormValue::Text("text".to_owned())),
        ]);
        assert!(matches!(body.field("k"), BodyField::File(_)));
        assert!(body.is_form());
        assert!(!ParsedBody::json(Value::Null).is_form());
        assert!(matches!(
            ParsedBody::json(Value::Null).field("k"),
            BodyField::Missing
        ));
    }

    #[test]
    fn boundary_checks_match_django_messages() {
        let mct = |opts: &str| format!("multipart/form-data{opts}");
        // Missing / empty / bad boundaries echo verbatim (probed).
        for (ct, message) in [
            (
                "multipart/form-data",
                "Multipart form parse error - Invalid boundary in multipart: None",
            ),
            (
                "multipart/form-data; boundary=",
                "Multipart form parse error - Invalid boundary in multipart: ",
            ),
        ] {
            assert_eq!(
                multipart_boundary(ct),
                Err(BodyFail::Parse(message.to_owned())),
                "{ct}"
            );
        }
        assert_eq!(
            multipart_boundary("Multipart/Form-Data; boundary=B"),
            Err(BodyFail::Parse(
                "Multipart form parse error - Invalid Content-Type: Multipart/Form-Data; boundary=B"
                    .to_owned()
            ))
        );
        // Param names are case-insensitive; extra params are fine.
        assert!(multipart_boundary("multipart/form-data; Boundary=B").is_ok());
        assert!(multipart_boundary(&mct("; boundary=B; charset=utf-8")).is_ok());
        // Regex: 201 chars ok with non-space end, 202 rejected, trailing
        // space rejected (all probed through the live boundary).
        assert!(multipart_boundary(&mct(&format!("; boundary={}", "x".repeat(201)))).is_ok());
        assert!(multipart_boundary(&mct(&format!("; boundary={}", "x".repeat(202)))).is_err());
        assert!(multipart_boundary(&mct("; boundary=\"abc \"")).is_err());
        // Quoted semicolons survive param splitting (probed).
        assert_eq!(
            multipart_boundary("multipart/form-data; boundary=\"a;b\""),
            Ok(b"a;b".to_vec())
        );
        // Last duplicate wins.
        assert_eq!(
            multipart_boundary("multipart/form-data; boundary=A; boundary=B"),
            Ok(b"B".to_vec())
        );
    }

    fn multipart_body(body: &[u8], boundary: &str) -> Result<Vec<(String, FormValue)>, BodyFail> {
        parse_multipart_body(
            &format!("multipart/form-data; boundary={boundary}"),
            body,
            5_242_880,
        )
    }

    fn part(name: &str, value: &str) -> Vec<u8> {
        format!("--B\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
            .into_bytes()
    }

    #[test]
    fn framing_splits_every_delimiter_occurrence() {
        // Headerless preamble and epilogue drop out; dup fields both
        // surface.
        let body = [
            b"preamble\r\n".to_vec(),
            part("a", "One"),
            part("a", "Two"),
            b"--B--\r\nepilogue".to_vec(),
        ]
        .concat();
        assert_eq!(
            multipart_body(&body, "B"),
            Ok(vec![
                ("a".to_owned(), FormValue::Text("One".to_owned())),
                ("a".to_owned(), FormValue::Text("Two".to_owned())),
            ])
        );
        // No delimiter and bare-LF bodies are empty data (probed).
        assert_eq!(multipart_body(b"not multipart at all", "B"), Ok(vec![]));
        assert_eq!(
            multipart_body(
                b"--B\nContent-Disposition: form-data; name=\"a\"\n\nv\n--B--\n",
                "B"
            ),
            Ok(vec![])
        );
        // Django has no closer concept: parts after `--` parse, junk
        // after a delimiter still splits (all probed).
        let body = [
            part("n", "Before"),
            b"--B--\r\n".to_vec(),
            part("n", "After"),
            b"--B--\r\n".to_vec(),
        ]
        .concat();
        let pairs = multipart_body(&body, "B").expect("parses");
        assert_eq!(
            pairs,
            vec![
                ("n".to_owned(), FormValue::Text("Before".to_owned())),
                ("n".to_owned(), FormValue::Text("After".to_owned())),
            ]
        );
        let body = [
            part("n", "A"),
            b"--Bxyz\r\n".to_vec(),
            part("n", "B"),
            b"--B--\r\n".to_vec(),
        ]
        .concat();
        let pairs = multipart_body(&body, "B").expect("parses");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[1].1, FormValue::Text("B".to_owned()));
        // Bare delimiters at the end of input are no-ops.
        assert_eq!(multipart_body(b"--B", "B"), Ok(vec![]));
        assert_eq!(multipart_body(b"--B\r\n", "B"), Ok(vec![]));
    }

    #[test]
    fn truncated_tail_completes_verbatim() {
        // A part cut off at the end of input still applies, trailing
        // breaks intact: Django reads the last segment to exhaustion
        // (the earlier rstripped reads were DRF `CharField` trimming —
        // `ChoiceField` sees the breaks, probed).
        let body = b"--B\r\nContent-Disposition: form-data; name=\"n\"\r\n\r\nTruncVal\r\n";
        assert_eq!(
            multipart_body(body, "B"),
            Ok(vec![(
                "n".to_owned(),
                FormValue::Text("TruncVal\r\n".to_owned())
            )])
        );
        for (tail, expected) in [
            ("NoCRLF", "NoCRLF"),
            ("DblCRLF\r\n\r\n", "DblCRLF\r\n\r\n"),
            ("v\r\nX\r\n", "v\r\nX\r\n"),
            ("v\n", "v\n"),
            ("v\r", "v\r"),
        ] {
            let body = format!("--B\r\nContent-Disposition: form-data; name=\"n\"\r\n\r\n{tail}");
            assert_eq!(
                multipart_body(body.as_bytes(), "B"),
                Ok(vec![("n".to_owned(), FormValue::Text(expected.to_owned()))]),
                "{tail:?}"
            );
        }
        // Headers cut off at the end of input drop the part (probed).
        let body = b"--B\r\nContent-Disposition: form-data; name=\"n";
        assert_eq!(multipart_body(body, "B"), Ok(vec![]));
    }

    #[test]
    fn preamble_parses_and_midline_delimiter_splits() {
        // A preamble carrying headers parses as the first part (a wrong
        // boundary parses the whole body as one part — probed).
        let body = b"Content-Disposition: form-data; name=\"n\"\r\n\r\nPre";
        assert_eq!(
            multipart_body(body, "WRONG"),
            Ok(vec![("n".to_owned(), FormValue::Text("Pre".to_owned()))])
        );
        // A mid-line `--boundary` delimits: Django's `find` has no
        // line anchoring (probed).
        let body = b"--B\r\nContent-Disposition: form-data; name=\"n\"\r\n\r\nv--Bq\r\nContent-Disposition: form-data; name=\"m\"\r\n\r\nw\r\n--B--\r\n";
        assert_eq!(
            multipart_body(body, "B"),
            Ok(vec![
                ("n".to_owned(), FormValue::Text("v".to_owned())),
                ("m".to_owned(), FormValue::Text("w".to_owned())),
            ])
        );
        // Only the last disposition line counts: a later line without
        // `filename` demotes a file back to a field.
        let body = b"--B\r\nContent-Disposition: form-data; name=\"n\"; filename=\"f.txt\"\r\nContent-Disposition: form-data; name=\"n\"\r\n\r\nv\r\n--B--\r\n";
        assert_eq!(
            multipart_body(body, "B"),
            Ok(vec![("n".to_owned(), FormValue::Text("v".to_owned()))])
        );
    }

    #[test]
    fn upload_caps_match_django_counts() {
        let ct = "multipart/form-data; boundary=B";
        let closer = b"--B--\r\n".to_vec();
        // 1000 field parts + preamble/epilogue RAW = 1002 counted: OK.
        let mut body = (0..1000)
            .map(|i| part(&format!("u{i}"), "v"))
            .collect::<Vec<_>>()
            .concat();
        body.extend_from_slice(&closer);
        assert!(parse_multipart_body(ct, &body, 5_242_880).is_ok());
        // The 1003rd counted segment trips `TooManyFieldsSent`.
        let mut body = (0..1001)
            .map(|i| part(&format!("u{i}"), "v"))
            .collect::<Vec<_>>()
            .concat();
        body.extend_from_slice(&closer);
        assert_eq!(
            parse_multipart_body(ct, &body, 5_242_880),
            Err(BodyFail::TooMany)
        );
        // Files do not count as fields, but the 101st named file trips.
        let file = |i: usize| {
            format!("--B\r\nContent-Disposition: form-data; name=\"f{i}\"; filename=\"g.txt\"\r\n\r\nx\r\n")
                .into_bytes()
        };
        let body = (0..100).map(file).collect::<Vec<_>>().concat();
        assert!(parse_multipart_body(ct, &body, 5_242_880).is_ok());
        let body = (0..101).map(file).collect::<Vec<_>>().concat();
        assert_eq!(
            parse_multipart_body(ct, &body, 5_242_880),
            Err(BodyFail::TooMany)
        );
        // An unnamed file part counts as neither (skipped before the
        // file count), while an unnamed field still counts as a field.
        let unnamed_file =
            b"--B\r\nContent-Disposition: form-data; filename=\"g.txt\"\r\n\r\nx\r\n--B--\r\n";
        assert!(parse_multipart_body(ct, unnamed_file, 5_242_880).is_ok());
        // Memory: FIELD bytes + name + 2 past the limit trips.
        let big = "v".repeat(100);
        let body = format!("--B\r\nContent-Disposition: form-data; name=\"n\"\r\n\r\n{big}");
        assert!(parse_multipart_body(ct, body.as_bytes(), 103).is_ok());
        assert_eq!(
            parse_multipart_body(ct, body.as_bytes(), 102),
            Err(BodyFail::TooMany)
        );
        // The name counts in CHARS (`len()` of the decoded `str`), not
        // bytes: `é` (2 bytes, 1 char) + 7 content bytes + 2 is 10.
        let body = "--B\r\nContent-Disposition: form-data; name=\"é\"\r\n\r\n1234567";
        assert!(parse_multipart_body(ct, body.as_bytes(), 10).is_ok());
        assert_eq!(
            parse_multipart_body(ct, body.as_bytes(), 9),
            Err(BodyFail::TooMany)
        );
        // Form: 1000 segments OK, 1001 (empties counted) trips.
        let ok = (0..1000)
            .map(|i| format!("u{i}=v"))
            .collect::<Vec<_>>()
            .join("&");
        assert!(parse_form_body(ok.as_bytes()).is_ok());
        let over = (0..1001)
            .map(|i| format!("u{i}=v"))
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(parse_form_body(over.as_bytes()), Err(BodyFail::TooMany));
        assert_eq!(
            parse_form_body(format!("&&&{ok}&&&").as_bytes()),
            Err(BodyFail::TooMany)
        );
    }

    #[test]
    fn disposition_edges_match_django() {
        let wrap = |headers: &str, value: &[u8]| {
            [
                format!("--B\r\n{headers}\r\n\r\n").into_bytes(),
                value.to_vec(),
                b"\r\n--B--\r\n".to_vec(),
            ]
            .concat()
        };
        // Empty filename is a field, not a file (probed).
        let body = wrap(
            "Content-Disposition: form-data; name=\"n\"; filename=\"\"",
            b"EMPTYFN",
        );
        assert_eq!(
            multipart_body(&body, "B"),
            Ok(vec![(
                "n".to_owned(),
                FormValue::Text("EMPTYFN".to_owned())
            )])
        );
        // Nameless parts and colon-less lines drop out.
        let body = wrap("Content-Disposition: form-data", b"noname");
        assert_eq!(multipart_body(&body, "B"), Ok(vec![]));
        let body = wrap("just garbage", b"x");
        assert_eq!(multipart_body(&body, "B"), Ok(vec![]));
        // Non-UTF-8 header lines are skipped, never lossy-read.
        let body = [
            b"--B\r\nContent-Disposition: form-data; name=\"n\"\r\nX-Bin: \xff\r\n\r\n".to_vec(),
            b"v".to_vec(),
            b"\r\n--B--\r\n".to_vec(),
        ]
        .concat();
        assert_eq!(
            multipart_body(&body, "B"),
            Ok(vec![("n".to_owned(), FormValue::Text("v".to_owned()))])
        );
        // Last disposition line and last param win.
        let body = wrap(
            "Content-Disposition: form-data; name=\"first\"\r\nContent-Disposition: form-data; name=\"last\"",
            b"v",
        );
        assert_eq!(
            multipart_body(&body, "B"),
            Ok(vec![("last".to_owned(), FormValue::Text("v".to_owned()))])
        );
        // Filenames: paths, entities, dots, controls (all probed).
        for (filename, expected) in [
            ("a/b.txt", Some("b.txt")),
            ("C:\\x\\y.txt", Some("y.txt")),
            ("a&amp;b.txt", Some("a&b.txt")),
            (".", None),
            ("..", None),
            ("a\x00b.txt", Some("ab.txt")),
            ("caf\u{e9}.txt", Some("caf\u{e9}.txt")),
        ] {
            let body = wrap(
                &format!("Content-Disposition: form-data; name=\"k\"; filename=\"{filename}\""),
                b"data",
            );
            let pairs = multipart_body(&body, "B").expect("parses");
            match expected {
                Some(name) => assert_eq!(
                    pairs,
                    vec![(
                        "k".to_owned(),
                        FormValue::File(FormFile {
                            filename: name.to_owned(),
                            size: 4
                        })
                    )],
                    "{filename}"
                ),
                None => assert_eq!(pairs, vec![], "{filename}"),
            }
        }
        // `filename*` RFC 2231-decodes; raw values pass through; last of
        // `filename`/`filename*` wins either way (all probed).
        for (disposition, expected) in [
            ("filename*=utf-8''%E2%82%AC.txt", "€.txt"),
            ("filename*=raw.txt", "raw.txt"),
            (
                "filename=\"plain.txt\"; filename*=utf-8''star.txt",
                "star.txt",
            ),
            (
                "filename*=utf-8''star.txt; filename=\"plain.txt\"",
                "plain.txt",
            ),
        ] {
            let body = wrap(
                &format!("Content-Disposition: form-data; name=\"k\"; {disposition}"),
                b"data",
            );
            let pairs = multipart_body(&body, "B").expect("parses");
            assert_eq!(
                pairs,
                vec![(
                    "k".to_owned(),
                    FormValue::File(FormFile {
                        filename: expected.to_owned(),
                        size: 4
                    })
                )],
                "{disposition}"
            );
        }
    }

    #[test]
    fn transfer_encoding_matches_django() {
        let wrap = |cte: Option<&str>, value: &[u8]| {
            let mut headers = "Content-Disposition: form-data; name=\"n\"".to_owned();
            if let Some(encoding) = cte {
                headers.push_str(&format!("\r\nContent-Transfer-Encoding: {encoding}"));
            }
            [
                format!("--B\r\n{headers}\r\n\r\n").into_bytes(),
                value.to_vec(),
                b"\r\n--B--\r\n".to_vec(),
            ]
            .concat()
        };
        // Field decoding is case-insensitive via the lowered main value
        // (probed: `TXA=` is `Mp`).
        for cte in ["base64", "Base64", "BASE64"] {
            assert_eq!(
                multipart_body(&wrap(Some(cte), b"TXA="), "B"),
                Ok(vec![("n".to_owned(), FormValue::Text("Mp".to_owned()))]),
                "{cte}"
            );
        }
        // Bad field base64 falls back to raw.
        assert_eq!(
            multipart_body(&wrap(Some("base64"), b"abc"), "B"),
            Ok(vec![("n".to_owned(), FormValue::Text("abc".to_owned()))])
        );
        // Files decode to their real size; undecodable files 400 (probed).
        let wrap_file = |cte: Option<&str>, value: &[u8]| {
            let mut headers =
                "Content-Disposition: form-data; name=\"n\"; filename=\"f.b64\"".to_owned();
            if let Some(encoding) = cte {
                headers.push_str(&format!("\r\nContent-Transfer-Encoding: {encoding}"));
            }
            [
                format!("--B\r\n{headers}\r\n\r\n").into_bytes(),
                value.to_vec(),
                b"\r\n--B--\r\n".to_vec(),
            ]
            .concat()
        };
        let pairs = multipart_body(&wrap_file(Some("base64"), &[b'Q'; 100]), "B").expect("parses");
        assert_eq!(
            pairs,
            vec![(
                "n".to_owned(),
                FormValue::File(FormFile {
                    filename: "f.b64".to_owned(),
                    size: 75
                })
            )]
        );
        assert_eq!(
            multipart_body(&wrap_file(Some("base64"), b"abc"), "B"),
            Err(BodyFail::Parse(
                "Multipart form parse error - Could not decode base64 data.".to_owned()
            ))
        );
    }

    #[test]
    fn base64_padding_matches_binascii() {
        // `binascii.a2b_base64` leniency (every case probed against
        // CPython — the strict crate decoder disagrees on all of these):
        // pads at 0-1 data chars skip; at 2 data chars a pad skips iff
        // data follows immediately, terminates on a following pad, and
        // fails at end; at 3 data chars a pad always terminates; a
        // trailing short group fails (1 data) or fails (2-3 data).
        let field_ok: &[(&[u8], &[u8])] = &[
            (b"====", b""),
            (b"=", b""),
            (b"=======", b""),
            (b"QUJD", b"ABC"),
            (b"QUJD====", b"ABC"),
            (b"====AAAA", b"\x00\x00\x00"),
            (b"AA==AA==", b"\x00"),
            (b"AA=A==", b"\x00\x00"),
            (b"AAA=A", b"\x00\x00"),
            (b"AA=AA", b"\x00\x00\x00"),
            (b"AA=AA==", b"\x00\x00\x00"),
            (b"AAAAAAA=", b"\x00\x00\x00\x00\x00"),
            (b"AA==A", b"\x00"),
            (b"A=A=A=", b"\x00\x00"),
        ];
        for (raw, want) in field_ok {
            assert_eq!(&decode_base64_field(raw), want, "field {raw:?}");
            assert_eq!(
                decode_base64_file(raw).expect("file decodes").as_slice(),
                *want,
                "file {raw:?}"
            );
        }
        // Failures: fields fall back to raw, files 400.
        for raw in [
            b"A===".as_slice(),
            b"==AA".as_slice(),
            b"=A==".as_slice(),
            b"AA=A".as_slice(),
            b"A=A=".as_slice(),
            b"=AAA".as_slice(),
            b"A==A".as_slice(),
            b"AA=".as_slice(),
            b"A=".as_slice(),
            b"A".as_slice(),
            b"AA".as_slice(),
            b"AAA".as_slice(),
            b"QUJDQUI".as_slice(),
            b"QUJDQ".as_slice(),
            b"AAAAAA=".as_slice(),
            b"AAAA=AAA".as_slice(),
            b"A=B=C".as_slice(),
            b"AB=C".as_slice(),
        ] {
            assert_eq!(decode_base64_field(raw), raw, "field {raw:?}");
            assert!(decode_base64_file(raw).is_err(), "file {raw:?} 400s");
        }
    }

    #[test]
    fn html_unescape_subset_matches_cpython_battery() {
        // Pinned against CPython `html.unescape` (see the workpad
        // battery); the `None` cases are the documented approximation
        // (other named entities stay literal here).
        for (input, expected) in [
            ("&amp;", "&"),
            ("&amp", "&"),
            ("&lt;", "<"),
            ("&gt", ">"),
            ("&quot;", "\""),
            ("&apos;", "'"),
            ("&apos", "&apos"),
            ("&AMP;", "&"),
            ("&AMP", "&"),
            ("&LT", "<"),
            ("&GT", ">"),
            ("&QUOT", "\""),
            ("&apos", "&apos"),
            ("&Amp;", "&Amp;"),
            ("&unknown;", "&unknown;"),
            ("&;", "&;"),
            ("&", "&"),
            ("a&b", "a&b"),
            ("&#65;", "A"),
            ("&#65", "A"),
            ("&#x41;", "A"),
            ("&#X41", "A"),
            ("&#0;", "\u{FFFD}"),
            ("&#xD800;", "\u{FFFD}"),
            ("&#x110000;", "\u{FFFD}"),
            ("&#99999999999;", "\u{FFFD}"),
            ("&#;", "&#;"),
            ("&#x;", "&#x;"),
            ("&#xZZ;", "&#xZZ;"),
            ("&#12ab;", "\u{c}ab;"),
            ("&ampamp;", "&amp;"),
            ("&&amp;;", "&&;"),
            ("&ampx", "&x"),
            ("&#9;", "\t"),
            ("&#13;", "\r"),
            ("&#128;", "€"),
            ("&#129;", "\u{81}"),
            ("&#159;", "Ÿ"),
            ("&#xAD;", "\u{AD}"),
            ("&#1;", ""),
            ("&#xFFFE;", ""),
        ] {
            assert_eq!(html_unescape_subset(input), expected, "{input}");
        }
        // Documented approximation: other named entities stay literal.
        for input in ["&hellip;", "&hellip", "&copy", "&notit;", "&nbsp"] {
            assert_eq!(html_unescape_subset(input), input, "{input}");
        }
        // A resolved-literal still consumes its whole regex match.
        let long = format!("&{};", "a".repeat(40));
        assert_eq!(html_unescape_subset(&long), long);
    }

    #[test]
    fn printable_matches_isprintable_spots() {
        // Pinned against `unicodedata`/`str.isprintable` (see workpad).
        for (c, expected) in [
            (' ', true),
            ('A', true),
            ('é', true),
            ('\t', false),
            ('\n', false),
            ('\u{0}', false),
            ('\u{7F}', false),
            ('\u{80}', false),
            ('\u{A0}', false),
            ('\u{1680}', false),
            ('\u{2003}', false),
            ('\u{2028}', false),
            ('\u{3000}', false),
            ('\u{AD}', false),
            ('\u{200B}', false),
            ('\u{FEFF}', false),
            ('\u{61C}', false),
            ('\u{E000}', false),
            ('\u{10FFFD}', false),
            ('\u{FDD0}', false),
            ('\u{FFFE}', false),
            ('\u{115F}', true),
            ('\u{17B4}', true),
            ('\u{3164}', true),
            ('\u{FFA0}', true),
        ] {
            assert_eq!(is_printable_py(c), expected, "U+{:04X}", c as u32);
        }
    }
}
