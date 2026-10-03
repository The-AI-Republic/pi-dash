//! CPython-compatible JSON request layer (PIDASHCONV-626).
//!
//! Every handler port parses request bodies through this module instead of
//! `serde_json` directly, because serde's acceptance envelope is smaller
//! than the one Django really uses (DRF `JSONParser` over CPython `json`
//! with `STRICT_JSON`, the default — `pi_dash` does not override it):
//!
//! * `NaN`/`Infinity`/`-Infinity` in value position are strict-constant
//!   400s (`Out of range float values are not JSON compliant: 'NaN'`),
//!   not acceptances and not generic syntax errors. The strict error wins
//!   exactly when the constant is first in scan order.
//! * Lone `\uD800-\uDFFF` surrogates are ACCEPTED (strings carry them;
//!   `CharField` answers its surrogate validator, top-level echoes crash
//!   the DRF renderer into wsgiref's plain 500).
//! * Nesting parses to a total container depth of 9939 (the 9940th nested
//!   open `RecursionError`s to Django's JSON 500); `str()` of a value
//!   deeper than 9937 does the same. Both measured live, stable across
//!   runs and identical on all four cycle write paths (2026-10-01).
//!   Error texts near the cap carry the C scanner's margins, also measured
//!   live per error family: value-dispatch failures surface to depth 9938,
//!   string/object/delimiter/strict failures to 9935 — past that the
//!   budget check fires first (`RecursionError`, the JSON 500).
//! * Integer literals over 4300 digits are `Exceeds the limit ...` 400s
//!   (`sys.get_int_max_str_digits` default); float literals are exempt.
//! * A zero-length body is `{}` (DRF's content-length short-circuit), and
//!   a trailing incomplete UTF-8 sequence is silently dropped (the codecs
//!   `StreamReader` never flushes its tail) — `{"a":1}\xc3` parses as
//!   `{"a":1}`. Impossible bytes still fail, wherever they sit.
//!
//! The parser below is a single-pass iterative recursive descent over the
//! decoded text, so every error — syntax, strict-constant, int-limit,
//! depth-cap — fires in true scan order with byte-exact CPython text
//! (differentially fuzzed against the strict oracle; see
//! `json_parse_cpython_parity` in [`super::cycle`]).
//!
//! Values come out as [`JVal`]: numbers keep their literal text (int vs
//! float is grammatical, like serde's `arbitrary_precision`), and strings
//! ([`JStr`]) preserve lone surrogates as units so echoes, validators and
//! the renderer-crash rule all see exactly what CPython saw.
//!
//! Shared by all handler ports: later domains adopt this module rather
//! than copying it (its first consumer is the D-20 cycle handlers).

use std::fmt::Write as _;

/// Prefix of every DRF `ParseError` detail (`rest_framework/parsers.py`).
pub const JSON_PARSE_PREFIX: &str = "JSON parse error - ";
/// Deepest total unclosed-container nesting the parser admits. Measured
/// live against the Django oracle (2026-10-01, CPython 3.12.3, runserver
/// thread): total depth 9939 parses, 9940 `RecursionError`s — stable
/// across runs, identical for lists/dicts/mixed input on all four cycle
/// write paths. Past the cap the request is Django's JSON 500.
pub const MAX_CONTAINER_DEPTH: usize = 9939;
/// Deepest container nesting `str()` renders before Django's JSON 500.
/// Measured live (2026-10-01): a 9937-deep field value echoes, a
/// 9938-deep one `RecursionError`s — stable across runs, uniform over
/// lists/dicts/mixed input.
pub const MAX_STR_DEPTH: usize = 9937;
/// Longest integer literal CPython accepts (`sys.get_int_max_str_digits`
/// default 4300; longer is an `Exceeds the limit ...` 400). Applies to
/// int tokens only — float literals of any width parse.
pub const MAX_INT_DIGITS: usize = 4300;
/// The wsgiref plain-500 body (exact bytes, note the double space).
/// Reached when a top-level surrogate echo crashes DRF's renderer: the
/// first `UnicodeEncodeError` fires while rendering the 400, the DEBUG
/// traceback page then fails to encode the same surrogate, and the fault
/// escapes to runserver's wsgiref layer, whose hardcoded `error_body`
/// answers with `text/plain`.
pub const PLAIN_CRASH_BODY: &str = "A server error occurred.  Please contact the administrator.";
/// Content type of the wsgiref plain 500 (exact, no charset parameter).
pub const PLAIN_CRASH_CONTENT_TYPE: &str = "text/plain";

// ---------------------------------------------------------------------------
// JStr: strings that can carry lone surrogates
// ---------------------------------------------------------------------------

/// One decoded string unit: a Unicode scalar value, or a lone UTF-16
/// surrogate (`\uD800-\uDFFF` without its pair) that CPython accepts and
/// Rust `str` cannot hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrUnit {
    Ch(char),
    Sur(u16),
}

/// A JSON string value, exactly as CPython decoded it: scalar values plus
/// any lone surrogates. Length, trimming and comparison follow Python
/// `str` semantics (a surrogate is one char, never whitespace, never
/// equal to any `&str`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JStr {
    units: Vec<StrUnit>,
}

impl JStr {
    /// Empty string.
    pub fn new() -> Self {
        Self::default()
    }

    /// A string known to be surrogate-free.
    pub fn from_clean(text: String) -> Self {
        Self {
            units: text.chars().map(StrUnit::Ch).collect(),
        }
    }

    /// A string slice (always surrogate-free).
    pub fn from_text(text: &str) -> Self {
        Self::from_clean(text.to_owned())
    }

    /// Decoded text plus lone-surrogate spans (exotic form/multipart
    /// charsets, PIDASHCONV-693): each span swaps its U+FFFD placeholder
    /// (byte offset, surrogate value) for a `Sur` unit.
    pub fn from_dirty(text: &str, surr: &[(usize, u16)]) -> Self {
        if surr.is_empty() {
            return Self::from_clean(text.to_owned());
        }
        let mut out = Self::new();
        let bytes = text.as_bytes();
        let mut pos = 0;
        let mut idx = 0;
        while pos < bytes.len() {
            // Stale spans (no placeholder here — unreachable from the
            // decoders, whose pairs stay aligned) drop instead of
            // stalling the later ones.
            while idx < surr.len() && surr[idx].0 < pos {
                idx += 1;
            }
            if idx < surr.len()
                && surr[idx].0 == pos
                && bytes.get(pos..pos + 3) == Some(b"\xef\xbf\xbd")
            {
                out.push_surrogate(surr[idx].1);
                idx += 1;
                pos += 3;
                continue;
            }
            let ch = text[pos..].chars().next().expect("char boundary");
            out.push_char(ch);
            pos += ch.len_utf8();
        }
        out
    }

    /// Append one scalar value (used by the parser).
    pub fn push_char(&mut self, ch: char) {
        self.units.push(StrUnit::Ch(ch));
    }

    /// Append one lone surrogate (used by the parser).
    pub fn push_surrogate(&mut self, unit: u16) {
        debug_assert!((0xD800..0xE000).contains(&unit));
        self.units.push(StrUnit::Sur(unit));
    }

    /// Append another string's units.
    pub fn push_jstr(&mut self, other: &JStr) {
        self.units.extend_from_slice(&other.units);
    }

    /// Python `len()`: one per unit (surrogates count, astral pairs are
    /// already combined into one `char` by the parser).
    pub fn len_chars(&self) -> usize {
        self.units.len()
    }

    /// Whether the string is empty.
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Whether any unit is a lone surrogate.
    pub fn has_surrogate(&self) -> bool {
        self.units
            .iter()
            .any(|unit| matches!(unit, StrUnit::Sur(_)))
    }

    /// The first surrogate value, if any (the `U+XXXX` echo of DRF's
    /// `ProhibitSurrogateCharactersValidator` reports the first).
    pub fn first_surrogate(&self) -> Option<u16> {
        self.units.iter().find_map(|unit| match unit {
            StrUnit::Sur(value) => Some(*value),
            StrUnit::Ch(_) => None,
        })
    }

    /// Whether any unit is NUL (the `ProhibitNullCharactersValidator` arm).
    pub fn contains_nul(&self) -> bool {
        self.units
            .iter()
            .any(|unit| matches!(unit, StrUnit::Ch('\0')))
    }

    /// Exact equality against clean text (a dirty string never equals).
    pub fn eq_str(&self, other: &str) -> bool {
        if self.has_surrogate() {
            return false;
        }
        let mut index = 0;
        for unit in &self.units {
            let StrUnit::Ch(ch) = unit else {
                return false;
            };
            let Some(next) = other[index..].chars().next() else {
                return false;
            };
            if *ch != next {
                return false;
            }
            index += next.len_utf8();
        }
        index == other.len()
    }

    /// The clean text, or `None` when a surrogate is present.
    pub fn to_clean_string(&self) -> Option<String> {
        let mut out = String::new();
        for unit in &self.units {
            match unit {
                StrUnit::Ch(ch) => out.push(*ch),
                StrUnit::Sur(_) => return None,
            }
        }
        Some(out)
    }

    /// Best-effort text for the task-publish boundary (jobs-crate APIs
    /// take serde types, which cannot hold surrogates): surrogates become
    /// U+FFFD. Post-validation payloads only ever carry surrogates inside
    /// ignored unknown fields, so this is worker-side best effort, never
    /// an API-visible answer.
    pub fn to_lossy_string(&self) -> String {
        let mut out = String::new();
        for unit in &self.units {
            match unit {
                StrUnit::Ch(ch) => out.push(*ch),
                StrUnit::Sur(_) => out.push('\u{FFFD}'),
            }
        }
        out
    }

    /// Python `str.strip()` over Rust's `char::is_whitespace` (exactly what
    /// the port's `coerce_char` trim did for clean strings): surrogates
    /// are never whitespace and stop the trim.
    pub fn trim(&self) -> JStr {
        let mut start = 0;
        while start < self.units.len() {
            match self.units[start] {
                StrUnit::Ch(ch) if ch.is_whitespace() => start += 1,
                _ => break,
            }
        }
        let mut end = self.units.len();
        while end > start {
            match self.units[end - 1] {
                StrUnit::Ch(ch) if ch.is_whitespace() => end -= 1,
                _ => break,
            }
        }
        JStr {
            units: self.units[start..end].to_vec(),
        }
    }

    /// Python `in` for clean needles (the PATCH completed gate and the add
    /// path's substring check): a surrogate unit never matches.
    pub fn contains_str(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        if !self.has_surrogate() {
            return self
                .to_clean_string()
                .is_some_and(|text| text.contains(needle));
        }
        let target: Vec<char> = needle.chars().collect();
        if target.len() > self.units.len() {
            return false;
        }
        for window in self.units.windows(target.len()) {
            let mut hit = true;
            for (unit, want) in window.iter().zip(target.iter()) {
                match unit {
                    StrUnit::Ch(ch) if ch == want => {}
                    _ => {
                        hit = false;
                        break;
                    }
                }
            }
            if hit {
                return true;
            }
        }
        false
    }

    /// One single-unit string per unit (the add path iterates a raw string
    /// into per-char candidates; a surrogate is one char).
    pub fn singletons(&self) -> Vec<JStr> {
        self.units
            .iter()
            .map(|unit| JStr { units: vec![*unit] })
            .collect()
    }

    /// Whether the units hold a single-quote character.
    fn has_single_quote(&self) -> bool {
        self.units
            .iter()
            .any(|unit| matches!(unit, StrUnit::Ch('\'')))
    }

    /// Whether the units hold a double-quote character.
    fn has_double_quote(&self) -> bool {
        self.units
            .iter()
            .any(|unit| matches!(unit, StrUnit::Ch('"')))
    }

