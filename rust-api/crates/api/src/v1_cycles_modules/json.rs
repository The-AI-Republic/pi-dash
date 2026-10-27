//! JSON request layer (PIDASHCONV-1555, rule 13 tier-3 cleanup).
//!
//! Every handler port parses request bodies through this module. It used to
//! be a hand-written CPython `json` scanner (PIDASHCONV-626); it now parses
//! with `serde_json` and keeps only tier 1/2 behaviour identical:
//!
//! * Any parse failure is a 400 with the DRF envelope
//!   `{"detail": "JSON parse error - <message>"}` (callers add the prefix;
//!   the message is serde's now, not CPython's).
//! * A zero-length body is `{}` (DRF's content-length short-circuit).
//! * A trailing incomplete UTF-8 sequence is silently dropped on the direct
//!   byte paths (the codecs `StreamReader` never flushes its tail);
//!   negotiated text already dropped it in `body.rs`.
//! * Duplicate keys are last-wins at first-seen positions (serde's
//!   `preserve_order` maps match CPython `dict` here).
//!
//! Tier 3, allowed to differ from CPython (listed in the PR): error texts,
//! `NaN`/`Infinity` strict-constant texts (still 400s, serde's words), lone
//! surrogates (now a 400 instead of accepted), the nesting-depth cap
//! (serde's recursion limit → 400 instead of 9939 → 500), and the int-digit
//! limit (unlimited now instead of 4300 → 400).
//!
//! Values come out as [`JVal`]: numbers keep their literal text plus the
//! grammatical int-vs-float bit (via `serde_json::Number`, with the crate's
//! `arbitrary_precision` feature), and strings ([`JStr`]) are clean text.
//! The surrogate validators in the coerce layers are dead now (no producer
//! emits surrogates since PIDASHCONV-1552); their `has_surrogate` /
//! `first_surrogate` arms stay as always-false shims so callers compile
//! unchanged.

use std::fmt::Write as _;

/// Prefix of every DRF `ParseError` detail (`rest_framework/parsers.py`).
pub const JSON_PARSE_PREFIX: &str = "JSON parse error - ";
/// The wsgiref plain-500 body (exact bytes, note the double space). Dead
/// since surrogates became a 400, kept so the renderer-crash arms in
/// `cycle`/`module` compile unchanged.
pub const PLAIN_CRASH_BODY: &str = "A server error occurred.  Please contact the administrator.";
/// Content type of the wsgiref plain 500 (exact, no charset parameter).
/// Dead since surrogates became a 400, kept for caller compat.
pub const PLAIN_CRASH_CONTENT_TYPE: &str = "text/plain";

// ---------------------------------------------------------------------------
// JStr: clean strings (no lone surrogates since PIDASHCONV-1555)
// ---------------------------------------------------------------------------

/// A JSON string value: clean text. Surrogate shims below always report
/// absent (the parser 400s lone surrogates; form spans are always empty
/// since PIDASHCONV-1552).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JStr {
    text: String,
}

impl JStr {
    /// Empty string.
    pub fn new() -> Self {
        Self::default()
    }

    /// A string known to be surrogate-free.
    pub fn from_clean(text: String) -> Self {
        Self { text }
    }

    /// A string slice (always surrogate-free).
    pub fn from_text(text: &str) -> Self {
        Self::from_clean(text.to_owned())
    }

    /// Decoded text plus lone-surrogate spans. Spans are always empty since
    /// PIDASHCONV-1552; they are ignored (the JSON `*_spans` parsers 400
    /// first when spans are non-empty).
    pub fn from_dirty(text: &str, _surr: &[(usize, u16)]) -> Self {
        Self::from_clean(text.to_owned())
    }

    /// Python `len()`: one per char.
    pub fn len_chars(&self) -> usize {
        self.text.chars().count()
    }

    /// Whether the string is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Whether any unit is a lone surrogate. Always false now.
    pub fn has_surrogate(&self) -> bool {
        false
    }

    /// The first surrogate value. Always `None` now.
    pub fn first_surrogate(&self) -> Option<u16> {
        None
    }

    /// Whether any unit is NUL (the `ProhibitNullCharactersValidator` arm).
    pub fn contains_nul(&self) -> bool {
        self.text.contains('\0')
    }

    /// Exact equality against clean text.
    pub fn eq_str(&self, other: &str) -> bool {
        self.text == other
    }

    /// The clean text. Always `Some` now.
    pub fn to_clean_string(&self) -> Option<String> {
        Some(self.text.clone())
    }

    /// Best-effort text for the task-publish boundary. Identical to the
    /// clean text now (no surrogates to replace).
    pub fn to_lossy_string(&self) -> String {
        self.text.clone()
    }

    /// Python `str.strip()` over Rust's `char::is_whitespace`.
    pub fn trim(&self) -> JStr {
        Self::from_clean(
            self.text
                .trim_matches(|ch: char| ch.is_whitespace())
                .to_owned(),
        )
    }

    /// Python `in` for clean needles.
    pub fn contains_str(&self, needle: &str) -> bool {
        self.text.contains(needle)
    }

    /// One single-char string per char.
    pub fn singletons(&self) -> Vec<JStr> {
        self.text
            .chars()
            .map(|ch| Self::from_text(&ch.to_string()))
            .collect()
    }

    /// Whether the text holds a single-quote character.
    fn has_single_quote(&self) -> bool {
        self.text.contains('\'')
    }

    /// Whether the text holds a double-quote character.
    fn has_double_quote(&self) -> bool {
        self.text.contains('"')
    }

    /// Python `repr()` for a string nested in an echo: single quotes
    /// unless the text holds `'` but no `"` (then double quotes);
    /// backslash, the active quote and `\n`/`\r`/`\t` take short escapes;
    /// other controls take `\xXX`/`\uXXXX`/`\UXXXXXXXX` (lowercase, by
    /// size); everything else is verbatim. ASCII-identical to CPython;
    /// non-ASCII printability edge cases are tier 3 (the old generated
    /// table is gone).
    pub fn py_quoted_into(&self, out: &mut String) {
        let double = self.has_single_quote() && !self.has_double_quote();
        out.push(if double { '"' } else { '\'' });
        for ch in self.text.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '\'' if !double => out.push_str("\\'"),
                '"' if double => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                quote @ ('\'' | '"') => out.push(quote),
                ch if ch.is_control() || ch == '\u{7f}' => {
                    let cp = ch as u32;
                    if cp < 0x100 {
                        let _ = write!(out, "\\x{cp:02x}");
                    } else if cp < 0x10000 {
                        let _ = write!(out, "\\u{cp:04x}");
                    } else {
                        let _ = write!(out, "\\U{cp:08x}");
                    }
                }
                ch => out.push(ch),
            }
        }
        out.push(if double { '"' } else { '\'' });
    }
}

// ---------------------------------------------------------------------------
// JNum: numbers with literal text + int-vs-float
// ---------------------------------------------------------------------------

/// A JSON number literal: the verbatim text plus whether the grammar made
/// it a float (fraction or exponent present — `5e3` is a float, `5` is an
/// int, exactly like serde's `arbitrary_precision`). Floats parse
/// saturating (`1e400` is inf, `1e-400` is 0.0), like CPython's `float()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JNum {
    text: String,
    is_float: bool,
}

impl JNum {
    /// An integer literal (grammar-validated by the parser).
    pub fn int(text: String) -> Self {
        Self {
            text,
            is_float: false,
        }
    }

    /// A float literal (grammar-validated by the parser).
    pub fn float(text: String) -> Self {
        Self {
            text,
            is_float: true,
        }
    }

    /// Whether the grammar made this a float.
    pub fn is_float(&self) -> bool {
        self.is_float
    }

    /// The verbatim literal text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The `f64` value (saturating, like CPython's `float()`). The parser
    /// only builds grammar-valid literals, so this never fails.
    pub fn as_f64(&self) -> f64 {
        self.text
            .parse::<f64>()
            .expect("float literal validated by JSON number grammar")
    }