    /// The JSON-quoted rendering for response bodies: byte-identical to
    /// `serde_json::to_string` on clean text (each clean run is escaped by
    /// serde itself — escaping is context-free, so run-splitting is
    /// exact), with lone surrogates as lowercase `\udXXX` like CPython's
    /// `json.dumps`.
    pub fn json_quoted(&self) -> String {
        let mut out = String::with_capacity(self.units.len() + 2);
        out.push('"');
        let mut run = String::new();
        let flush = |run: &mut String, out: &mut String| {
            if !run.is_empty() {
                let escaped = serde_json::to_string(run).expect("json string run");
                out.push_str(&escaped[1..escaped.len() - 1]);
                run.clear();
            }
        };
        for unit in &self.units {
            match unit {
                StrUnit::Ch(ch) => run.push(*ch),
                StrUnit::Sur(value) => {
                    flush(&mut run, &mut out);
                    let _ = write!(out, "\\u{value:04x}");
                }
            }
        }
        flush(&mut run, &mut out);
        out.push('"');
        out
    }

    /// Python `repr()` for a string nested in an echo: single quotes
    /// unless the text holds `'` but no `"` (then double quotes);
    /// backslash, the active quote and `\n`/`\r`/`\t` take short escapes;
    /// other non-printables take `\xXX`/`\uXXXX`/`\UXXXXXXXX` (lowercase,
    /// by size); lone surrogates take lowercase `\uXXXX`. Printability is
    /// the generated [`REPR_VERBATIM`] table (exhaustive over
    /// `0x0-0x10FFFF` against the oracle build's `repr()`).
    pub fn py_quoted_into(&self, out: &mut String) {
        let double = self.has_single_quote() && !self.has_double_quote();
        out.push(if double { '"' } else { '\'' });
        for unit in &self.units {
            match unit {
                StrUnit::Sur(value) => {
                    let _ = write!(out, "\\u{value:04x}");
                }
                StrUnit::Ch('\\') => out.push_str("\\\\"),
                StrUnit::Ch('\'') if !double => out.push_str("\\'"),
                StrUnit::Ch('"') if double => out.push_str("\\\""),
                StrUnit::Ch('\n') => out.push_str("\\n"),
                StrUnit::Ch('\r') => out.push_str("\\r"),
                StrUnit::Ch('\t') => out.push_str("\\t"),
                // The inactive quote falls through its guarded arm above
                // and stays verbatim (the table marks both quotes `Q`).
                StrUnit::Ch(quote @ ('\'' | '"')) => out.push(*quote),
                StrUnit::Ch(ch) => {
                    if repr_is_verbatim(*ch as u32) {
                        out.push(*ch);
                    } else if (*ch as u32) < 0x100 {
                        let _ = write!(out, "\\x{:02x}", *ch as u32);
                    } else if (*ch as u32) < 0x10000 {
                        let _ = write!(out, "\\u{:04x}", *ch as u32);
                    } else {
                        let _ = write!(out, "\\U{:08x}", *ch as u32);
                    }
                }
            }
        }
        out.push(if double { '"' } else { '\'' });
    }
}