    /// The `i64` value, if the literal fits (float texts never parse).
    pub fn as_i64(&self) -> Option<i64> {
        self.text.parse::<i64>().ok()
    }

    /// The `u64` value, if the literal fits (float texts never parse).
    pub fn as_u64(&self) -> Option<u64> {
        self.text.parse::<u64>().ok()
    }

    /// The `u128` value for `UUID(int=...)` folds (ints in range only).
    pub fn to_u128(&self) -> Option<u128> {
        if self.is_float {
            return None;
        }
        self.text.parse::<u128>().ok()
    }

    /// Python truthiness: ints are falsy only for zero (`0`/`-0` — the
    /// grammar admits no other all-zero spellings); floats compare
    /// against `0.0` (so `-0.0` and underflows are falsy, inf truthy).
    pub fn is_zero(&self) -> bool {
        if self.is_float {
            self.as_f64() == 0.0
        } else {
            self.text.bytes().all(|b| b == b'0' || b == b'-')
        }
    }

    /// Python `str()`: ints verbatim (unbounded — the grammar admits no
    /// non-canonical spellings except `-0`, which `int()` folds to `0`),
    /// floats via `paginator::py_float_str`.
    pub fn py_string(&self) -> String {
        if self.is_float {
            crate::paginator::py_float_str(self.as_f64())
        } else if self.text == "-0" {
            "0".to_owned()
        } else {
            self.text.clone()
        }
    }
}

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum JVal {
    Null,
    Bool(bool),
    Num(JNum),
    Str(JStr),
    Array(Vec<JVal>),
    Object(JObject),
}

/// An insertion-ordered JSON object with last-wins duplicate keys at
/// first-seen positions (matching serde's `preserve_order` maps).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JObject {
    entries: Vec<(JStr, JVal)>,
}

impl JObject {
    /// Empty object.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the object has no members.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Member lookup by clean name.
    pub fn get(&self, key: &str) -> Option<&JVal> {
        self.entries
            .iter()
            .find(|(name, _)| name.eq_str(key))
            .map(|(_, value)| value)
    }

    /// Whether a member name is present (Python `in` on dicts).
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Insert a member: last value wins, first-seen position kept.
    pub fn insert(&mut self, key: JStr, value: JVal) {
        if let Some(slot) = self.entries.iter_mut().find(|(name, _)| *name == key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    /// Iterate members in first-seen order.
    pub fn iter(&self) -> impl Iterator<Item = &(JStr, JVal)> {
        self.entries.iter()
    }
}

impl JVal {
    /// Whether the value is an object.
    pub fn is_object(&self) -> bool {
        matches!(self, JVal::Object(_))
    }

    /// Move the object out of an owned value (`None` for anything else).
    pub fn into_object(self) -> Option<JObject> {
        match self {
            JVal::Object(map) => Some(map),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Parser: serde_json with tier-2 wrappers
// ---------------------------------------------------------------------------

/// A body-parse failure: serde's error text (the `ParseError` suffix), or
/// the depth cap. `Recursion` is dead now (serde's limit 400s instead of
/// Django's JSON 500); it stays so callers compile unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonFail {
    Message(String),
    Recursion,
}

/// `request.data` exactly as DRF's `Request._parse` produces it: a
/// zero-length body short-circuits to `{}` (the content-length check),
/// anything else goes through [`parse_request_bytes`].
pub fn parse_request_data(raw: &[u8]) -> Result<JVal, JsonFail> {
    parse_request_data_spans(raw, &[])
}

/// [`parse_request_data`] with charset surrogate spans. Spans are always
/// empty since PIDASHCONV-1552; a non-empty span list is a 400 (lone
/// surrogates are tier-3 rejections now).
pub fn parse_request_data_spans(raw: &[u8], surr: &[(usize, u16)]) -> Result<JVal, JsonFail> {
    if !surr.is_empty() {
        return Err(JsonFail::Message("lone surrogate".to_owned()));
    }
    if raw.is_empty() {
        return Ok(JVal::Object(JObject::new()));
    }
    parse_request_bytes_spans(raw, surr)
}

/// Parse request bytes: the UTF-8 codec first (a codec failure wins over
/// any syntax error, with CPython's text for the tier-2 nine charsets),
/// then the JSON text via serde.
pub fn parse_request_bytes(raw: &[u8]) -> Result<JVal, JsonFail> {
    parse_request_bytes_spans(raw, &[])
}

/// [`parse_request_bytes`] with charset surrogate spans (see
/// [`parse_request_data_spans`]).
pub fn parse_request_bytes_spans(raw: &[u8], surr: &[(usize, u16)]) -> Result<JVal, JsonFail> {
    if !surr.is_empty() {
        return Err(JsonFail::Message("lone surrogate".to_owned()));
    }
    let text = match std::str::from_utf8(raw) {
        Ok(text) => text,
        Err(_) => match incomplete_tail_start(raw) {
            // A trailing incomplete sequence is silently dropped (the
            // codecs `StreamReader` never flushes its tail).
            Some(start) => {
                std::str::from_utf8(&raw[..start]).expect("scanned prefix is valid UTF-8")
            }
            None => return Err(JsonFail::Message(utf8_decode_detail(raw))),
        },
    };
    parse_json_text(text)
}

/// Start of a trailing incomplete UTF-8 sequence, mirroring the codecs
/// incremental decoder: continuation-bit patterns buffer, with eager
/// second-byte range checks for `E0`/`F0`/`F4` — but none for `ED` (even
/// `\xed\xa0`, which could never complete, buffers; verified live). Any
/// hard violation (a bare continuation, an impossible lead, a failed
/// eager check, a non-continuation mid-sequence) defeats the tail-drop.
fn incomplete_tail_start(raw: &[u8]) -> Option<usize> {
    let mut need = 0u8;
    let mut seq_start = 0usize;
    let mut lead = 0u8;
    for (index, byte) in raw.iter().enumerate() {
        if need > 0 {
            if byte & 0xC0 == 0x80 {
                // Eager second-byte ranges (overlong/range doomedness is
                // decided before the sequence completes).
                if index == seq_start + 1
                    && ((lead == 0xE0 && *byte < 0xA0)
                        || (lead == 0xF0 && *byte < 0x90)
                        || (lead == 0xF4 && *byte > 0x8F))
                {
                    return None;
                }
                need -= 1;
                // A completed sequence range-checks immediately (an
                // overlong or surrogate prefix is a hard error even when
                // an incomplete tail follows it).
                if need == 0 && std::str::from_utf8(&raw[seq_start..=index]).is_err() {
                    return None;
                }
            } else {
                return None;
            }
            continue;
        }
        match byte {
            0x00..=0x7F => {}
            0xC2..=0xDF => {
                need = 1;
                seq_start = index;
                lead = *byte;
            }
            0xE0..=0xEF => {
                need = 2;
                seq_start = index;
                lead = *byte;
            }
            0xF0..=0xF4 => {
                need = 3;
                seq_start = index;
                lead = *byte;
            }
            _ => return None,
        }
    }
    if need > 0 {
        Some(seq_start)
    } else {
        None
    }
}

/// Parse decoded JSON text via serde (see [`parse_request_bytes`]).
pub fn parse_json_text(text: &str) -> Result<JVal, JsonFail> {
    parse_json_text_spans(text, &[])
}

/// [`parse_json_text`] with charset surrogate spans (see
/// [`parse_request_data_spans`]).
pub fn parse_json_text_spans(text: &str, surr: &[(usize, u16)]) -> Result<JVal, JsonFail> {
    if !surr.is_empty() {
        return Err(JsonFail::Message("lone surrogate".to_owned()));
    }
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => Ok(from_serde(value)),
        Err(error) => Err(JsonFail::Message(error.to_string())),
    }
}

/// Convert a serde value to a [`JVal`], keeping number literal text plus
/// the grammatical int-vs-float bit. Recursion depth is bounded by
/// serde's own recursion limit, so a plain recursive walk is safe.
fn from_serde(value: serde_json::Value) -> JVal {
    match value {
        serde_json::Value::Null => JVal::Null,
        serde_json::Value::Bool(flag) => JVal::Bool(flag),
        serde_json::Value::Number(number) => {
            let text = number.to_string();
            if number.is_f64() {
                JVal::Num(JNum::float(text))
            } else {
                JVal::Num(JNum::int(text))
            }
        }
        serde_json::Value::String(text) => JVal::Str(JStr::from_clean(text)),
        serde_json::Value::Array(items) => JVal::Array(items.into_iter().map(from_serde).collect()),
        serde_json::Value::Object(map) => {
            let mut object = JObject::new();
            for (name, member) in map {
                object.insert(JStr::from_clean(name), from_serde(member));
            }
            JVal::Object(object)
        }
    }
}

// ---------------------------------------------------------------------------
// Positions and codec errors (tier-2 nine charsets)
// ---------------------------------------------------------------------------

/// CPython's `UnicodeDecodeError` text for the first bad sequence
/// (`codecs.getreader("utf-8")`, strict): the lead byte selects the
/// reason, the valid-continuation run selects the byte/bytes form.
/// Shared with the body layer (PIDASHCONV-627), which needs the same
/// text for charset-decoded request bodies.
pub(crate) fn utf8_decode_detail(raw: &[u8]) -> String {
    let start = std::str::from_utf8(raw)
        .expect_err("invalid UTF-8 checked")
        .valid_up_to();
    let lead = raw[start];
    let expected: Option<usize> = match lead {
        0xC2..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
    };
    let Some(expected) = expected else {
        // Stray continuation, overlong C0/C1, or F5-FF lead.
        return format!(
            "'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: invalid start byte"
        );
    };
    let mut run = 1;
    while run < expected && start + run < raw.len() && (0x80..=0xBF).contains(&raw[start + run]) {
        run += 1;
    }
    // The second byte has range checks (overlong E0/F0, surrogate ED,
    // above-maximum F4): out of range fails at the lead even when
    // truncated (`\xed\xa0` + EOF → invalid continuation, not end of
    // data).
    if run >= 2 {
        let second = raw[start + 1];
        let in_range = match lead {
            0xE0 => (0xA0..=0xBF).contains(&second),
            0xED => (0x80..=0x9F).contains(&second),
            0xF0 => (0x90..=0xBF).contains(&second),
            0xF4 => (0x80..=0x8F).contains(&second),
            _ => true,
        };
        if !in_range {
            return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: invalid continuation byte");
        }
    }
    if run == expected {
        // Full-length but range-invalid (overlong E0/F0, surrogate ED,
        // above-maximum F4): reported at the lead byte.
        return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: invalid continuation byte");
    }
    if start + run == raw.len() {
        if run == 1 {
            return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: unexpected end of data");
        }
        return format!(
            "'utf-8' codec can't decode bytes in position {start}-{}: unexpected end of data",
            start + run - 1
        );
    }
    if run == 1 {
        return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: invalid continuation byte");
    }
    format!(
        "'utf-8' codec can't decode bytes in position {start}-{}: invalid continuation byte",
        start + run - 1
    )
}

// ---------------------------------------------------------------------------
// Python str(): recursive, no depth cap (serde bounds depth)
// ---------------------------------------------------------------------------

/// `str()` rendering failure. Dead now (values never nest past serde's
/// recursion limit); it stays so callers compile unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrDepthFail;

/// Python `str()` over a parsed value: strings verbatim, bools as
/// `True`/`False`, null as `None`, numbers via [`JNum::py_string`],
/// containers recursively with Python separators (`, `, `: `) and
/// repr-quoted strings.
pub fn py_str(value: &JVal) -> Result<JStr, StrDepthFail> {
    let mut out = String::new();
    render_py_str(value, &mut out);
    Ok(JStr::from_clean(out))
}

/// Recursive `str()` renderer (see [`py_str`]).
fn render_py_str(value: &JVal, out: &mut String) {
    match value {
        JVal::Null => out.push_str("None"),
        JVal::Bool(true) => out.push_str("True"),
        JVal::Bool(false) => out.push_str("False"),
        JVal::Num(number) => out.push_str(&number.py_string()),
        JVal::Str(text) => out.push_str(&text.to_lossy_string()),
        JVal::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                if let JVal::Str(text) = item {
                    text.py_quoted_into(out);
                } else {
                    render_py_str(item, out);
                }
            }
            out.push(']');
        }
        JVal::Object(object) => {
            out.push('{');
            for (index, (name, member)) in object.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                name.py_quoted_into(out);
                out.push_str(": ");
                if let JVal::Str(text) = member {
                    text.py_quoted_into(out);
                } else {
                    render_py_str(member, out);
                }
            }
            out.push('}');
        }
    }
}