/// Code points Python `repr()` renders verbatim inside a quoted string
/// (generated: exhaustive `repr()` over `0x0-0x10FFFF` on the oracle build,
/// CPython 3.12.3 / Unicode 15.0, 2026-10-01; sorted, non-overlapping).
/// Everything else (outside `\n`/`\r`/`\t`) escapes as `\xXX` below
/// `0x100`, `\uXXXX` below `0x10000`, `\UXXXXXXXX` above.
const REPR_VERBATIM: &[(u32, u32)] = &[
    (0x000020, 0x000021),
    (0x000023, 0x000026),
    (0x000028, 0x00005B),
    (0x00005D, 0x00007E),
    (0x0000A1, 0x0000AC),
    (0x0000AE, 0x000377),
    (0x00037A, 0x00037F),
    (0x000384, 0x00038A),
    (0x00038C, 0x00038C),
    (0x00038E, 0x0003A1),
    (0x0003A3, 0x00052F),
    (0x000531, 0x000556),
    (0x000559, 0x00058A),
    (0x00058D, 0x00058F),
    (0x000591, 0x0005C7),
    (0x0005D0, 0x0005EA),
    (0x0005EF, 0x0005F4),
    (0x000606, 0x00061B),
    (0x00061D, 0x0006DC),
    (0x0006DE, 0x00070D),
    (0x000710, 0x00074A),
    (0x00074D, 0x0007B1),
    (0x0007C0, 0x0007FA),
    (0x0007FD, 0x00082D),
    (0x000830, 0x00083E),
    (0x000840, 0x00085B),
    (0x00085E, 0x00085E),
    (0x000860, 0x00086A),
    (0x000870, 0x00088E),
    (0x000898, 0x0008E1),
    (0x0008E3, 0x000983),
    (0x000985, 0x00098C),
    (0x00098F, 0x000990),
    (0x000993, 0x0009A8),
    (0x0009AA, 0x0009B0),
    (0x0009B2, 0x0009B2),
    (0x0009B6, 0x0009B9),
    (0x0009BC, 0x0009C4),
    (0x0009C7, 0x0009C8),
    (0x0009CB, 0x0009CE),
    (0x0009D7, 0x0009D7),
    (0x0009DC, 0x0009DD),
    (0x0009DF, 0x0009E3),
    (0x0009E6, 0x0009FE),
    (0x000A01, 0x000A03),
    (0x000A05, 0x000A0A),
    (0x000A0F, 0x000A10),
    (0x000A13, 0x000A28),
    (0x000A2A, 0x000A30),
    (0x000A32, 0x000A33),
    (0x000A35, 0x000A36),
    (0x000A38, 0x000A39),
    (0x000A3C, 0x000A3C),
    (0x000A3E, 0x000A42),
    (0x000A47, 0x000A48),
    (0x000A4B, 0x000A4D),
    (0x000A51, 0x000A51),
    (0x000A59, 0x000A5C),
    (0x000A5E, 0x000A5E),
    (0x000A66, 0x000A76),
    (0x000A81, 0x000A83),
    (0x000A85, 0x000A8D),
    (0x000A8F, 0x000A91),
    (0x000A93, 0x000AA8),
    (0x000AAA, 0x000AB0),
    (0x000AB2, 0x000AB3),
    (0x000AB5, 0x000AB9),
    (0x000ABC, 0x000AC5),
    (0x000AC7, 0x000AC9),
    (0x000ACB, 0x000ACD),
    (0x000AD0, 0x000AD0),
    (0x000AE0, 0x000AE3),
    (0x000AE6, 0x000AF1),
    (0x000AF9, 0x000AFF),
    (0x000B01, 0x000B03),
    (0x000B05, 0x000B0C),
    (0x000B0F, 0x000B10),
    (0x000B13, 0x000B28),
    (0x000B2A, 0x000B30),
    (0x000B32, 0x000B33),
    (0x000B35, 0x000B39),
    (0x000B3C, 0x000B44),
    (0x000B47, 0x000B48),
    (0x000B4B, 0x000B4D),
    (0x000B55, 0x000B57),
    (0x000B5C, 0x000B5D),
    (0x000B5F, 0x000B63),
    (0x000B66, 0x000B77),
    (0x000B82, 0x000B83),
    (0x000B85, 0x000B8A),
    (0x000B8E, 0x000B90),
    (0x000B92, 0x000B95),
    (0x000B99, 0x000B9A),
    (0x000B9C, 0x000B9C),
    (0x000B9E, 0x000B9F),
    (0x000BA3, 0x000BA4),
    (0x000BA8, 0x000BAA),
    (0x000BAE, 0x000BB9),
    (0x000BBE, 0x000BC2),
    (0x000BC6, 0x000BC8),
    (0x000BCA, 0x000BCD),
    (0x000BD0, 0x000BD0),
    (0x000BD7, 0x000BD7),
    (0x000BE6, 0x000BFA),
    (0x000C00, 0x000C0C),
    (0x000C0E, 0x000C10),
    (0x000C12, 0x000C28),
    (0x000C2A, 0x000C39),
    (0x000C3C, 0x000C44),
    (0x000C46, 0x000C48),
    (0x000C4A, 0x000C4D),
    (0x000C55, 0x000C56),
    (0x000C58, 0x000C5A),
    (0x000C5D, 0x000C5D),
    (0x000C60, 0x000C63),
    (0x000C66, 0x000C6F),
    (0x000C77, 0x000C8C),
    (0x000C8E, 0x000C90),
    (0x000C92, 0x000CA8),
    (0x000CAA, 0x000CB3),
    (0x000CB5, 0x000CB9),
    (0x000CBC, 0x000CC4),
    (0x000CC6, 0x000CC8),
    (0x000CCA, 0x000CCD),
    (0x000CD5, 0x000CD6),
    (0x000CDD, 0x000CDE),
    (0x000CE0, 0x000CE3),
    (0x000CE6, 0x000CEF),
    (0x000CF1, 0x000CF3),
    (0x000D00, 0x000D0C),
    (0x000D0E, 0x000D10),
    (0x000D12, 0x000D44),
    (0x000D46, 0x000D48),
    (0x000D4A, 0x000D4F),
    (0x000D54, 0x000D63),
    (0x000D66, 0x000D7F),
    (0x000D81, 0x000D83),
    (0x000D85, 0x000D96),
    (0x000D9A, 0x000DB1),
    (0x000DB3, 0x000DBB),
    (0x000DBD, 0x000DBD),
    (0x000DC0, 0x000DC6),
    (0x000DCA, 0x000DCA),
    (0x000DCF, 0x000DD4),
    (0x000DD6, 0x000DD6),
    (0x000DD8, 0x000DDF),
    (0x000DE6, 0x000DEF),
    (0x000DF2, 0x000DF4),
    (0x000E01, 0x000E3A),
    (0x000E3F, 0x000E5B),
    (0x000E81, 0x000E82),
    (0x000E84, 0x000E84),
    (0x000E86, 0x000E8A),
    (0x000E8C, 0x000EA3),
    (0x000EA5, 0x000EA5),
    (0x000EA7, 0x000EBD),
    (0x000EC0, 0x000EC4),
    (0x000EC6, 0x000EC6),
    (0x000EC8, 0x000ECE),
    (0x000ED0, 0x000ED9),
    (0x000EDC, 0x000EDF),
    (0x000F00, 0x000F47),
    (0x000F49, 0x000F6C),
    (0x000F71, 0x000F97),
    (0x000F99, 0x000FBC),
    (0x000FBE, 0x000FCC),
    (0x000FCE, 0x000FDA),
    (0x001000, 0x0010C5),
    (0x0010C7, 0x0010C7),
    (0x0010CD, 0x0010CD),
    (0x0010D0, 0x001248),
    (0x00124A, 0x00124D),
    (0x001250, 0x001256),
    (0x001258, 0x001258),
    (0x00125A, 0x00125D),
    (0x001260, 0x001288),
    (0x00128A, 0x00128D),
    (0x001290, 0x0012B0),
    (0x0012B2, 0x0012B5),
    (0x0012B8, 0x0012BE),
    (0x0012C0, 0x0012C0),
    (0x0012C2, 0x0012C5),
    (0x0012C8, 0x0012D6),
    (0x0012D8, 0x001310),
    (0x001312, 0x001315),
    (0x001318, 0x00135A),
    (0x00135D, 0x00137C),
    (0x001380, 0x001399),
    (0x0013A0, 0x0013F5),
    (0x0013F8, 0x0013FD),
    (0x001400, 0x00167F),
    (0x001681, 0x00169C),
    (0x0016A0, 0x0016F8),
    (0x001700, 0x001715),
    (0x00171F, 0x001736),
    (0x001740, 0x001753),
    (0x001760, 0x00176C),
    (0x00176E, 0x001770),
    (0x001772, 0x001773),
    (0x001780, 0x0017DD),
    (0x0017E0, 0x0017E9),
    (0x0017F0, 0x0017F9),
    (0x001800, 0x00180D),
    (0x00180F, 0x001819),
    (0x001820, 0x001878),
    (0x001880, 0x0018AA),
    (0x0018B0, 0x0018F5),
    (0x001900, 0x00191E),
    (0x001920, 0x00192B),
    (0x001930, 0x00193B),
    (0x001940, 0x001940),
    (0x001944, 0x00196D),
    (0x001970, 0x001974),
    (0x001980, 0x0019AB),
    (0x0019B0, 0x0019C9),
    (0x0019D0, 0x0019DA),
    (0x0019DE, 0x001A1B),
    (0x001A1E, 0x001A5E),
    (0x001A60, 0x001A7C),
    (0x001A7F, 0x001A89),
    (0x001A90, 0x001A99),
    (0x001AA0, 0x001AAD),
    (0x001AB0, 0x001ACE),
    (0x001B00, 0x001B4C),
    (0x001B50, 0x001B7E),
    (0x001B80, 0x001BF3),
    (0x001BFC, 0x001C37),
    (0x001C3B, 0x001C49),
    (0x001C4D, 0x001C88),
    (0x001C90, 0x001CBA),
    (0x001CBD, 0x001CC7),
    (0x001CD0, 0x001CFA),
    (0x001D00, 0x001F15),
    (0x001F18, 0x001F1D),
    (0x001F20, 0x001F45),
    (0x001F48, 0x001F4D),
    (0x001F50, 0x001F57),
    (0x001F59, 0x001F59),
    (0x001F5B, 0x001F5B),
    (0x001F5D, 0x001F5D),
    (0x001F5F, 0x001F7D),
    (0x001F80, 0x001FB4),
    (0x001FB6, 0x001FC4),
    (0x001FC6, 0x001FD3),
    (0x001FD6, 0x001FDB),
    (0x001FDD, 0x001FEF),
    (0x001FF2, 0x001FF4),
    (0x001FF6, 0x001FFE),
    (0x002010, 0x002027),
    (0x002030, 0x00205E),
    (0x002070, 0x002071),
    (0x002074, 0x00208E),
    (0x002090, 0x00209C),
    (0x0020A0, 0x0020C0),
    (0x0020D0, 0x0020F0),
    (0x002100, 0x00218B),
    (0x002190, 0x002426),
    (0x002440, 0x00244A),
    (0x002460, 0x002B73),
    (0x002B76, 0x002B95),
    (0x002B97, 0x002CF3),
    (0x002CF9, 0x002D25),
    (0x002D27, 0x002D27),
    (0x002D2D, 0x002D2D),
    (0x002D30, 0x002D67),
    (0x002D6F, 0x002D70),
    (0x002D7F, 0x002D96),
    (0x002DA0, 0x002DA6),
    (0x002DA8, 0x002DAE),
    (0x002DB0, 0x002DB6),
    (0x002DB8, 0x002DBE),
    (0x002DC0, 0x002DC6),
    (0x002DC8, 0x002DCE),
    (0x002DD0, 0x002DD6),
    (0x002DD8, 0x002DDE),
    (0x002DE0, 0x002E5D),
    (0x002E80, 0x002E99),
    (0x002E9B, 0x002EF3),
    (0x002F00, 0x002FD5),
    (0x002FF0, 0x002FFB),
    (0x003001, 0x00303F),
    (0x003041, 0x003096),
    (0x003099, 0x0030FF),
    (0x003105, 0x00312F),
    (0x003131, 0x00318E),
    (0x003190, 0x0031E3),
    (0x0031F0, 0x00321E),
    (0x003220, 0x00A48C),
    (0x00A490, 0x00A4C6),
    (0x00A4D0, 0x00A62B),
    (0x00A640, 0x00A6F7),
    (0x00A700, 0x00A7CA),
    (0x00A7D0, 0x00A7D1),
    (0x00A7D3, 0x00A7D3),
    (0x00A7D5, 0x00A7D9),
    (0x00A7F2, 0x00A82C),
    (0x00A830, 0x00A839),
    (0x00A840, 0x00A877),
    (0x00A880, 0x00A8C5),
    (0x00A8CE, 0x00A8D9),
    (0x00A8E0, 0x00A953),
    (0x00A95F, 0x00A97C),
    (0x00A980, 0x00A9CD),
    (0x00A9CF, 0x00A9D9),
    (0x00A9DE, 0x00A9FE),
    (0x00AA00, 0x00AA36),
    (0x00AA40, 0x00AA4D),
    (0x00AA50, 0x00AA59),
    (0x00AA5C, 0x00AAC2),
    (0x00AADB, 0x00AAF6),
    (0x00AB01, 0x00AB06),
    (0x00AB09, 0x00AB0E),
    (0x00AB11, 0x00AB16),
    (0x00AB20, 0x00AB26),
    (0x00AB28, 0x00AB2E),
    (0x00AB30, 0x00AB6B),
    (0x00AB70, 0x00ABED),
    (0x00ABF0, 0x00ABF9),
    (0x00AC00, 0x00D7A3),
    (0x00D7B0, 0x00D7C6),
    (0x00D7CB, 0x00D7FB),
    (0x00F900, 0x00FA6D),
    (0x00FA70, 0x00FAD9),
    (0x00FB00, 0x00FB06),
    (0x00FB13, 0x00FB17),
    (0x00FB1D, 0x00FB36),
    (0x00FB38, 0x00FB3C),
    (0x00FB3E, 0x00FB3E),
    (0x00FB40, 0x00FB41),
    (0x00FB43, 0x00FB44),
    (0x00FB46, 0x00FBC2),
    (0x00FBD3, 0x00FD8F),
    (0x00FD92, 0x00FDC7),
    (0x00FDCF, 0x00FDCF),
    (0x00FDF0, 0x00FE19),
    (0x00FE20, 0x00FE52),
    (0x00FE54, 0x00FE66),
    (0x00FE68, 0x00FE6B),
    (0x00FE70, 0x00FE74),
    (0x00FE76, 0x00FEFC),
    (0x00FF01, 0x00FFBE),
    (0x00FFC2, 0x00FFC7),
    (0x00FFCA, 0x00FFCF),
    (0x00FFD2, 0x00FFD7),
    (0x00FFDA, 0x00FFDC),
    (0x00FFE0, 0x00FFE6),
    (0x00FFE8, 0x00FFEE),
    (0x00FFFC, 0x00FFFD),
    (0x010000, 0x01000B),
    (0x01000D, 0x010026),
    (0x010028, 0x01003A),
    (0x01003C, 0x01003D),
    (0x01003F, 0x01004D),
    (0x010050, 0x01005D),
    (0x010080, 0x0100FA),
    (0x010100, 0x010102),
    (0x010107, 0x010133),
    (0x010137, 0x01018E),
    (0x010190, 0x01019C),
    (0x0101A0, 0x0101A0),
    (0x0101D0, 0x0101FD),
    (0x010280, 0x01029C),
    (0x0102A0, 0x0102D0),
    (0x0102E0, 0x0102FB),
    (0x010300, 0x010323),
    (0x01032D, 0x01034A),
    (0x010350, 0x01037A),
    (0x010380, 0x01039D),
    (0x01039F, 0x0103C3),
    (0x0103C8, 0x0103D5),
    (0x010400, 0x01049D),
    (0x0104A0, 0x0104A9),
    (0x0104B0, 0x0104D3),
    (0x0104D8, 0x0104FB),
    (0x010500, 0x010527),
    (0x010530, 0x010563),
    (0x01056F, 0x01057A),
    (0x01057C, 0x01058A),
    (0x01058C, 0x010592),
    (0x010594, 0x010595),
    (0x010597, 0x0105A1),
    (0x0105A3, 0x0105B1),
    (0x0105B3, 0x0105B9),
    (0x0105BB, 0x0105BC),
    (0x010600, 0x010736),
    (0x010740, 0x010755),
    (0x010760, 0x010767),
    (0x010780, 0x010785),
    (0x010787, 0x0107B0),
    (0x0107B2, 0x0107BA),
    (0x010800, 0x010805),
    (0x010808, 0x010808),
    (0x01080A, 0x010835),
    (0x010837, 0x010838),
    (0x01083C, 0x01083C),
    (0x01083F, 0x010855),
    (0x010857, 0x01089E),
    (0x0108A7, 0x0108AF),
    (0x0108E0, 0x0108F2),
    (0x0108F4, 0x0108F5),
    (0x0108FB, 0x01091B),
    (0x01091F, 0x010939),
    (0x01093F, 0x01093F),
    (0x010980, 0x0109B7),
    (0x0109BC, 0x0109CF),
    (0x0109D2, 0x010A03),
    (0x010A05, 0x010A06),
    (0x010A0C, 0x010A13),
    (0x010A15, 0x010A17),
    (0x010A19, 0x010A35),
    (0x010A38, 0x010A3A),
    (0x010A3F, 0x010A48),
    (0x010A50, 0x010A58),
    (0x010A60, 0x010A9F),
    (0x010AC0, 0x010AE6),
    (0x010AEB, 0x010AF6),
    (0x010B00, 0x010B35),
    (0x010B39, 0x010B55),
    (0x010B58, 0x010B72),
    (0x010B78, 0x010B91),
    (0x010B99, 0x010B9C),
    (0x010BA9, 0x010BAF),
    (0x010C00, 0x010C48),
    (0x010C80, 0x010CB2),
    (0x010CC0, 0x010CF2),
    (0x010CFA, 0x010D27),
    (0x010D30, 0x010D39),
    (0x010E60, 0x010E7E),
    (0x010E80, 0x010EA9),
    (0x010EAB, 0x010EAD),
    (0x010EB0, 0x010EB1),
    (0x010EFD, 0x010F27),
    (0x010F30, 0x010F59),
    (0x010F70, 0x010F89),
    (0x010FB0, 0x010FCB),
    (0x010FE0, 0x010FF6),
    (0x011000, 0x01104D),
    (0x011052, 0x011075),
    (0x01107F, 0x0110BC),
    (0x0110BE, 0x0110C2),
    (0x0110D0, 0x0110E8),
    (0x0110F0, 0x0110F9),
    (0x011100, 0x011134),
    (0x011136, 0x011147),
    (0x011150, 0x011176),
    (0x011180, 0x0111DF),
    (0x0111E1, 0x0111F4),
    (0x011200, 0x011211),
    (0x011213, 0x011241),
    (0x011280, 0x011286),
    (0x011288, 0x011288),
    (0x01128A, 0x01128D),
    (0x01128F, 0x01129D),
    (0x01129F, 0x0112A9),
    (0x0112B0, 0x0112EA),
    (0x0112F0, 0x0112F9),
    (0x011300, 0x011303),
    (0x011305, 0x01130C),
    (0x01130F, 0x011310),
    (0x011313, 0x011328),
    (0x01132A, 0x011330),
    (0x011332, 0x011333),
    (0x011335, 0x011339),
    (0x01133B, 0x011344),
    (0x011347, 0x011348),
    (0x01134B, 0x01134D),
    (0x011350, 0x011350),
    (0x011357, 0x011357),
    (0x01135D, 0x011363),
    (0x011366, 0x01136C),
    (0x011370, 0x011374),
    (0x011400, 0x01145B),
    (0x01145D, 0x011461),
    (0x011480, 0x0114C7),
    (0x0114D0, 0x0114D9),
    (0x011580, 0x0115B5),
    (0x0115B8, 0x0115DD),
    (0x011600, 0x011644),
    (0x011650, 0x011659),
    (0x011660, 0x01166C),
    (0x011680, 0x0116B9),
    (0x0116C0, 0x0116C9),
    (0x011700, 0x01171A),
    (0x01171D, 0x01172B),
    (0x011730, 0x011746),
    (0x011800, 0x01183B),
    (0x0118A0, 0x0118F2),
    (0x0118FF, 0x011906),
    (0x011909, 0x011909),
    (0x01190C, 0x011913),
    (0x011915, 0x011916),
    (0x011918, 0x011935),
    (0x011937, 0x011938),
    (0x01193B, 0x011946),
    (0x011950, 0x011959),
    (0x0119A0, 0x0119A7),
    (0x0119AA, 0x0119D7),
    (0x0119DA, 0x0119E4),
    (0x011A00, 0x011A47),
    (0x011A50, 0x011AA2),
    (0x011AB0, 0x011AF8),
    (0x011B00, 0x011B09),
    (0x011C00, 0x011C08),
    (0x011C0A, 0x011C36),
    (0x011C38, 0x011C45),
    (0x011C50, 0x011C6C),
    (0x011C70, 0x011C8F),
    (0x011C92, 0x011CA7),
    (0x011CA9, 0x011CB6),
    (0x011D00, 0x011D06),
    (0x011D08, 0x011D09),
    (0x011D0B, 0x011D36),
    (0x011D3A, 0x011D3A),
    (0x011D3C, 0x011D3D),
    (0x011D3F, 0x011D47),
    (0x011D50, 0x011D59),
    (0x011D60, 0x011D65),
    (0x011D67, 0x011D68),
    (0x011D6A, 0x011D8E),
    (0x011D90, 0x011D91),
    (0x011D93, 0x011D98),
    (0x011DA0, 0x011DA9),
    (0x011EE0, 0x011EF8),
    (0x011F00, 0x011F10),
    (0x011F12, 0x011F3A),
    (0x011F3E, 0x011F59),
    (0x011FB0, 0x011FB0),
    (0x011FC0, 0x011FF1),
    (0x011FFF, 0x012399),
    (0x012400, 0x01246E),
    (0x012470, 0x012474),
    (0x012480, 0x012543),
    (0x012F90, 0x012FF2),
    (0x013000, 0x01342F),
    (0x013440, 0x013455),
    (0x014400, 0x014646),
    (0x016800, 0x016A38),
    (0x016A40, 0x016A5E),
    (0x016A60, 0x016A69),
    (0x016A6E, 0x016ABE),
    (0x016AC0, 0x016AC9),
    (0x016AD0, 0x016AED),
    (0x016AF0, 0x016AF5),
    (0x016B00, 0x016B45),
    (0x016B50, 0x016B59),
    (0x016B5B, 0x016B61),
    (0x016B63, 0x016B77),
    (0x016B7D, 0x016B8F),
    (0x016E40, 0x016E9A),
    (0x016F00, 0x016F4A),
    (0x016F4F, 0x016F87),
    (0x016F8F, 0x016F9F),
    (0x016FE0, 0x016FE4),
    (0x016FF0, 0x016FF1),
    (0x017000, 0x0187F7),
    (0x018800, 0x018CD5),
    (0x018D00, 0x018D08),
    (0x01AFF0, 0x01AFF3),
    (0x01AFF5, 0x01AFFB),
    (0x01AFFD, 0x01AFFE),
    (0x01B000, 0x01B122),
    (0x01B132, 0x01B132),
    (0x01B150, 0x01B152),
    (0x01B155, 0x01B155),
    (0x01B164, 0x01B167),
    (0x01B170, 0x01B2FB),
    (0x01BC00, 0x01BC6A),
    (0x01BC70, 0x01BC7C),
    (0x01BC80, 0x01BC88),
    (0x01BC90, 0x01BC99),
    (0x01BC9C, 0x01BC9F),
    (0x01CF00, 0x01CF2D),
    (0x01CF30, 0x01CF46),
    (0x01CF50, 0x01CFC3),
    (0x01D000, 0x01D0F5),
    (0x01D100, 0x01D126),
    (0x01D129, 0x01D172),
    (0x01D17B, 0x01D1EA),
    (0x01D200, 0x01D245),
    (0x01D2C0, 0x01D2D3),
    (0x01D2E0, 0x01D2F3),
    (0x01D300, 0x01D356),
    (0x01D360, 0x01D378),
    (0x01D400, 0x01D454),
    (0x01D456, 0x01D49C),
    (0x01D49E, 0x01D49F),
    (0x01D4A2, 0x01D4A2),
    (0x01D4A5, 0x01D4A6),
    (0x01D4A9, 0x01D4AC),
    (0x01D4AE, 0x01D4B9),
    (0x01D4BB, 0x01D4BB),
    (0x01D4BD, 0x01D4C3),
    (0x01D4C5, 0x01D505),
    (0x01D507, 0x01D50A),
    (0x01D50D, 0x01D514),
    (0x01D516, 0x01D51C),
    (0x01D51E, 0x01D539),
    (0x01D53B, 0x01D53E),
    (0x01D540, 0x01D544),
    (0x01D546, 0x01D546),
    (0x01D54A, 0x01D550),
    (0x01D552, 0x01D6A5),
    (0x01D6A8, 0x01D7CB),
    (0x01D7CE, 0x01DA8B),
    (0x01DA9B, 0x01DA9F),
    (0x01DAA1, 0x01DAAF),
    (0x01DF00, 0x01DF1E),
    (0x01DF25, 0x01DF2A),
    (0x01E000, 0x01E006),
    (0x01E008, 0x01E018),
    (0x01E01B, 0x01E021),
    (0x01E023, 0x01E024),
    (0x01E026, 0x01E02A),
    (0x01E030, 0x01E06D),
    (0x01E08F, 0x01E08F),
    (0x01E100, 0x01E12C),
    (0x01E130, 0x01E13D),
    (0x01E140, 0x01E149),
    (0x01E14E, 0x01E14F),
    (0x01E290, 0x01E2AE),
    (0x01E2C0, 0x01E2F9),
    (0x01E2FF, 0x01E2FF),
    (0x01E4D0, 0x01E4F9),
    (0x01E7E0, 0x01E7E6),
    (0x01E7E8, 0x01E7EB),
    (0x01E7ED, 0x01E7EE),
    (0x01E7F0, 0x01E7FE),
    (0x01E800, 0x01E8C4),
    (0x01E8C7, 0x01E8D6),
    (0x01E900, 0x01E94B),
    (0x01E950, 0x01E959),
    (0x01E95E, 0x01E95F),
    (0x01EC71, 0x01ECB4),
    (0x01ED01, 0x01ED3D),
    (0x01EE00, 0x01EE03),
    (0x01EE05, 0x01EE1F),
    (0x01EE21, 0x01EE22),
    (0x01EE24, 0x01EE24),
    (0x01EE27, 0x01EE27),
    (0x01EE29, 0x01EE32),
    (0x01EE34, 0x01EE37),
    (0x01EE39, 0x01EE39),
    (0x01EE3B, 0x01EE3B),
    (0x01EE42, 0x01EE42),
    (0x01EE47, 0x01EE47),
    (0x01EE49, 0x01EE49),
    (0x01EE4B, 0x01EE4B),
    (0x01EE4D, 0x01EE4F),
    (0x01EE51, 0x01EE52),
    (0x01EE54, 0x01EE54),
    (0x01EE57, 0x01EE57),
    (0x01EE59, 0x01EE59),
    (0x01EE5B, 0x01EE5B),
    (0x01EE5D, 0x01EE5D),
    (0x01EE5F, 0x01EE5F),
    (0x01EE61, 0x01EE62),
    (0x01EE64, 0x01EE64),
    (0x01EE67, 0x01EE6A),
    (0x01EE6C, 0x01EE72),
    (0x01EE74, 0x01EE77),
    (0x01EE79, 0x01EE7C),
    (0x01EE7E, 0x01EE7E),
    (0x01EE80, 0x01EE89),
    (0x01EE8B, 0x01EE9B),
    (0x01EEA1, 0x01EEA3),
    (0x01EEA5, 0x01EEA9),
    (0x01EEAB, 0x01EEBB),
    (0x01EEF0, 0x01EEF1),
    (0x01F000, 0x01F02B),
    (0x01F030, 0x01F093),
    (0x01F0A0, 0x01F0AE),
    (0x01F0B1, 0x01F0BF),
    (0x01F0C1, 0x01F0CF),
    (0x01F0D1, 0x01F0F5),
    (0x01F100, 0x01F1AD),
    (0x01F1E6, 0x01F202),
    (0x01F210, 0x01F23B),
    (0x01F240, 0x01F248),
    (0x01F250, 0x01F251),
    (0x01F260, 0x01F265),
    (0x01F300, 0x01F6D7),
    (0x01F6DC, 0x01F6EC),
    (0x01F6F0, 0x01F6FC),
    (0x01F700, 0x01F776),
    (0x01F77B, 0x01F7D9),
    (0x01F7E0, 0x01F7EB),
    (0x01F7F0, 0x01F7F0),
    (0x01F800, 0x01F80B),
    (0x01F810, 0x01F847),
    (0x01F850, 0x01F859),
    (0x01F860, 0x01F887),
    (0x01F890, 0x01F8AD),
    (0x01F8B0, 0x01F8B1),
    (0x01F900, 0x01FA53),
    (0x01FA60, 0x01FA6D),
    (0x01FA70, 0x01FA7C),
    (0x01FA80, 0x01FA88),
    (0x01FA90, 0x01FABD),
    (0x01FABF, 0x01FAC5),
    (0x01FACE, 0x01FADB),
    (0x01FAE0, 0x01FAE8),
    (0x01FAF0, 0x01FAF8),
    (0x01FB00, 0x01FB92),
    (0x01FB94, 0x01FBCA),
    (0x01FBF0, 0x01FBF9),
    (0x020000, 0x02A6DF),
    (0x02A700, 0x02B739),
    (0x02B740, 0x02B81D),
    (0x02B820, 0x02CEA1),
    (0x02CEB0, 0x02EBE0),
    (0x02F800, 0x02FA1D),
    (0x030000, 0x03134A),
    (0x031350, 0x0323AF),
    (0x0E0100, 0x0E01EF),
];