// ---------------------------------------------------------------------------
// Publish boundary: JVal back to serde
// ---------------------------------------------------------------------------

/// Convert a parsed value to a serde value for the jobs-crate publish
/// APIs (which take serde types): numbers keep their literal text
/// (arbitrary-precision), containers recurse.
pub fn to_serde_publish(value: &JVal) -> serde_json::Value {
    match value {
        JVal::Null => serde_json::Value::Null,
        JVal::Bool(flag) => serde_json::Value::Bool(*flag),
        JVal::Num(number) => serde_json::from_str::<serde_json::Value>(number.text())
            .expect("parser-validated number literal"),
        JVal::Str(text) => serde_json::Value::String(text.to_lossy_string()),
        JVal::Array(items) => {
            serde_json::Value::Array(items.iter().map(to_serde_publish).collect())
        }
        JVal::Object(object) => serde_json::Value::Object(to_serde_publish_map(object)),
    }
}

/// Convert a parsed object to a serde map for `model_created_job` /
/// `model_updated_job` (see [`to_serde_publish`]).
pub fn to_serde_publish_map(object: &JObject) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::with_capacity(object.iter().count());
    for (name, member) in object.iter() {
        map.insert(name.to_lossy_string(), to_serde_publish(member));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_data_empty_is_object() {
        assert!(matches!(
            &parse_request_data(b""),
            Ok(JVal::Object(map)) if map.is_empty()
        ));
        // The bytes path has no empty shortcut: decoded-empty is a 400.
        assert!(matches!(
            parse_request_bytes(b""),
            Err(JsonFail::Message(_))
        ));
    }

    #[test]
    fn trailing_incomplete_utf8_drops() {
        assert!(matches!(
            parse_request_data(b"{\"a\":1}\xc3"),
            Ok(JVal::Object(_))
        ));
        // Impossible bytes still fail, even at end of input.
        assert!(matches!(
            parse_request_data(b"\xf5"),
            Err(JsonFail::Message(_))
        ));
        assert!(matches!(
            parse_request_data(b"\xe0\x80"),
            Err(JsonFail::Message(_))
        ));
    }

    #[test]
    fn duplicate_keys_last_win_first_position() {
        let value = parse_request_bytes(br#"{"b": 0, "a": 1, "a": 2}"#).expect("dup keys");
        let JVal::Object(object) = &value else {
            panic!("object");
        };
        let keys: Vec<String> = object
            .iter()
            .map(|(name, _)| name.to_clean_string().unwrap())
            .collect();
        assert_eq!(keys, vec!["b".to_owned(), "a".to_owned()]);
        assert!(matches!(
            object.get("a"),
            Some(JVal::Num(number)) if number.text() == "2"
        ));
    }

    #[test]
    fn malformed_json_is_serde_message() {
        for raw in [b"{bad}" as &[u8], b"{\"a\":}", b"   ", b"{\"a\":1,}"] {
            match parse_request_bytes(raw) {
                Err(JsonFail::Message(detail)) => {
                    assert!(!detail.is_empty(), "{raw:?}");
                }
                other => panic!("{raw:?}: expected Message, got {other:?}"),
            }
        }
    }

    #[test]
    fn tier3_strict_surrogate_depth_are_400s() {
        // NaN / Infinity: 400 (serde text, not CPython's strict text).
        assert!(matches!(
            parse_request_bytes(b"[NaN]"),
            Err(JsonFail::Message(_))
        ));
        // Lone surrogates: 400 now (were accepted).
        assert!(matches!(
            parse_request_bytes(b"\"\\ud800\""),
            Err(JsonFail::Message(_))
        ));
        // Deep nesting past serde's recursion limit: 400 (was 500 past
        // 9939, parseable below it).
        let deep = format!("{}1{}", "[".repeat(1000), "]".repeat(1000));
        assert!(matches!(
            parse_request_bytes(deep.as_bytes()),
            Err(JsonFail::Message(_))
        ));
        // Non-empty spans: 400.
        assert!(matches!(
            parse_request_data_spans(b"{}", &[(0, 0xD800)]),
            Err(JsonFail::Message(_))
        ));
    }

    #[test]
    fn tier3_long_ints_parse() {
        // 5000-digit ints were `Exceeds the limit` 400s; arbitrary
        // precision keeps them now.
        let wide = format!("{{\"a\": {}}}", "9".repeat(5000));
        let value = parse_request_bytes(wide.as_bytes()).expect("long int parses");
        let JVal::Object(object) = &value else {
            panic!("object");
        };
        assert!(matches!(
            object.get("a"),
            Some(JVal::Num(number)) if !number.is_float() && number.text().len() == 5000
        ));
    }

    #[test]
    fn numbers_keep_text_and_float_bit() {
        let value = parse_request_bytes(br#"{"i": 5, "f": 5.0, "e": 5e3}"#).expect("numbers");
        let JVal::Object(object) = &value else {
            panic!("object");
        };
        let int = object.get("i").expect("i");
        let JVal::Num(int) = int else { panic!("int") };
        assert!(!int.is_float());
        assert_eq!(int.text(), "5");
        assert_eq!(int.as_i64(), Some(5));
        let float = object.get("f").expect("f");
        let JVal::Num(float) = float else {
            panic!("float")
        };
        assert!(float.is_float());
        assert_eq!(float.py_string(), "5.0");
        let exp = object.get("e").expect("e");
        let JVal::Num(exp) = exp else { panic!("exp") };
        assert!(exp.is_float());
        // Saturating floats and -0 folding still hold.
        assert_eq!(JNum::float("1e400".to_owned()).py_string(), "inf");
        assert_eq!(JNum::int("-0".to_owned()).py_string(), "0");
    }

    #[test]
    fn py_str_ascii_matches_python() {
        let value = parse_request_bytes(br#"{"a": 1, "b": "x"}"#).expect("object");
        let rendered = py_str(&value).expect("renders");
        assert_eq!(
            rendered.to_clean_string().as_deref(),
            Some("{'a': 1, 'b': 'x'}")
        );
        let value = parse_request_bytes(br#"[1, "a'b", null, true]"#).expect("array");
        let rendered = py_str(&value).expect("renders");
        assert_eq!(
            rendered.to_clean_string().as_deref(),
            Some("[1, \"a'b\", None, True]")
        );
    }

    #[test]
    fn publish_roundtrip_keeps_numbers() {
        let value =
            parse_request_bytes(br#"{"big": 123456789012345678901234567890}"#).expect("big int");
        let back = to_serde_publish(&value);
        assert_eq!(
            back.get("big").and_then(|n| n.as_str()),
            None,
            "numbers are Number, not String"
        );
        assert_eq!(back.to_string(), "{\"big\":123456789012345678901234567890}");
    }

    #[test]
    fn surrogate_shims_are_absent() {
        let text = JStr::from_dirty("a\u{FFFD}b", &[(1, 0xD800)]);
        assert!(!text.has_surrogate());
        assert_eq!(text.first_surrogate(), None);
        assert_eq!(text.to_clean_string().as_deref(), Some("a\u{FFFD}b"));
    }
}