/// Whether `repr()` renders a code point verbatim (binary search).
fn repr_is_verbatim(cp: u32) -> bool {
    let mut low = 0usize;
    let mut high = REPR_VERBATIM.len();
    while low < high {
        let mid = (low + high) / 2;
        let (start, end) = REPR_VERBATIM[mid];
        if cp < start {
            high = mid;
        } else if cp > end {
            low = mid + 1;
        } else {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// JNum / JVal / JObject: values with CPython's number grammar
// ---------------------------------------------------------------------------

/// A JSON number literal: the verbatim text plus whether the grammar made
/// it a float (fraction or exponent present — `5e3` is a float, `5` and
/// `2**128` are ints, exactly like serde's `arbitrary_precision`). Floats
/// parse saturating (`1e400` is inf, `1e-400` is 0.0), like CPython's
/// `float()`; `NaN`/`Infinity` tokens never reach here (strict rejects).
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

/// A parsed JSON value with CPython's number grammar and
/// surrogate-preserving strings.
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
/// first-seen positions (CPython `dict` semantics, matching serde's
/// `preserve_order` maps).
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

    /// Member lookup by clean name (dirty keys never match).
    pub fn get(&self, key: &str) -> Option<&JVal> {
        self.entries
            .iter()
            .find(|(name, _)| name.eq_str(key))
            .map(|(_, value)| value)
    }

    /// Whether a clean member name is present (Python `in` on dicts).
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
    /// (`JVal` implements [`Drop`], so callers cannot move variant
    /// contents out with a plain `match`.)
    pub fn into_object(mut self) -> Option<JObject> {
        match &mut self {
            JVal::Object(map) => Some(std::mem::take(map)),
            _ => None,
        }
    }
}

impl Drop for JVal {
    fn drop(&mut self) {
        // Iterative: values nest to `MAX_CONTAINER_DEPTH` and the derived
        // drop recurses per level, overflowing 2MB workers past ~4000
        // levels — the same abort class as serde's `Map` drop (found in
        // the 626 review: the parity battery aborted on macOS defaults).
        // Children move onto an explicit work stack; each visited shell
        // is forgotten (it holds no allocation — children leave via
        // `take`, which resets to capacity zero), so no visited value
        // ever re-enters this `drop`.
        let mut stack = vec![std::mem::replace(self, JVal::Null)];
        while let Some(mut owned) = stack.pop() {
            match &mut owned {
                JVal::Array(items) => stack.extend(std::mem::take(items)),
                JVal::Object(object) => stack.extend(
                    std::mem::take(&mut object.entries)
                        .into_iter()
                        .map(|(_, member)| member),
                ),
                JVal::Null | JVal::Bool(_) | JVal::Num(_) | JVal::Str(_) => {}
            }
            std::mem::forget(owned);
        }
    }
}

// ---------------------------------------------------------------------------
// Parser: single-pass iterative recursive descent
// ---------------------------------------------------------------------------

/// A body-parse failure: either CPython's error text (the `ParseError`
/// suffix, position included or not as CPython renders it) or the depth
/// cap (Django's JSON 500 — `RecursionError` is not a `ValueError`, so
/// DRF does not catch it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonFail {
    Message(String),
    Recursion,
}

/// `request.data` exactly as DRF's `Request._parse` produces it: a
/// zero-length body short-circuits to `{}` (the content-length check —
/// verified live: an empty POST answers serializer errors, never a
/// parse error), anything else goes through [`parse_request_bytes`].
pub fn parse_request_data(raw: &[u8]) -> Result<JVal, JsonFail> {
    parse_request_data_spans(raw, &[])
}

/// [`parse_request_data`] over charset-decoded text carrying
/// lone-surrogate spans (exotic charsets, PIDASHCONV-693).
pub fn parse_request_data_spans(raw: &[u8], surr: &[(usize, u16)]) -> Result<JVal, JsonFail> {
    if raw.is_empty() {
        return Ok(JVal::Object(JObject::new()));
    }
    parse_request_bytes_spans(raw, surr)
}

/// Parse request bytes exactly as DRF's `JSONParser` does: the UTF-8 codec
/// first (a codec failure wins over any syntax error), then the JSON
/// text. Fires every error in true scan order — syntax, strict-constant,
/// int-limit, depth-cap — with byte-exact CPython text.
pub fn parse_request_bytes(raw: &[u8]) -> Result<JVal, JsonFail> {
    parse_request_bytes_spans(raw, &[])
}

/// [`parse_request_bytes`] over charset-decoded text carrying
/// lone-surrogate spans (exotic charsets, PIDASHCONV-693). Decoded text
/// is always valid UTF-8, so only the tail-drop arm can truncate —
/// and spans past the cut are dropped with it.
pub fn parse_request_bytes_spans(raw: &[u8], surr: &[(usize, u16)]) -> Result<JVal, JsonFail> {
    let text = match std::str::from_utf8(raw) {
        Ok(text) => text,
        Err(_) => match incomplete_tail_start(raw) {
            // A trailing incomplete sequence is silently dropped (the
            // codecs `StreamReader` never flushes its tail: `{"a":1}\xc3`
            // parses as `{"a":1}`, and even `\xed\xa0` — which could never
            // complete — vanishes; verified live); the truncated prefix
            // parses as-is, even when it strips to empty.
            Some(start) => {
                std::str::from_utf8(&raw[..start]).expect("scanned prefix is valid UTF-8")
            }
            None => return Err(JsonFail::Message(utf8_decode_detail(raw))),
        },
    };
    let kept = surr
        .iter()
        .take_while(|(off, _)| *off < text.len())
        .copied()
        .collect::<Vec<_>>();
    parse_json_text_spans(text, &kept)
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

/// Parse decoded JSON text (see [`parse_request_bytes`]).
pub fn parse_json_text(text: &str) -> Result<JVal, JsonFail> {
    parse_json_text_spans(text, &[])
}

/// Parse decoded JSON text carrying lone-surrogate spans from the
/// charset decode (exotic charsets, PIDASHCONV-693): each span is the
/// byte offset of a U+FFFD placeholder plus the surrogate value. The
/// string reader swaps placeholders for [`StrUnit::Sur`] units.
pub fn parse_json_text_spans(text: &str, surr: &[(usize, u16)]) -> Result<JVal, JsonFail> {
    // A leading BOM is CPython's one special case (anywhere else it is an
    // ordinary char, and inside strings a literal).
    if text.starts_with('\u{FEFF}') {
        return Err(JsonFail::Message(
            "Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)".to_owned(),
        ));
    }
    let mut parser = Scan {
        text,
        bytes: text.as_bytes(),
        pos: skip_ws_from(text.as_bytes(), 0),
        stack: Vec::new(),
        surr,
        surr_idx: 0,
    };
    // The root value: `None` while a root container is still open.
    let mut pending = parser.read_value()?;
    loop {
        match pending.take() {
            None => {
                // Just opened a container: move to its first slot. (`None`
                // only ever means "just opened", so a closer here always
                // ends an empty container.)
                let is_array = matches!(parser.stack.last(), Some(Frame::Array(_)));
                parser.skip_ws();
                if is_array {
                    if parser.consume(b']') {
                        pending = Some(parser.close_array());
                    } else {
                        pending = parser.read_value()?;
                    }
                } else {
                    if parser.consume(b'}') {
                        pending = Some(parser.close_object());
                    } else {
                        let key = parser.read_key()?;
                        parser.expect_colon()?;
                        parser.set_pending_key(key);
                        pending = parser.read_value()?;
                    }
                }
            }
            Some(value) => {
                if parser.stack.is_empty() {
                    // Root value complete: trailing text is `Extra data`.
                    parser.skip_ws();
                    if parser.pos == parser.bytes.len() {
                        return Ok(value);
                    }
                    return Err(parser.fail("Extra data", parser.pos));
                }
                parser.place_value(value);
                // After a value: `,` or the matching closer.
                parser.skip_ws();
                let Some(byte) = parser.bytes.get(parser.pos).copied() else {
                    return Err(parser.eof());
                };
                if byte == b',' {
                    parser.pos += 1;
                    let is_array = matches!(parser.stack.last(), Some(Frame::Array(_)));
                    if is_array {
                        pending = parser.read_value()?;
                    } else {
                        // After a comma a `}` is a missing key, not an end.
                        let key = parser.read_key()?;
                        parser.expect_colon()?;
                        parser.set_pending_key(key);
                        pending = parser.read_value()?;
                    }
                    continue;
                }
                let is_array = matches!(parser.stack.last(), Some(Frame::Array(_)));
                if is_array && byte == b']' {
                    parser.pos += 1;
                    pending = Some(parser.close_array());
                } else if !is_array && byte == b'}' {
                    parser.pos += 1;
                    pending = Some(parser.close_object());
                } else {
                    return Err(parser.fail_margin("Expecting ',' delimiter", parser.pos, 4));
                }
            }
        }
    }
}

/// One unclosed container on the explicit stack.
enum Frame {
    Array(Vec<JVal>),
    Object(JObject, Option<JStr>),
}

/// The scanner state (byte offsets; positions render as chars on failure).
struct Scan<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    stack: Vec<Frame>,
    /// Lone-surrogate spans from the charset decode: byte offset of a
    /// U+FFFD placeholder in `text` plus the surrogate value, sorted by
    /// offset. The string reader swaps placeholders for [`StrUnit::Sur`]
    /// units (exotic charsets, PIDASHCONV-693); spans outside strings
    /// (structural positions) are skipped — the syntax error there (or
    /// the clean parse) matches Django either way.
    surr: &'a [(usize, u16)],
    surr_idx: usize,
}

impl Scan<'_> {
    fn skip_ws(&mut self) {
        self.pos = skip_ws_from(self.bytes, self.pos);
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn fail(&self, template: &str, at: usize) -> JsonFail {
        let (line, column, ch) = cpython_pos(self.text, at);
        JsonFail::Message(format!(
            "{template}: line {line} column {column} (char {ch})"
        ))
    }

    /// A syntax failure with the C scanner's error margin: within `margin`
    /// levels of the cap the budget check fires first (`RecursionError`
    /// instead of the message — measured live through Django: the
    /// value-dispatch family surfaces to depth 9938, the
    /// string/object/delimiter/strict family to 9935).
    fn fail_margin(&self, template: &str, at: usize, margin: usize) -> JsonFail {
        if self.stack.len() + margin > MAX_CONTAINER_DEPTH {
            JsonFail::Recursion
        } else {
            self.fail(template, at)
        }
    }

    fn eof(&self) -> JsonFail {
        let (template, at) = eof_detail(self.text);
        // End of input takes its family's margin (verified live for the
        // value and delimiter shapes; the object shapes share their
        // family's code path).
        let margin = match template {
            "Expecting value" => 1,
            _ => 4,
        };
        self.fail_margin(template, at, margin)
    }

    fn close_array(&mut self) -> JVal {
        match self.stack.pop() {
            Some(Frame::Array(items)) => JVal::Array(items),
            _ => unreachable!("array closer matches array frame"),
        }
    }

    fn close_object(&mut self) -> JVal {
        match self.stack.pop() {
            Some(Frame::Object(map, None)) => JVal::Object(map),
            _ => unreachable!("object closer matches keyless object frame"),
        }
    }

    fn set_pending_key(&mut self, key: JStr) {
        match self.stack.last_mut() {
            Some(Frame::Object(_, slot)) => *slot = Some(key),
            _ => unreachable!("pending key sets on object frame"),
        }
    }

    fn place_value(&mut self, value: JVal) {
        match self.stack.last_mut() {
            Some(Frame::Array(items)) => items.push(value),
            Some(Frame::Object(map, key)) => {
                let name = key.take().expect("object value has pending key");
                map.insert(name, value);
            }
            None => unreachable!("placement with empty stack handled by caller"),
        }
    }

    /// Read one value at the current position (after skipping whitespace):
    /// `Ok(None)` opens a container (the frame is pushed, the caller moves
    /// to its first slot); `Ok(Some)` completes a scalar.
    fn read_value(&mut self) -> Result<Option<JVal>, JsonFail> {
        self.skip_ws();
        let Some(byte) = self.bytes.get(self.pos).copied() else {
            return Err(self.eof());
        };
        match byte {
            b'{' | b'[' => {
                if self.stack.len() + 1 > MAX_CONTAINER_DEPTH {
                    return Err(JsonFail::Recursion);
                }
                self.pos += 1;
                if byte == b'{' {
                    self.stack.push(Frame::Object(JObject::new(), None));
                } else {
                    self.stack.push(Frame::Array(Vec::new()));
                }
                Ok(None)
            }
            b'"' => Ok(Some(JVal::Str(self.read_string()?))),
            b't' => self.read_literal("true", JVal::Bool(true)),
            b'f' => self.read_literal("false", JVal::Bool(false)),
            b'n' => self.read_literal("null", JVal::Null),
            b'N' => {
                if self.bytes[self.pos..].starts_with(b"NaN") {
                    Err(self.strict_fail("NaN"))
                } else {
                    Err(self.fail_margin("Expecting value", self.pos, 1))
                }
            }
            b'I' => {
                if self.bytes[self.pos..].starts_with(b"Infinity") {
                    Err(self.strict_fail("Infinity"))
                } else {
                    Err(self.fail_margin("Expecting value", self.pos, 1))
                }
            }
            b'-' => {
                if self.bytes[self.pos..].starts_with(b"-Infinity") {
                    return Err(self.strict_fail("-Infinity"));
                }
                Ok(Some(self.read_number()?))
            }
            b'0'..=b'9' => Ok(Some(self.read_number()?)),
            _ => Err(self.fail_margin("Expecting value", self.pos, 1)),
        }
    }

    /// Match one exact literal (`true`/`false`/`null`): a complete match
    /// is the value even before trailing garbage (`nullx` errors after
    /// it, at the delimiter check); anything else wants a value at the
    /// token start.
    fn read_literal(&mut self, word: &str, value: JVal) -> Result<Option<JVal>, JsonFail> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(Some(value))
        } else {
            Err(self.fail_margin("Expecting value", self.pos, 1))
        }
    }

    /// Read one number at the current position (leading `-`/digit checked
    /// by the caller): CPython's `number_re` grammar, greedy prefix; int
    /// literals past [`MAX_INT_DIGITS`] are the digit-limit 400.
    fn read_number(&mut self) -> Result<JVal, JsonFail> {
        let start = self.pos;
        let Some(end) = number_prefix_end(self.bytes, start) else {
            return Err(self.fail_margin("Expecting value", start, 1));
        };
        let literal = &self.text[start..end];
        debug_assert!(!literal.is_empty());
        let is_float = literal.bytes().any(|b| b == b'.' || b == b'e' || b == b'E');
        if !is_float {
            let digits = end - start - usize::from(self.bytes[start] == b'-');
            if digits > MAX_INT_DIGITS {
                // The digit-limit conversion shares the value-dispatch
                // margin (verified live: it surfaces to depth 9938).
                if self.stack.len() + 1 > MAX_CONTAINER_DEPTH {
                    return Err(JsonFail::Recursion);
                }
                return Err(JsonFail::Message(format!(
                    "Exceeds the limit ({MAX_INT_DIGITS} digits) for integer string conversion: \
                     value has {digits} digits; use sys.set_int_max_str_digits() to increase the limit"
                )));
            }
            self.pos = end;
            return Ok(JVal::Num(JNum::int(literal.to_owned())));
        }
        self.pos = end;
        Ok(JVal::Num(JNum::float(literal.to_owned())))
    }

    /// Read one object key at the current position (after skipping
    /// whitespace): anything but a string wants a property name here.
    fn read_key(&mut self) -> Result<JStr, JsonFail> {
        self.skip_ws();
        let Some(byte) = self.bytes.get(self.pos).copied() else {
            return Err(self.eof());
        };
        if byte != b'"' {
            return Err(self.fail_margin(
                "Expecting property name enclosed in double quotes",
                self.pos,
                4,
            ));
        }
        self.read_string()
    }

    /// Expect the `:` after an object key.
    fn expect_colon(&mut self) -> Result<(), JsonFail> {
        self.skip_ws();
        let Some(byte) = self.bytes.get(self.pos).copied() else {
            return Err(self.eof());
        };
        if byte != b':' {
            return Err(self.fail_margin("Expecting ':' delimiter", self.pos, 4));
        }
        self.pos += 1;
        Ok(())
    }

    /// Read one string starting at the current position (the opening
    /// quote): standard escapes plus `\uXXXX` with surrogate pairing —
    /// lone leads/trails become [`StrUnit::Sur`] units (CPython accepts
    /// them), astral pairs combine. Unterminated strings report the
    /// opener unless the tail holds a truncated (or exactly terminal)
    /// `\u` escape, which CPython reports instead.
    fn read_string(&mut self) -> Result<JStr, JsonFail> {
        debug_assert_eq!(self.bytes.get(self.pos), Some(&b'"'));
        let open = self.pos;
        self.pos += 1;
        let mut out = JStr::new();
        loop {
            let Some(byte) = self.bytes.get(self.pos).copied() else {
                return Err(self.unterminated_string(open));
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => self.read_escape(&mut out, open)?,
                0x00..=0x1F => {
                    return Err(self.fail_margin("Invalid control character at", self.pos, 4));
                }
                _ => {
                    // A charset-decode surrogate placeholder at this
                    // offset becomes a `Sur` unit (spans are sorted;
                    // `pos` only advances, so one cursor suffices).
                    while self.surr_idx < self.surr.len() && self.surr[self.surr_idx].0 < self.pos {
                        self.surr_idx += 1;
                    }
                    if self.surr_idx < self.surr.len()
                        && self.surr[self.surr_idx].0 == self.pos
                        && self.bytes.get(self.pos..self.pos + 3) == Some(b"\xef\xbf\xbd")
                    {
                        out.push_surrogate(self.surr[self.surr_idx].1);
                        self.surr_idx += 1;
                        self.pos += 3;
                        continue;
                    }
                    // Valid UTF-8 input: decode one scalar value.
                    let rest = &self.text[self.pos..];
                    let ch = rest.chars().next().expect("char boundary");
                    out.push_char(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    /// Read one `\` escape at the current position (the backslash),
    /// appending to `out`. Bad escapes fail at the backslash (or the `u`);
    /// a truncated `\u` at end of input fails at the `u`.
    fn read_escape(&mut self, out: &mut JStr, open: usize) -> Result<(), JsonFail> {
        debug_assert_eq!(self.bytes.get(self.pos), Some(&b'\\'));
        let slash = self.pos;
        let Some(kind) = self.bytes.get(self.pos + 1).copied() else {
            // A lone backslash at end of input: the string is unterminated
            // (CPython reports the opener — the backslash is content).
            return Err(self.unterminated_string(open));
        };
        match kind {
            b'"' => {
                out.push_char('"');
                self.pos += 2;
            }
            b'\\' => {
                out.push_char('\\');
                self.pos += 2;
            }
            b'/' => {
                out.push_char('/');
                self.pos += 2;
            }
            b'b' => {
                out.push_char('\u{8}');
                self.pos += 2;
            }
            b'f' => {
                out.push_char('\u{C}');
                self.pos += 2;
            }
            b'n' => {
                out.push_char('\n');
                self.pos += 2;
            }
            b'r' => {
                out.push_char('\r');
                self.pos += 2;
            }
            b't' => {
                out.push_char('\t');
                self.pos += 2;
            }
            b'u' => {
                let unit = self.read_hex4(slash + 1)?;
                match unit {
                    // Lead surrogate: pair with an immediately following
                    // `\uDC00-\uDFFF` trail, else keep the lone lead.
                    0xD800..0xDC00 => {
                        self.pos += 6;
                        if let Some(trail) = self.peek_trail_escape() {
                            let high = u32::from(unit - 0xD800);
                            let low = u32::from(trail - 0xDC00);
                            let astral = char::from_u32(0x10000 + (high << 10) + low)
                                .expect("paired surrogates combine");
                            out.push_char(astral);
                            self.pos += 6;
                        } else {
                            out.push_surrogate(unit);
                        }
                    }
                    0xDC00..0xE000 => {
                        out.push_surrogate(unit);
                        self.pos += 6;
                    }
                    _ => {
                        out.push_char(char::from_u32(u32::from(unit)).expect("BMP scalar"));
                        self.pos += 6;
                    }
                }
            }
            _ => return Err(self.fail_margin("Invalid \\escape", slash, 4)),
        }
        Ok(())
    }

    /// Parse the four hex digits of a `\u` escape (`u_at` is the `u`):
    /// fewer than four hex digits (or a non-hex digit) is
    /// `Invalid \uXXXX escape` at the `u`, even in a closed string.
    fn read_hex4(&self, u_at: usize) -> Result<u16, JsonFail> {
        let mut value: u16 = 0;
        for offset in 0..4 {
            let Some(digit) = self.bytes.get(u_at + 1 + offset).copied() else {
                return Err(self.fail_margin("Invalid \\uXXXX escape", u_at, 4));
            };
            let nibble = match digit {
                b'0'..=b'9' => u16::from(digit - b'0'),
                b'a'..=b'f' => u16::from(digit - b'a') + 10,
                b'A'..=b'F' => u16::from(digit - b'A') + 10,
                _ => return Err(self.fail_margin("Invalid \\uXXXX escape", u_at, 4)),
            };
            value = value * 16 + nibble;
        }
        Ok(value)
    }

    /// The trail value when the input at the current position is exactly
    /// `\uDC00-\uDFFF` (for surrogate pairing); anything else (including
    /// a truncated `\u`, which the main loop reports) is not a trail.
    fn peek_trail_escape(&self) -> Option<u16> {
        if self.bytes.get(self.pos) != Some(&b'\\') || self.bytes.get(self.pos + 1) != Some(&b'u') {
            return None;
        }
        let mut value: u16 = 0;
        for offset in 0..4 {
            let digit = *self.bytes.get(self.pos + 2 + offset)?;
            let nibble = match digit {
                b'0'..=b'9' => u16::from(digit - b'0'),
                b'a'..=b'f' => u16::from(digit - b'a') + 10,
                b'A'..=b'F' => u16::from(digit - b'A') + 10,
                _ => return None,
            };
            value = value * 16 + nibble;
        }
        (0xDC00..0xE000).contains(&value).then_some(value)
    }

    /// The failure for end of input inside the string opened at `open`:
    /// `Unterminated string starting at` the opener — unless the tail
    /// holds a truncated `\u` escape (or a complete one ending exactly at
    /// end of input, which the C scanner reads past), reported instead.
    fn unterminated_string(&self, open: usize) -> JsonFail {
        if let Some(at) = truncated_hex_escape(self.text, open, self.text.len()) {
            return self.fail_margin("Invalid \\uXXXX escape", at, 4);
        }
        self.fail_margin("Unterminated string starting at", open, 4)
    }

    /// DRF strict-constant failure for one `NaN`/`Infinity`/`-Infinity`
    /// token in value position (repr-quoted token, no position): the
    /// callback shares the string family's margin (verified live: it
    /// surfaces to depth 9935).
    fn strict_fail(&self, token: &str) -> JsonFail {
        if self.stack.len() + 4 > MAX_CONTAINER_DEPTH {
            return JsonFail::Recursion;
        }
        JsonFail::Message(format!(
            "Out of range float values are not JSON compliant: '{token}'"
        ))
    }
}

/// Skip JSON whitespace (`\x0c` is not JSON whitespace — neither engine
/// skips it) from `pos`.
fn skip_ws_from(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && is_json_ws(bytes[pos]) {
        pos += 1;
    }
    pos
}

// ---------------------------------------------------------------------------
// Positions, codec errors, end-of-input analysis
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

/// CPython's 1-based `(line, column)` + 0-based char index for a byte
/// offset (chars, not bytes, past any multibyte text).
fn cpython_pos(text: &str, pos: usize) -> (usize, usize, usize) {
    let pos = pos.min(text.len());
    let prefix = &text[..pos];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column, prefix.chars().count())
}

/// JSON whitespace for the EOF region scan (`\x0c` is not JSON
/// whitespace — neither engine skips it).
fn is_json_ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Whether a byte continues a broken token when scanning back from a
/// serde offset: anything but structure, quotes and whitespace.
fn is_token_byte(byte: u8) -> bool {
    !matches!(byte, b'[' | b']' | b'{' | b'}' | b',' | b':' | b'"')
        && !matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Innermost unclosed bracket before `offset` (string-aware): the
/// trailing-comma context and the top-level test.
fn innermost_bracket(text: &str, offset: usize) -> Option<u8> {
    let bytes = text.as_bytes();
    let end = offset.min(bytes.len());
    let mut stack: Vec<u8> = Vec::new();
    let mut index = 0;
    while index < end {
        match bytes[index] {
            b'"' => {
                index += 1;
                while index < end && bytes[index] != b'"' {
                    if bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
                index += 1;
            }
            b'{' | b'[' => {
                stack.push(bytes[index]);
                index += 1;
            }
            b'}' | b']' => {
                stack.pop();
                index += 1;
            }
            _ => index += 1,
        }
    }
    stack.pop()
}

/// Match CPython's `number_re` at `start`: the valid-prefix end, if any
/// (`-?(0|[1-9]\d*)(\.\d+)?([eE][-+]?\d+)?`).
fn number_prefix_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            while matches!(bytes.get(index), Some(b'0'..=b'9')) {
                index += 1;
            }
        }
        _ => return None,
    }
    if bytes.get(index) == Some(&b'.') && matches!(bytes.get(index + 1), Some(b'0'..=b'9')) {
        index += 2;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        let mut end = index + 1;
        if matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
        if matches!(bytes.get(end), Some(b'0'..=b'9')) {
            end += 1;
            while matches!(bytes.get(end), Some(b'0'..=b'9')) {
                end += 1;
            }
            index = end;
        }
    }
    Some(index)
}

/// Opening quote of the string unterminated at `offset`: the nearest `"`
/// with an even run of preceding backslashes.
fn json_opening_quote(text: &str, offset: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = offset.min(bytes.len());
    while index > 0 {
        index -= 1;
        if bytes[index] == b'"' {
            let mut slashes = 0;
            let mut cursor = index;
            while cursor > 0 && bytes[cursor - 1] == b'\\' {
                slashes += 1;
                cursor -= 1;
            }
            if slashes % 2 == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// Index of the `u` when the string content's last `\u` escape is
/// truncated: fewer than 4 hex digits — or a complete escape ending
/// exactly at `end` (the C scanner reads past it: `"\u0041` →
/// `Invalid \uXXXX escape`).
fn truncated_hex_escape(text: &str, open: usize, end: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    // Last unescaped `\u` in the string content.
    let mut last_u: Option<usize> = None;
    let mut index = open + 1;
    while index < end {
        if bytes[index] == b'\\' {
            if bytes.get(index + 1) == Some(&b'u') {
                last_u = Some(index + 1);
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    let u = last_u?;
    let mut hex = 0;
    while hex < 4 && bytes.get(u + 1 + hex).is_some_and(u8::is_ascii_hexdigit) {
        hex += 1;
    }
    if hex < 4 || u + 5 == end {
        return Some(u);
    }
    None
}

/// Whether the string ending at `end` (exclusive) is an object key: its
/// opening quote follows `{` or `,` (else it is a value). The scan
/// excludes the closing quote itself, which is the nearest quote back.
fn is_key_string(text: &str, end: usize) -> bool {
    let bytes = text.as_bytes();
    if end == 0 {
        return false;
    }
    let Some(open) = json_opening_quote(text, end - 1) else {
        return false;
    };
    let mut index = open;
    while index > 0 && is_json_ws(bytes[index - 1]) {
        index -= 1;
    }
    index > 0 && matches!(bytes[index - 1], b'{' | b',')
}

/// End-of-input failures: CPython reports what it wanted next (a value,
/// a key, a colon, a delimiter) at the token start or the end.
fn eof_detail(text: &str) -> (&'static str, usize) {
    let bytes = text.as_bytes();
    let end = bytes.len();
    let mut start = end;
    while start > 0 && (is_token_byte(bytes[start - 1]) || is_json_ws(bytes[start - 1])) {
        start -= 1;
    }
    let first = bytes[start..end]
        .iter()
        .position(|byte| !is_json_ws(*byte))
        .map_or(end, |offset| start + offset);
    if first == end {
        return eof_after_char(text, start, end);
    }
    // A token trails: the context before it decides.
    if start == 0 {
        return eof_value_token(text, first, end, true);
    }
    match bytes[start - 1] {
        b'[' | b':' => eof_value_token(text, first, end, false),
        b'{' => ("Expecting property name enclosed in double quotes", first),
        b',' => match innermost_bracket(text, start - 1) {
            Some(b'[') => eof_value_token(text, first, end, false),
            _ => ("Expecting property name enclosed in double quotes", first),
        },
        b'"' => {
            // A complete string precedes the token (an opening quote here
            // would be an EOF-string error instead): after a key CPython
            // wants the colon, after a value the delimiter.
            if is_key_string(text, start) {
                ("Expecting ':' delimiter", first)
            } else {
                ("Expecting ',' delimiter", first)
            }
        }
        _ => ("Expecting ',' delimiter", first),
    }
}

/// Nothing but whitespace trails: the last structural char (or the whole
/// input) decides what CPython wanted at the end.
fn eof_after_char(text: &str, start: usize, end: usize) -> (&'static str, usize) {
    if start == 0 {
        return ("Expecting value", end);
    }
    let bytes = text.as_bytes();
    match bytes[start - 1] {
        b'[' => ("Expecting value", end),
        b'{' => ("Expecting property name enclosed in double quotes", end),
        b',' => match innermost_bracket(text, start - 1) {
            Some(b'[') => ("Expecting value", end),
            _ => ("Expecting property name enclosed in double quotes", end),
        },
        b':' => {
            if colon_after_key(text, start - 1) {
                ("Expecting value", end)
            } else {
                // A stray colon after a value: CPython wants the
                // delimiter at the colon itself.
                ("Expecting ',' delimiter", start - 1)
            }
        }
        b'"' => {
            if is_key_string(text, start) {
                ("Expecting ':' delimiter", end)
            } else {
                ("Expecting ',' delimiter", end)
            }
        }
        _ => ("Expecting ',' delimiter", end),
    }
}

/// Whether the colon at `pos` follows an object key (else it is stray).
fn colon_after_key(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    let mut index = pos;
    while index > 0 && is_json_ws(bytes[index - 1]) {
        index -= 1;
    }
    index > 0 && bytes[index - 1] == b'"' && is_key_string(text, index)
}

/// A value-position token at end of input: a valid number prefix plus
/// trailing garbage wants the delimiter at the prefix end (`Extra data`
/// at top level); a complete value wants the delimiter at the end; a
/// partial token wants a value at its start.
fn eof_value_token(text: &str, first: usize, end: usize, top: bool) -> (&'static str, usize) {
    let bytes = text.as_bytes();
    // Complete literals (`true`/`false`/`null`) behave like complete
    // numbers; a literal plus trailing garbage delimits after it.
    for literal in [b"true".as_slice(), b"false".as_slice(), b"null".as_slice()] {
        if bytes[first..end].starts_with(literal) {
            let after = first + literal.len();
            if bytes[after..end].iter().all(|b| is_json_ws(*b)) {
                return ("Expecting ',' delimiter", end);
            }
            if top {
                return ("Extra data", after);
            }
            return ("Expecting ',' delimiter", after);
        }
    }
    match number_prefix_end(bytes, first) {
        Some(prefix_end) if prefix_end > first => {
            if bytes[prefix_end..end].iter().all(|b| is_json_ws(*b)) {
                ("Expecting ',' delimiter", end)
            } else if top {
                ("Extra data", prefix_end)
            } else {
                ("Expecting ',' delimiter", prefix_end)
            }
        }
        _ => ("Expecting value", first),
    }
}

// ---------------------------------------------------------------------------
// Python str(): iterative, depth-capped
// ---------------------------------------------------------------------------

/// `str()` failed past [`MAX_STR_DEPTH`] (Django's JSON 500).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrDepthFail;

/// Python `str()` over a parsed value: strings verbatim (surrogates raw —
/// callers divert top-level dirty echoes to the renderer-crash 500),
/// bools as `True`/`False`, null as `None`, numbers via
/// [`JNum::py_string`], containers recursively with Python separators
/// (`, `, `: `) and repr-quoted strings. Iterative over an explicit
/// stack (values can nest to [`MAX_CONTAINER_DEPTH`]); past
/// [`MAX_STR_DEPTH`] the render fails like Django's `RecursionError`.
pub fn py_str(value: &JVal) -> Result<JStr, StrDepthFail> {
    let mut out = JStr::new();
    // Frames of still-open containers: (is_object, member index).
    let mut stack: Vec<(&JVal, usize)> = Vec::new();
    let mut current = value;
    // Depth of `current` (unclosed containers above it).
    let mut depth = 0usize;
    loop {
        match current {
            JVal::Null => out.push_jstr(&JStr::from_text("None")),
            JVal::Bool(true) => out.push_jstr(&JStr::from_text("True")),
            JVal::Bool(false) => out.push_jstr(&JStr::from_text("False")),
            JVal::Num(number) => out.push_jstr(&JStr::from_text(&number.py_string())),
            JVal::Str(text) => out.push_jstr(text),
            JVal::Array(items) => {
                if depth + 1 > MAX_STR_DEPTH {
                    return Err(StrDepthFail);
                }
                out.push_jstr(&JStr::from_text("["));
                if items.is_empty() {
                    out.push_jstr(&JStr::from_text("]"));
                } else {
                    stack.push((current, 0));
                    depth += 1;
                    current = &items[0];
                    // Nested strings render repr-quoted (fall through to
                    // the advance loop); anything else renders via the
                    // outer loop.
                    if matches!(current, JVal::Str(_)) {
                        render_quoted(current, &mut out);
                    } else {
                        continue;
                    }
                }
            }
            JVal::Object(object) => {
                if depth + 1 > MAX_STR_DEPTH {
                    return Err(StrDepthFail);
                }
                out.push_jstr(&JStr::from_text("{"));
                if object.is_empty() {
                    out.push_jstr(&JStr::from_text("}"));
                } else {
                    stack.push((current, 0));
                    depth += 1;
                    // Keys render repr-quoted, like values.
                    let (name, member) = object.entries.first().expect("nonempty object");
                    let mut quoted = String::new();
                    name.py_quoted_into(&mut quoted);
                    out.push_jstr(&JStr::from_text(&quoted));
                    out.push_jstr(&JStr::from_text(": "));
                    current = member;
                    if matches!(current, JVal::Str(_)) {
                        render_quoted(current, &mut out);
                    } else {
                        continue;
                    }
                }
            }
        }
        // The scalar (or just-closed container) at `current` is rendered;
        // advance through closers and separators.
        loop {
            let Some((parent, index)) = stack.pop() else {
                return Ok(out);
            };
            match parent {
                JVal::Array(items) => {
                    let next = index + 1;
                    if next < items.len() {
                        out.push_jstr(&JStr::from_text(", "));
                        stack.push((parent, next));
                        current = &items[next];
                        // Nested strings render repr-quoted.
                        if matches!(current, JVal::Str(_)) {
                            render_quoted(current, &mut out);
                            continue;
                        }
                        break;
                    }
                    out.push_jstr(&JStr::from_text("]"));
                    depth -= 1;
                }
                JVal::Object(object) => {
                    let next = index + 1;
                    if next < object.entries.len() {
                        out.push_jstr(&JStr::from_text(", "));
                        stack.push((parent, next));
                        let (name, member) = &object.entries[next];
                        let mut quoted = String::new();
                        name.py_quoted_into(&mut quoted);
                        out.push_jstr(&JStr::from_text(&quoted));
                        out.push_jstr(&JStr::from_text(": "));
                        current = member;
                        if matches!(current, JVal::Str(_)) {
                            render_quoted(current, &mut out);
                            continue;
                        }
                        break;
                    }
                    out.push_jstr(&JStr::from_text("}"));
                    depth -= 1;
                }
                _ => unreachable!("stack holds containers only"),
            }
        }
    }
}

/// Render a nested string value repr-quoted into `out`.
fn render_quoted(value: &JVal, out: &mut JStr) {
    let JVal::Str(text) = value else {
        unreachable!("quoted render takes strings");
    };
    let mut quoted = String::new();
    text.py_quoted_into(&mut quoted);
    out.push_jstr(&JStr::from_text(&quoted));
}

// ---------------------------------------------------------------------------
// Publish boundary: JVal back to serde (lossy for unknown-field dirt)
// ---------------------------------------------------------------------------

/// Convert a parsed value to a serde value for the jobs-crate publish
/// APIs (which take serde types): numbers keep their literal text
/// (arbitrary-precision, so overflow literals render exactly as today),
/// containers recurse, and surrogate-bearing strings become U+FFFD (see
/// [`JStr::to_lossy_string`]).
pub fn to_serde_publish(value: &JVal) -> serde_json::Value {
    // Iterative: request bodies nest to 9939 levels and this runs on a
    // 2MB tokio worker (a recursive walk aborts the process — found by
    // the 626 exotic differential on PATCH with a deep unknown key).
    enum Task<'v> {
        Emit(&'v JVal),
        SealArray(usize),
        SealObject(usize),
    }
    fn scalar(value: &JVal) -> serde_json::Value {
        match value {
            JVal::Null => serde_json::Value::Null,
            JVal::Bool(flag) => serde_json::Value::Bool(*flag),
            JVal::Num(number) => serde_json::from_str::<serde_json::Value>(number.text())
                .expect("parser-validated number literal"),
            JVal::Str(text) => serde_json::Value::String(text.to_lossy_string()),
            JVal::Array(_) | JVal::Object(_) => unreachable!("scalars only"),
        }
    }
    let mut stack = vec![Task::Emit(value)];
    // Finished child values, in completion order; each Seal pops its own.
    let mut done: Vec<serde_json::Value> = Vec::new();
    // Pending object keys parallel to `done` (keys complete with values).
    let mut keys: Vec<String> = Vec::new();
    while let Some(task) = stack.pop() {
        match task {
            Task::Emit(JVal::Array(items)) => {
                stack.push(Task::SealArray(items.len()));
                for item in items.iter().rev() {
                    stack.push(Task::Emit(item));
                }
            }
            Task::Emit(JVal::Object(object)) => {
                let entries: Vec<&(JStr, JVal)> = object.iter().collect();
                stack.push(Task::SealObject(entries.len()));
                for (name, member) in entries.iter().rev() {
                    keys.push(name.to_lossy_string());
                    stack.push(Task::Emit(member));
                }
                // Keys were pushed reversed; SealObject pops them back.
                let at = keys.len() - entries.len();
                keys[at..].reverse();
            }
            Task::Emit(leaf) => done.push(scalar(leaf)),
            Task::SealArray(count) => {
                let at = done.len() - count;
                let items: Vec<serde_json::Value> = done.drain(at..).collect();
                done.push(serde_json::Value::Array(items));
            }
            Task::SealObject(count) => {
                let at = done.len() - count;
                let values: Vec<serde_json::Value> = done.drain(at..).collect();
                let kat = keys.len() - count;
                let names: Vec<String> = keys.drain(kat..).collect();
                let mut map = serde_json::Map::with_capacity(count);
                for (name, member) in names.into_iter().zip(values) {
                    map.insert(name, member);
                }
                done.push(serde_json::Value::Object(map));
            }
        }
    }
    debug_assert!(keys.is_empty());
    debug_assert_eq!(done.len(), 1);
    done.pop().expect("root value")
}

/// Convert a parsed object to a serde map for `model_created_job` /
/// `model_updated_job` (see [`to_serde_publish`]).
pub fn to_serde_publish_map(object: &JObject) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::with_capacity(object.entries.len());
    for (name, member) in object.iter() {
        map.insert(name.to_lossy_string(), to_serde_publish(member));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_constants_reject_with_oracle_text() {
        for (raw, token) in [
            ("NaN", "NaN"),
            ("[Infinity]", "Infinity"),
            ("{\"a\": -Infinity}", "-Infinity"),
        ] {
            match parse_request_bytes(raw.as_bytes()) {
                Err(JsonFail::Message(detail)) => assert_eq!(
                    detail,
                    format!("Out of range float values are not JSON compliant: '{token}'"),
                    "{raw}"
                ),
                other => panic!("{raw}: expected strict message, got {other:?}"),
            }
        }
        // Delimiter-position constants and near-misses stay syntax errors.
        assert!(matches!(
            parse_request_bytes(b"{\"a\" NaN}"),
            Err(JsonFail::Message(detail)) if detail.starts_with("Expecting ':' delimiter")
        ));
        assert!(matches!(
            parse_request_bytes(b"{\"a\": +Infinity}"),
            Err(JsonFail::Message(detail)) if detail.starts_with("Expecting value")
        ));
    }

    #[test]
    fn surrogates_accept_with_exact_units() {
        let value = parse_request_bytes(b"{\"a\": \"\\ud800x\\udc00\"}").expect("accept");
        let JVal::Object(object) = &value else {
            panic!("object");
        };
        let field = object.get("a").expect("field");
        let JVal::Str(text) = field else {
            panic!("string");
        };
        assert!(text.has_surrogate());
        assert_eq!(text.len_chars(), 3);
        assert_eq!(text.first_surrogate(), Some(0xD800));
        assert_eq!(text.to_clean_string(), None);
        assert_eq!(text.to_lossy_string(), "\u{FFFD}x\u{FFFD}");
        // Astral pairs combine instead.
        let paired = parse_request_bytes(b"\"\\ud834\\udd1e\"").expect("astral");
        assert!(
            matches!(&paired, JVal::Str(text) if text.to_clean_string().as_deref() == Some("\u{1D11E}"))
        );
    }

    #[test]
    fn int_digit_limit_matches_oracle() {
        let ok = format!("{{\"a\": {}}}", "1".repeat(MAX_INT_DIGITS));
        assert!(parse_request_bytes(ok.as_bytes()).is_ok());
        let over = format!("{{\"a\": {}}}", "1".repeat(MAX_INT_DIGITS + 1));
        match parse_request_bytes(over.as_bytes()) {
            Err(JsonFail::Message(detail)) => assert_eq!(
                detail,
                format!(
                    "Exceeds the limit (4300 digits) for integer string conversion: value has {} digits; \
                     use sys.set_int_max_str_digits() to increase the limit",
                    MAX_INT_DIGITS + 1
                )
            ),
            other => panic!("expected int-limit message, got {other:?}"),
        }
        // Floats of any width parse (the limit is int tokens only).
        let wide = format!("{{\"a\": {}.0}}", "9".repeat(5000));
        assert!(parse_request_bytes(wide.as_bytes()).is_ok());
    }

    #[test]
    fn depth_caps_match_oracle() {
        let ok = format!("{{\"a\": {}1{}}}", "[".repeat(9938), "]".repeat(9938));
        assert!(parse_request_bytes(ok.as_bytes()).is_ok());
        let over = format!("{{\"a\": {}1{}}}", "[".repeat(9939), "]".repeat(9939));
        assert!(matches!(
            parse_request_bytes(over.as_bytes()),
            Err(JsonFail::Recursion)
        ));
        // str() renders 9937-deep, fails 9938-deep.
        let nested = |depth: usize| {
            let raw = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
            parse_request_bytes(raw.as_bytes()).expect("parseable")
        };
        assert!(py_str(&nested(9937)).is_ok());
        assert!(matches!(py_str(&nested(9938)), Err(StrDepthFail)));
    }

    #[test]
    fn repr_table_spot_checks() {
        // `'`-only strings take double quotes; controls take `\xXX`;
        // lone surrogates take lowercase `\uXXXX`.
        let mut out = String::new();
        JStr::from_text("a'b").py_quoted_into(&mut out);
        assert_eq!(out, "\"a'b\"");
        out.clear();
        JStr::from_text("a\x01b\x7fé").py_quoted_into(&mut out);
        assert_eq!(out, "'a\\x01b\\x7fé'");
        let mut dirty = JStr::from_text("[");
        dirty.push_surrogate(0xD800);
        dirty.push_jstr(&JStr::from_text("]"));
        out.clear();
        dirty.py_quoted_into(&mut out);
        assert_eq!(out, "'[\\ud800]'");
        assert_eq!(dirty.json_quoted(), "\"[\\ud800]\"");
        // Clean JSON quoting is serde-identical.
        assert_eq!(
            JStr::from_text("a\"\nb\x01é").json_quoted(),
            serde_json::to_string("a\"\nb\x01é").unwrap()
        );
    }

    #[test]
    fn object_duplicate_keys_last_win_first_position() {
        let value = parse_request_bytes(br#"{"b": 0, "a": 1, "a": 2}"#).expect("dup keys");
        let JVal::Object(object) = &value else {
            panic!("object");
        };
        let keys: Vec<String> = object
            .iter()
            .map(|(name, _)| name.to_clean_string().unwrap())
            .collect();
        assert_eq!(keys, vec!["b".to_owned(), "a".to_owned()]);
        assert!(matches!(object.get("a"), Some(JVal::Num(_))));
    }

    #[test]
    fn request_data_empty_and_tail_drop() {
        // Zero-length short-circuits to `{}` (DRF `_parse`).
        assert!(matches!(
            &parse_request_data(b""),
            Ok(JVal::Object(map)) if map.is_empty()
        ));
        // A trailing incomplete sequence parses as the truncated text.
        assert!(matches!(
            parse_request_data(b"{\"a\":1}\xc3"),
            Ok(JVal::Object(_))
        ));
        assert!(matches!(
            parse_request_data(b"\xed\xa0"),
            Err(JsonFail::Message(detail))
            if detail == "Expecting value: line 1 column 1 (char 0)"
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
    fn publish_conversion_survives_small_stack() {
        // The publish boundary converts whole request bodies on a 2MB
        // tokio worker: a recursive walk aborts the process (626 found
        // it via PATCH with a deep unknown key — Django 500s the eager
        // round-trip there, brokered Django 200s, so the port converts
        // and enqueues without a cap).
        let raw = format!("{{\"a\": {}1{}}}", "[".repeat(9938), "]".repeat(9938));
        let value = parse_request_bytes(raw.as_bytes()).expect("parseable");
        let back = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || to_serde_publish(&value))
            .expect("spawn")
            .join()
            .expect("no stack overflow");
        assert!(matches!(back, serde_json::Value::Object(_)));
    }

    #[test]
    fn error_margins_match_oracle() {
        // Every flip verified live through Django (base 9939): the
        // value family surfaces to 9938, the string/object/delimiter/
        // strict family to 9935; valid values are free at any depth.
        let at = |depth: usize, inner: &str| {
            let mut raw = vec![b'['; depth];
            raw.extend_from_slice(inner.as_bytes());
            parse_request_bytes(&raw)
        };
        assert!(matches!(at(9938, ","), Err(JsonFail::Message(_))));
        assert!(matches!(at(9939, ","), Err(JsonFail::Recursion)));
        assert!(matches!(at(9935, "NaN"), Err(JsonFail::Message(_))));
        assert!(matches!(at(9936, "NaN"), Err(JsonFail::Recursion)));
        assert!(matches!(at(9935, "\"a\\x\""), Err(JsonFail::Message(_))));
        assert!(matches!(at(9936, "\"a\\x\""), Err(JsonFail::Recursion)));
        assert!(matches!(at(9935, "1}"), Err(JsonFail::Message(_))));
        assert!(matches!(at(9936, "1}"), Err(JsonFail::Recursion)));
        let deep = format!("{{\"a\": {}1{}}}", "[".repeat(9938), "]".repeat(9938));
        assert!(parse_request_bytes(deep.as_bytes()).is_ok());
    }

    #[test]
    fn number_rendering_matches_oracle() {
        // `str(int)` canonicalizes the only non-canonical spelling;
        // `float()` saturates out-of-range literals (both verified live
        // through the oracle: `{"name": -0}` stores `"0"`, `1e400` echoes
        // `"inf"`).
        assert_eq!(JNum::int("-0".to_owned()).py_string(), "0");
        assert_eq!(JNum::int("0".to_owned()).py_string(), "0");
        assert_eq!(JNum::int("123".to_owned()).py_string(), "123");
        assert_eq!(JNum::float("1e400".to_owned()).py_string(), "inf");
        assert_eq!(JNum::float("1e-400".to_owned()).py_string(), "0.0");
        assert_eq!(JNum::float("1.5".to_owned()).py_string(), "1.5");
        let huge = "9".repeat(4300);
        assert_eq!(JNum::int(huge.clone()).py_string(), huge);
    }

    #[test]
    fn deep_values_drop_on_small_stack() {
        // Request values nest to the 9939 cap and drop on 2MB tokio
        // workers: the derived drop recurses per level and aborts, so
        // `Drop` dismantles iteratively (array and object chains — the
        // object shape is the fatter drop path).
        let array_doc = format!("{{\"a\": {}1{}}}", "[".repeat(9938), "]".repeat(9938));
        let object_doc = format!("{}{}{}", "{\"a\": ".repeat(9939), "1", "}".repeat(9939));
        for raw in [array_doc, object_doc] {
            let value = parse_request_bytes(raw.as_bytes()).expect("parseable");
            std::thread::Builder::new()
                .stack_size(2 * 1024 * 1024)
                .spawn(move || drop(value))
                .expect("spawn")
                .join()
                .expect("no stack overflow");
        }
    }
}
