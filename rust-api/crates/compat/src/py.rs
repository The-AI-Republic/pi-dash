//! Python repr/str/truthiness/number rendering (OVL-04 part 5, PIDASHCONV-1542).
//!
//! Canonical home of the `str()`/`repr()`/truthiness/number shapes.
//! This family is genuinely fragmented — `py_str` alone has 29
//! distinct bodies across 33 copies (some are different jobs wearing
//! the same name) — so only clusters proven identical move here:
//!
//! - moved: `py_str`/`py_repr`/`py_repr_str`/`py_num_str`/`py_float_str`/
//!   `py_printable` (the runner-sessions trio + `run_endpoints` float,
//!   19 defs), `py_float_repr` (3 defs), `layout_py_float` (2),
//!   `py_truthy` (4 defs across 2 textually-different but
//!   truth-table-identical bodies), `py_str_value` +
//!   `py_float_repr_probed` (the engage ChoiceField/UUID family,
//!   PIDASHCONV-1623), `render_number` + `py_json_float` (the exact
//!   JSON-number renderer, PIDASHCONV-1623), `py_strip` (all 23 defs: the
//!   spellings differ and three add a redundant `\u{85}`, which
//!   `char::is_whitespace` already covers) + `py_is_space` (4),
//!   `py_json_string` (7), `python_dumps` + `python_dump_into` +
//!   `python_dump_str` (5 each), `py_repr_string` (2), `py_bool` (2),
//!   `py_uuid_or_none` (the `None`-rendering `opt_uuid` pair),
//!   `django_dumps` + `dumps_value` + `dumps_escape` + `dumps_number`
//!   (the iterative `json.dumps`, moved from
//!   `pidash_jobs::tasks_webhooks::activity_dispatch` by OVL-14,
//!   PIDASHCONV-1649; the jobs module re-exports it);
//! - kept local: the `shape`/`shape_relations` str/repr set (escapes
//!   `\u{2028}` through, own float algorithm), the invites str/repr
//!   set (same `\u{2028}` divergence plus inf/nan number handling),
//!   the webhooks `py_str` (JSON-array rendering), the invites
//!   `py_truthy` (`None`-means-inf default), the naive/partial
//!   `py_repr` pairs, `py_str_repr` x2+x2, the divergent `dumps`
//!   triples, and every singleton (`py_int`, the license
//!   `py_str_value` pair (different shapes), `py_repr_value`,
//!   `is_truthy` x12 — twelve distinct bodies, `py_number_str`,
//!   `python_int`, the `types` `render_number`, `py_dumps`,
//!   `py_title`, `*_is_falsy`).

use icu_properties::{props::GeneralCategory, CodePointMapData};
use serde_json::Value;
use uuid::Uuid;

/// Python `str()` over a JSON value.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => py_num_str(n),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// Python `repr()` over a JSON value.
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::String(s) => py_repr_str(s),
        other => py_str(other),
    }
}

/// Python `repr()` of a string: single quotes unless the text holds a
/// lone `'`, backslash escapes, non-printables as `\x`/`\u`/`\U`.
pub fn py_repr_str(s: &str) -> String {
    let use_double = s.contains('\'') && !s.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        if c == quote {
            out.push('\\');
            out.push(c);
        } else {
            match c {
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if py_printable(c) => out.push(c),
                c if (c as u32) < 0x100 => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c if (c as u32) < 0x1_0000 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => {
                    out.push_str(&format!("\\U{:08x}", c as u32));
                }
            }
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a string, minimal-escaping variant: only the
/// quote/backslash/`\n`/`\r`/`\t` escapes plus sub-`0x20` `\x` escapes;
/// everything else (including `\x7f`, other controls and `\u{2028}`)
/// passes through. Kept distinct from [`py_repr_str`] on purpose.
pub fn py_repr_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python number rendering: integers plain, floats via [`py_float_str`].
pub fn py_num_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    n.as_f64()
        .map(py_float_str)
        .unwrap_or_else(|| n.to_string())
}

/// Python `repr()` of a float, shortest-round-trip form: Rust's `{:?}`
/// with the exponent normalized to a signed two digits (`1e16` →
/// `1e+16`), `nan`/`inf` spelled the Python way.
pub fn py_float_str(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_owned();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    let rust = format!("{f:?}");
    let Some(pos) = rust.find('e') else {
        return rust;
    };
    let (mantissa, exp) = rust.split_at(pos);
    let exp = &exp[1..];
    let (sign, digits) = match exp.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("+", exp.strip_prefix('+').unwrap_or(exp)),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

/// Python `repr()` of a float, ryu-shortest-digits form with explicit
/// fixed-vs-exponential layout (`dec_exp` in `-3..=16` renders fixed).
/// A separate implementation from [`py_float_str`], kept under its own
/// name; both spell `nan`/`inf` the Python way.
pub fn py_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let negative = value.is_sign_negative();
    let abs = value.abs();
    if abs == 0.0 {
        return if negative { "-0.0" } else { "0.0" }.to_string();
    }
    // Shortest digits: `1.5`, `100.0`, `1e20`, `1.5e-05` shapes.
    let ryu = serde_json::Number::from_f64(abs)
        .expect("finite")
        .to_string();
    let (mantissa, exp): (&str, i32) = match ryu.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse().expect("ryu exponent")),
        None => (ryu.as_str(), 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len());
    let mut digits: Vec<char> = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    while digits.len() > 1 && digits[0] == '0' {
        digits.remove(0);
    }
    let after_point = mantissa.len() - point - usize::from(point < mantissa.len());
    // Value = digits × 10^(exp - after_point) = 0.digits × 10^dec_exp.
    let dec_exp = exp - after_point as i32 + digits.len() as i32;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-3..=16).contains(&dec_exp) {
        // Fixed notation.
        if dec_exp <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-dec_exp) as usize));
            out.extend(digits);
        } else if dec_exp as usize >= digits.len() {
            let pad = dec_exp as usize - digits.len();
            out.extend(digits);
            out.push_str(&"0".repeat(pad));
            out.push_str(".0");
        } else {
            let split = dec_exp as usize;
            out.extend(digits[..split].iter());
            out.push('.');
            out.extend(digits[split..].iter());
        }
    } else {
        // Exponential notation.
        out.push(digits[0]);
        if digits.len() > 1 {
            out.push('.');
            out.extend(digits[1..].iter());
        }
        let exp10 = dec_exp - 1;
        out.push('e');
        out.push(if exp10 < 0 { '-' } else { '+' });
        let mag = exp10.unsigned_abs().to_string();
        if mag.len() < 2 {
            out.push('0');
        }
        out.push_str(&mag);
    }
    out
}

/// Lay out `rendered` (a shortest-digits float rendering) with `value`'s
/// magnitude deciding plain (`1e-4..1e16`) vs `d[.ddd]e±XX` layout.
pub fn layout_py_float(rendered: &str, value: f64) -> String {
    let (mantissa, exp) = match rendered.split_once(['e', 'E']) {
        Some((mantissa, exp)) => (mantissa, exp.parse::<i32>().unwrap_or(0)),
        None => (rendered, 0),
    };
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|ch| ch.is_ascii_digit()).collect();
    let digits = digits.trim_start_matches('0');
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // Decimal exponent of `0.digits × 10^E`.
    let point_at = mantissa
        .find('.')
        .map(|pos| pos - usize::from(negative))
        .unwrap_or_else(|| mantissa.len() - usize::from(negative));
    let exp10 = exp + point_at as i32;
    let abs = value.abs();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (1e-4..1e16).contains(&abs) {
        // Plain layout.
        if exp10 <= 0 {
            out.push_str("0.");
            out.push_str(&"0".repeat((-exp10) as usize));
            out.push_str(digits);
        } else if exp10 as usize >= digits.len() {
            out.push_str(digits);
            out.push_str(&"0".repeat(exp10 as usize - digits.len()));
            out.push_str(".0");
        } else {
            let at = exp10 as usize;
            out.push_str(&digits[..at]);
            out.push('.');
            out.push_str(&digits[at..]);
        }
        return out;
    }
    // Exponent layout: `d[.ddd]e±XX`.
    let head: String = digits.chars().take(1).collect();
    let tail: String = digits.chars().skip(1).collect();
    out.push_str(&head);
    if !tail.is_empty() {
        out.push('.');
        out.push_str(&tail);
    }
    let exp = exp10 - 1;
    out.push('e');
    out.push(if exp < 0 { '-' } else { '+' });
    let mag = exp.abs().to_string();
    if mag.len() < 2 {
        out.push('0');
    }
    out.push_str(&mag);
    out
}

/// Python printability for `repr()` escaping: a char is printable
/// unless its Unicode general category is Other (`Cc`/`Cf`/`Cs`/
/// `Co`/`Cn`) or Separator (`Zl`/`Zp`/`Zs`) — except the ASCII
/// space, which stays printable. This is CPython
/// `str.isprintable` per char (PIDASHCONV-1624; the rule holds
/// with zero mismatches against the 3.12 interpreter over the
/// full BMP plus astral `E0000`–`E0FFF`/`F0000`–`10FFFF`).
/// Categories come from the linked ICU4X tables (Unicode 16), so a
/// code point assigned after CPython 3.12's Unicode 15 prints raw
/// here while 3.12 escapes it — no caller input is near that
/// boundary. (`Surrogate` is unreachable: Rust `char` never holds
/// one; it is listed so the category rule reads complete.)
pub fn py_printable(c: char) -> bool {
    if c == ' ' {
        return true;
    }
    !matches!(
        CodePointMapData::<GeneralCategory>::new().get(c),
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::Surrogate
            | GeneralCategory::PrivateUse
            | GeneralCategory::Unassigned
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
            | GeneralCategory::SpaceSeparator
    )
}

/// Python truthiness over a JSON value.
pub fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python `bool` spelling.
pub fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

/// Whether `str.strip()` removes `ch`: Unicode whitespace plus the
/// U+001C–U+001F control block Python strips but `is_whitespace`
/// does not cover.
pub fn py_is_space(ch: char) -> bool {
    ch.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&ch)
}

/// Python `str.strip()` (no args): strip [`py_is_space`] from both ends.
pub fn py_strip(text: &str) -> &str {
    text.trim_matches(py_is_space)
}

/// `json.dumps` string escaping (`ensure_ascii`): C escapes, `\uXXXX`
/// for other controls, and `\uXXXX` (surrogate pairs past the BMP) for
/// everything non-ASCII.
pub fn py_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// `json.dumps` with CPython's default separators (`", "` / `": "`).
pub fn python_dumps(value: &Value) -> String {
    let mut out = String::new();
    python_dump_into(&mut out, value);
    out
}

/// `python_dumps` worker: JSON with `", "`/`": "` separators.
pub fn python_dump_into(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => python_dump_str(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_into(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (index, (key, item)) in map.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                python_dump_str(out, key);
                out.push_str(": ");
                python_dump_into(out, item);
            }
            out.push('}');
        }
    }
}

/// `python_dumps` string half: `ensure_ascii` escaping with manual
/// surrogate-pair splitting past the BMP.
pub fn python_dump_str(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 || (ch as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) > 0x7E => {
                let code = ch as u32;
                if code > 0xFFFF {
                    let v = code - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (v >> 10),
                        0xDC00 + (v & 0x3FF)
                    ));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Python `repr()` of an optional UUID: the hyphenated id, or `None`.
pub fn py_uuid_or_none(id: &Option<Uuid>) -> String {
    id.map_or_else(|| "None".to_owned(), |id| id.to_string())
}

// ---------------------------------------------------------------------------
// CPython `str()` over parsed JSON, precision-probe floats (PIDASHCONV-1623)
// ---------------------------------------------------------------------------

/// CPython `str()` over a parsed JSON value: `None` / `True` / `False`,
/// decimal ints, [`py_float_repr_probed`] floats, raw strings, and — for
/// containers — brackets with `repr()` elements.
///
/// Moved from `api::app_issues::handlers_engage` (PIDASHCONV-1623),
/// which uses it for `ChoiceField` membership input and UUID-error
/// rendering. This is NOT [`py_str`]: the float arm probes precisions
/// for the shortest round-trip rendering instead of Rust's `{:?}`
/// (the two agree on every finite float probed — 300k random patterns
/// plus edges — but that sampling is not an all-inputs proof, so the
/// forms stay separate). Container strings escape non-printables per
/// [`py_printable`], like [`py_repr_str`] (PIDASHCONV-1644).
pub fn py_str_value(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int.to_string()
            } else if let Some(uint) = number.as_u64() {
                uint.to_string()
            } else if number.is_f64() {
                match number.as_f64() {
                    Some(float) => py_float_repr_probed(float),
                    None => number.to_string(),
                }
            } else {
                // Arbitrary-precision integers past `u64`: full digits,
                // like CPython `str(int)` (never the lossy `as_f64`).
                number.to_string()
            }
        }
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_str_value_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_str_value_repr_str(key),
                        py_str_value_repr(item)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython `repr(float)`: the shortest digit string that round-trips,
/// found by probing precisions `1..=19` (like the OSS port), laid out
/// per the magnitude rule in [`layout_py_float`]. `nan`/`inf` spelled
/// the Python way.
///
/// Moved from `api::app_issues::handlers_engage` (PIDASHCONV-1623).
/// Kept under its own name next to [`py_float_repr`] (the ryu form):
/// same outputs on every input probed, different algorithm.
pub fn py_float_repr_probed(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    for precision in 1..=19usize {
        let rendered = format!("{value:.dec$e}", dec = precision - 1);
        if rendered.parse::<f64>().ok() == Some(value) {
            return layout_py_float(&rendered, value);
        }
    }
    // Unreachable in practice (17 significant digits always identify
    // an f64); fall back to the longest rendering laid out.
    layout_py_float(&format!("{value:.18e}"), value)
}

/// CPython `repr()` over a parsed JSON value (container elements and dict
/// keys use this; see [`py_str_value`]).
fn py_str_value_repr(value: &Value) -> String {
    match value {
        Value::String(text) => py_str_value_repr_str(text),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_str_value_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_str_value_repr_str(key),
                        py_str_value_repr(item)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        other => py_str_value(other),
    }
}

/// CPython `repr()` of a string (see [`py_str_value`]): single quotes
/// unless the text holds one but no double quote, short escapes plus
/// `\xhh` / `\uhhhh` / `\Uhhhhhhhh` for non-printables. Printability
/// is [`py_printable`], shared with [`py_repr_str`] (PIDASHCONV-1644).
fn py_str_value_repr_str(text: &str) -> String {
    let use_double = text.contains('\'') && !text.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        if ch == quote {
            out.push('\\');
            out.push(ch);
            continue;
        }
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if !py_printable(ch) => {
                let code = ch as u32;
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code < 0x10000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

// ---------------------------------------------------------------------------
// Exact JSON numbers (PIDASHCONV-1623)
// ---------------------------------------------------------------------------

/// Exact JSON-number rendering into `out`: integers print full digits
/// (arbitrary-precision past `u64` included — never the lossy `as_f64`
/// scientific form [`py_num_str`] produces there), floats via
/// [`py_json_float`].
///
/// Canonicalized from the overlay (`cloud-api/src/mcp/protocol.rs`,
/// PIDASHCONV-1623), which is the Python-correct form: `json.dumps`
/// prints big ints exactly, and the previous OSS copy in
/// `jobs::tasks_webhooks::webhook_send` printed them scientifically
/// (e.g. `2**70` as `1.1805916207174113e+21`). The webhook sender now
/// calls this. QUIRK (shared, pre-existing, out of scope): a
/// float-overflow spelling like `1e999` renders as `1e+999` (the
/// normalized input spelling), where Python prints `Infinity`.
pub fn render_number(out: &mut String, number: &serde_json::Number) {
    if let Some(int) = number.as_i64() {
        out.push_str(&int.to_string());
    } else if let Some(int) = number.as_u64() {
        out.push_str(&int.to_string());
    } else if let Some(float) = number.as_f64().filter(|_| number.is_f64()) {
        out.push_str(&py_json_float(float));
    } else {
        // Arbitrary-precision integer beyond u64 (the workspace unifies
        // `arbitrary_precision`): `as_f64` still reports `Some` for these,
        // hence the `is_f64` guard above. Exact digits, like Python's
        // unbounded ints — never scientific notation.
        out.push_str(&number.to_string());
    }
}

/// CPython `repr` for a float, exactly: Rust's `{:?}` picks the same
/// fixed-vs-scientific shape and the same shortest digits in every case
/// probed (300k random bit patterns plus magnitude boundaries), differing
/// only in exponent spelling (`e16` vs `e+16`, `e-5` vs `e-05`) and in the
/// round-half-even tie-break when the exact binary value sits exactly
/// halfway between two shortest candidates (Rust rounds the last digit up,
/// CPython rounds to even). Non-finite values follow `json.dumps` with the
/// default `allow_nan` (`Infinity`, `-Infinity`, `NaN`).
///
/// Canonicalized from the overlay (`cloud-api/src/mcp/protocol.rs`,
/// PIDASHCONV-1623) with [`render_number`]; previously named
/// `python_float_repr` there.
pub fn py_json_float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        };
    }
    let debug = format!("{value:?}");
    let normalized = normalize_float_exponent(&debug);
    even_halfway_tie(&normalized, value).unwrap_or(normalized)
}

/// Rewrite a Rust `{:?}` exponent to CPython spelling: always signed, at
/// least two digits (`e16` to `e+16`, `e-5` to `e-05`).
fn normalize_float_exponent(debug: &str) -> String {
    let Some(exp_at) = debug.find('e') else {
        return debug.to_owned();
    };
    let (mantissa, exponent) = debug.split_at(exp_at);
    let exponent = exponent.strip_prefix('e').unwrap_or(exponent);
    let (sign, digits) = match exponent.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("+", exponent.strip_prefix('+').unwrap_or(exponent)),
    };
    let mut out = String::with_capacity(debug.len() + 2);
    out.push_str(mantissa);
    out.push('e');
    out.push_str(sign);
    for _ in digits.len()..2 {
        out.push('0');
    }
    out.push_str(digits);
    out
}

/// If `rendered` ends in an odd digit, the value is exactly halfway between
/// `rendered` and its last-digit-decremented sibling, and the sibling still
/// parses to `value`, return the sibling (CPython's round-half-even pick).
/// Otherwise return `None` to keep `rendered`.
///
/// Exactness without a bignum: a halfway case forces `|exp10| <= 24`
/// (otherwise `5^|exp10|` cannot divide the 53-bit mantissa or the
/// 17-digit decimal), so the midpoint equation fits in `u128` after
/// dividing out the shared power of five; see the inline derivation.
fn even_halfway_tie(rendered: &str, value: f64) -> Option<String> {
    let body = rendered.strip_prefix('-').unwrap_or(rendered);
    let (mantissa, exp_text) = match body.split_once('e') {
        Some((mantissa, exp_text)) => (mantissa, exp_text),
        None => (body, "+00"),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some(parts) => parts,
        None => (mantissa, ""),
    };
    let full_digits = format!("{int_part}{frac_part}");
    let mut digits = full_digits.clone();
    let last = digits.pop()?;
    if !matches!(last, '1' | '3' | '5' | '7' | '9') {
        return None;
    }
    let explicit_exp: i32 = exp_text.parse().ok()?;
    let exp10 = explicit_exp - i32::try_from(frac_part.len()).ok()?;
    if exp10.abs() > 24 {
        return None;
    }
    // Candidate sibling: same shape, last digit decremented (no borrow is
    // possible from an odd digit, so the digit count is unchanged).
    let decremented = char::from_digit(last.to_digit(10)? - 1, 10)?;
    let mut sibling = String::with_capacity(rendered.len());
    if rendered.starts_with('-') {
        sibling.push('-');
    }
    sibling.push_str(int_part);
    if !frac_part.is_empty() {
        sibling.push('.');
        sibling.push_str(&frac_part[..frac_part.len() - 1]);
        sibling.push(decremented);
    } else {
        sibling.push_str(&digits);
        sibling.push(decremented);
    }
    if body.contains('e') {
        sibling.push('e');
        sibling.push_str(exp_text);
    }
    if sibling.parse::<f64>().ok()? != value {
        return None;
    }
    // Exact halfway check. R = D * 10^exp10 with D the digit value;
    // M = (D - 1/2) * 10^exp10; v = m * 2^e. Halfway (v == M) forces
    // 5^|exp10| to divide m (exp10 >= 0) or 2D-1 (exp10 < 0); dividing it
    // out keeps every term in u128 (D < 1e17, m < 2^53, 5^24 < 2^56).
    let bits = value.to_bits();
    let mantissa_bits = bits & ((1 << 52) - 1);
    let biased_exp = ((bits >> 52) & 0x7FF) as i32;
    let (m, e) = if biased_exp == 0 {
        (mantissa_bits, -1074)
    } else {
        (mantissa_bits | (1 << 52), biased_exp - 1075)
    };
    let digit_value: u128 = full_digits.parse::<u128>().ok()?;
    if !is_exact_midpoint(digit_value, exp10, m, e) {
        return None;
    }
    Some(sibling)
}

/// Decide `v == M` exactly, where `v = m * 2^e` (decoded double bits) and
/// `M = (D - 1/2) * 10^exp10` (midpoint below the odd rendering `R`).
fn is_exact_midpoint(digit_value: u128, exp10: i32, m: u64, e: i32) -> bool {
    if exp10 >= 0 {
        let Some(pow5) = 5u128.checked_pow(exp10 as u32) else {
            return false;
        };
        let m = m as u128;
        if !m.is_multiple_of(pow5) {
            return false;
        }
        let reduced = m / pow5;
        let target = 2 * digit_value - 1;
        let shift = e - exp10 + 1;
        if shift >= 0 {
            let Ok(shift) = u32::try_from(shift) else {
                return false;
            };
            reduced.checked_shl(shift) == Some(target)
        } else {
            let Ok(shift) = u32::try_from(-shift) else {
                return false;
            };
            target.checked_shl(shift) == Some(reduced)
        }
    } else {
        let Some(pow5) = 5u128.checked_pow((-exp10) as u32) else {
            return false;
        };
        let odd = 2 * digit_value - 1;
        if !odd.is_multiple_of(pow5) {
            return false;
        }
        let target = odd / pow5;
        let shift = e + 1 - exp10;
        let m = m as u128;
        if shift >= 0 {
            let Ok(shift) = u32::try_from(shift) else {
                return false;
            };
            m.checked_shl(shift) == Some(target)
        } else {
            let Ok(shift) = u32::try_from(-shift) else {
                return false;
            };
            target.checked_shl(shift) == Some(m)
        }
    }
}

// ---------------------------------------------------------------------------
// `django_dumps`: moved verbatim from
// `pidash_jobs::tasks_webhooks::activity_dispatch` by OVL-14
// (PIDASHCONV-1649) — the operations in `pidash-services` need it and
// cannot depend on `pidash-jobs`. The jobs module re-exports it, so
// every existing path (including the jobs unit tests) runs this copy.
// Emits the same bytes as `python_dumps` above; the iterative walk
// (explicit stack instead of recursion) is what differs.
// ---------------------------------------------------------------------------

/// Escape one string exactly like `json.dumps` (`ensure_ascii=True`,
/// lowercase `\uXXXX`): control escapes, quotes, backslash, then
/// non-ASCII as `\uXXXX` with surrogate pairs above BMP.
fn dumps_escape(into: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '"' => into.push_str("\\\""),
            '\\' => into.push_str("\\\\"),
            '\n' => into.push_str("\\n"),
            '\r' => into.push_str("\\r"),
            '\t' => into.push_str("\\t"),
            '\u{08}' => into.push_str("\\b"),
            '\u{0C}' => into.push_str("\\f"),
            c if (c as u32) < 0x20 => into.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x7F => into.push(c),
            c => {
                let code = c as u32;
                if code < 0x10000 {
                    into.push_str(&format!("\\u{code:04x}"));
                } else {
                    // Surrogate pair, like CPython's `ensure_ascii`.
                    let v = code - 0x10000;
                    into.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (v >> 10),
                        0xDC00 + (v & 0x3FF)
                    ));
                }
            }
        }
    }
}

/// Python `repr()` of a float as `json.dumps` renders it: `repr(f)`
/// shortest-roundtrip. `ryu` (via `serde_json`) prints the same digits
/// for finite values; non-finite floats never occur in this payload
/// (Django would render `Infinity`/`NaN` bare tokens the same way).
fn dumps_number(into: &mut String, value: &serde_json::Number) {
    into.push_str(&value.to_string());
}

/// `json.dumps(obj, cls=DjangoJSONEncoder)` with default separators
/// (`", "` / `": "`, `ensure_ascii=True`): callers pass trees of
/// strings, numbers, booleans and nulls (every datetime, UUID and
/// Decimal is pre-rendered), so no encoder hooks are needed.
pub fn django_dumps(value: &Value) -> String {
    let mut out = String::new();
    dumps_value(&mut out, value);
    out
}

fn dumps_value(into: &mut String, value: &Value) {
    // Iterative over an explicit stack: callers may pass values nested
    // to the 9939 request cap (the cycle add path echoes raw `issues`),
    // and the recursive walk overflows 2MB workers. Output is unchanged
    // (same scalars, separators and key order).
    enum Task<'v> {
        Emit(&'v Value),
        Key(&'v str),
        Lit(&'static str),
        Seal(char),
    }
    fn scalar(into: &mut String, value: &Value) {
        match value {
            Value::Null => into.push_str("null"),
            Value::Bool(true) => into.push_str("true"),
            Value::Bool(false) => into.push_str("false"),
            Value::Number(n) => dumps_number(into, n),
            Value::String(s) => {
                into.push('"');
                dumps_escape(into, s);
                into.push('"');
            }
            Value::Array(_) | Value::Object(_) => unreachable!("scalars only"),
        }
    }
    let mut stack = vec![Task::Emit(value)];
    while let Some(task) = stack.pop() {
        match task {
            Task::Emit(Value::Array(items)) => {
                into.push('[');
                stack.push(Task::Seal(']'));
                for (index, item) in items.iter().rev().enumerate() {
                    if index > 0 {
                        stack.push(Task::Lit(", "));
                    }
                    stack.push(Task::Emit(item));
                }
            }
            Task::Emit(Value::Object(map)) => {
                into.push('{');
                stack.push(Task::Seal('}'));
                for (index, (key, item)) in map.iter().rev().enumerate() {
                    if index > 0 {
                        stack.push(Task::Lit(", "));
                    }
                    stack.push(Task::Emit(item));
                    stack.push(Task::Lit("\": "));
                    stack.push(Task::Key(key));
                }
            }
            Task::Emit(leaf) => scalar(into, leaf),
            Task::Key(key) => {
                // Opening quote plus text; the `": "` literal carries the
                // closing quote, exactly like the recursive original.
                into.push('"');
                dumps_escape(into, key);
            }
            Task::Lit(text) => into.push_str(text),
            Task::Seal(closer) => into.push(closer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::str::FromStr;

    // Moved alongside `py_str` from `runner_sessions/runner_open.rs`.
    #[test]
    fn py_str_vectors() {
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&Value::Bool(true)), "True");
        assert_eq!(py_str(&Value::Bool(false)), "False");
        assert_eq!(py_str(&serde_json::json!(5)), "5");
        assert_eq!(py_str(&serde_json::json!(-42)), "-42");
        assert_eq!(py_str(&serde_json::json!("CT00003")), "CT00003");
        assert_eq!(py_str(&serde_json::json!(1.5)), "1.5");
        assert_eq!(py_str(&serde_json::json!(1e16)), "1e+16");
        assert_eq!(
            py_str(&serde_json::json!([1, "a", true, null])),
            "[1, 'a', True, None]"
        );
        assert_eq!(py_str(&serde_json::json!({"k": 1})), "{'k': 1}");
        assert_eq!(py_repr_str("it\"s"), "'it\"s'");
        assert_eq!(py_repr_str("it's"), "\"it's\"");
        assert_eq!(py_repr_str("a'b\"c"), "'a\\'b\"c'");
    }

    // Moved alongside `py_repr_str` from `runner_runs/mod.rs`.
    #[test]
    fn py_repr_str_quotes_and_escapes() {
        assert_eq!(py_repr_str("abc"), "'abc'");
        assert_eq!(py_repr_str("x's"), "\"x's\"");
        assert_eq!(py_repr_str("say \"hi\""), "'say \"hi\"'");
        assert_eq!(py_repr_str("a'b\"c"), "'a\\'b\"c'");
        assert_eq!(py_repr_str("a\nb\tc\\d"), "'a\\nb\\tc\\\\d'");
        assert_eq!(py_repr_str("\u{1}"), "'\\x01'");
        assert_eq!(py_repr_str("\u{7f}"), "'\\x7f'");
        assert_eq!(py_repr_str("—"), "'—'");
        // The line boundaries escape (the shape/invites variants pass
        // them through — that divergence stays local).
        assert_eq!(py_repr_str("\u{2028}"), "'\\u2028'");
        assert_eq!(py_repr_str("\u{2029}"), "'\\u2029'");
    }

    // PIDASHCONV-1624: printability follows CPython `str.isprintable`
    // (general category C*/Z* except U+0020). Every vector below was
    // verified against the 3.12 interpreter; the rule itself swept
    // clean over the full BMP plus astral E0000-E0FFF/F0000-10FFFF.
    #[test]
    fn py_repr_str_escapes_cpython_nonprintables() {
        // The issue's vectors: nbsp, unassigned, private-use.
        assert_eq!(py_repr_str("\u{a0}"), "'\\xa0'");
        assert_eq!(py_repr_str("\u{378}"), "'\\u0378'");
        assert_eq!(py_repr_str("\u{e000}"), "'\\ue000'");
        // One escape per remaining category: Cf (soft hyphen, ZWJ,
        // BOM), Zs (ogham space, en quad), astral Co/Cn.
        assert_eq!(py_repr_str("\u{ad}"), "'\\xad'");
        assert_eq!(py_repr_str("\u{200d}"), "'\\u200d'");
        assert_eq!(py_repr_str("\u{feff}"), "'\\ufeff'");
        assert_eq!(py_repr_str("\u{1680}"), "'\\u1680'");
        assert_eq!(py_repr_str("\u{2000}"), "'\\u2000'");
        assert_eq!(py_repr_str("\u{f0000}"), "'\\U000f0000'");
        assert_eq!(py_repr_str("\u{e0000}"), "'\\U000e0000'");
        // Printable near-misses stay raw: the ASCII space, U+0377
        // (Ll, next to unassigned U+0378), U+FE00 (Mn variation
        // selector), accented letters, emoji, the empty string.
        assert_eq!(py_repr_str(" "), "' '");
        assert_eq!(py_repr_str("\u{377}"), "'\u{377}'");
        assert_eq!(py_repr_str("\u{fe00}"), "'\u{fe00}'");
        assert_eq!(py_repr_str("é"), "'é'");
        assert_eq!(py_repr_str("😀"), "'😀'");
        assert_eq!(py_repr_str(""), "''");
        // NUL + CR: the overlay's `repr_string_matches_cpython_quoting`
        // pins these through its own copy today; the OVL-10 SDK swap
        // (PIDASHCONV-1548) re-pins them onto this function.
        assert_eq!(py_repr_str("\u{0}"), "'\\x00'");
        assert_eq!(py_repr_str("a\nb\tc\rd"), "'a\\nb\\tc\\rd'");
    }

    #[test]
    fn py_repr_string_is_the_minimal_variant() {
        assert_eq!(py_repr_string("abc"), "'abc'");
        assert_eq!(py_repr_string("a'b"), "\"a'b\"");
        assert_eq!(py_repr_string("a\nb"), "'a\\nb'");
        // Only sub-0x20 escapes here: DEL, other controls and U+2028
        // pass through (divergent from `py_repr_str` on purpose).
        assert_eq!(py_repr_string("\u{7f}"), "'\u{7f}'");
        assert_eq!(py_repr_string("\u{2028}"), "'\u{2028}'");
        assert_eq!(py_repr_string("\u{85}"), "'\u{85}'");
    }

    // Moved alongside `python_dumps` from `app_issues/handlers_relations.rs`.
    #[test]
    fn python_dumps_matches_separators_and_ascii() {
        let value = serde_json::json!({"b": [1, true, null], "a": "é"});
        assert_eq!(
            python_dumps(&value),
            r#"{"b": [1, true, null], "a": "\u00e9"}"#
        );
        // Astral planes escape as surrogate pairs.
        assert_eq!(python_dumps(&serde_json::json!("𝄞")), r#""\ud834\udd1e""#);
        // Controls escape short or long.
        assert_eq!(
            python_dumps(&serde_json::json!("a\nb\tc\x01")),
            r#""a\nb\tc\u0001""#
        );
    }

    // Moved alongside `python_dumps` from `app_issues/handlers_engage.rs`.
    #[test]
    fn python_dumps_matches_cpython() {
        let value = json!({"b": [1, "x\ny"], "a": Value::Null, "u": "café"});
        assert_eq!(
            python_dumps(&value),
            r#"{"b": [1, "x\ny"], "a": null, "u": "caf\u00e9"}"#
        );
        assert_eq!(
            python_dumps(&json!({"comment_id": "abc"})),
            r#"{"comment_id": "abc"}"#
        );
    }

    // Moved alongside `python_dumps` from `space/social.rs`.
    #[test]
    fn python_dumps_matches_cpython_separators() {
        let value = serde_json::json!({"a": 1, "b": "<p>hi</p>", "c": "héllo"});
        assert_eq!(
            python_dumps(&value),
            r#"{"a": 1, "b": "<p>hi</p>", "c": "h\u00e9llo"}"#
        );
        assert_eq!(
            python_dumps(&serde_json::json!([1, true, None::<()>])),
            "[1, true, null]"
        );
    }

    // Moved alongside `py_json_string` from `space/assets.rs`.
    #[test]
    fn py_json_string_matches_cpython_escapes() {
        assert_eq!(py_json_string("shot.png"), "\"shot.png\"");
        assert_eq!(py_json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(py_json_string("é"), "\"\\u00e9\"");
        assert_eq!(py_json_string("𝄞"), "\"\\ud834\\udd1e\"");
    }

    // Moved alongside `py_bool` from `tasks_ticker/fire_tick.rs`.
    #[test]
    fn log_bools_match_python_capitalisation() {
        assert_eq!(py_bool(true), "True");
        assert_eq!(py_bool(false), "False");
    }

    #[test]
    fn strip_covers_nel_and_control_block() {
        assert_eq!(py_strip("  x  "), "x");
        assert_eq!(py_strip(""), "");
        // U+001C–U+001F: outside `is_whitespace`, stripped explicitly.
        assert_eq!(py_strip("\u{1c}x\u{1f}"), "x");
        // NEL is `White_Space`: `is_whitespace` already covers it, so the
        // three spellings (plain, `+ \u{85}`, `py_is_space`) agree.
        assert!(py_is_space('\u{85}'));
        assert_eq!(py_strip("\u{85}x\u{85}"), "x");
        assert_eq!(py_strip("\u{a0}x"), "x");
        assert!(!py_is_space('x'));
    }

    #[test]
    fn truthy_vectors() {
        for falsy in [
            json!(null),
            json!(false),
            json!(0),
            json!(0.0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(!py_truthy(&falsy), "{falsy}");
        }
        for truthy in [
            json!(true),
            json!(1),
            json!(-7),
            json!(0.5),
            json!("x"),
            json!("false"),
            json!([0]),
            json!({"a": 0}),
        ] {
            assert!(py_truthy(&truthy), "{truthy}");
        }
        // Non-finite floats are unrepresentable (`Number::from_f64`
        // rejects them; `json!(inf)` is `Null`), so the `None` arm of
        // the float check is dead — the `is_some_and` and
        // `map+unwrap_or(false)` spellings agree everywhere.
        assert!(!py_truthy(&json!(f64::INFINITY)));
    }

    #[test]
    fn float_repr_vectors_match_oracle() {
        // Oracle: CPython `repr()` on the same doubles.
        for (value, expected) in [
            (1.5, "1.5"),
            (100.0, "100.0"),
            (1e16, "1e+16"),
            (0.1, "0.1"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1.5e-5, "1.5e-05"),
            (123456.0, "123456.0"),
            (1e20, "1e+20"),
            (std::f64::consts::PI, "3.141592653589793"),
            (2.5e-7, "2.5e-07"),
        ] {
            assert_eq!(py_float_repr(value), expected, "{value}");
            assert_eq!(py_float_str(value), expected, "{value}");
        }
        assert_eq!(py_float_repr(-0.0), "-0.0");
        assert_eq!(py_float_str(f64::NAN), "nan");
        assert_eq!(py_float_str(f64::INFINITY), "inf");
        assert_eq!(py_float_str(f64::NEG_INFINITY), "-inf");
        // `layout_py_float` agrees with the oracle on shortest inputs.
        assert_eq!(layout_py_float("1.5", 1.5), "1.5");
        assert_eq!(layout_py_float("1e16", 1e16), "1e+16");
        assert_eq!(layout_py_float("1e-05", 1e-5), "1e-05");
    }

    #[test]
    fn uuid_or_none_vectors() {
        let id = Uuid::from_str("12345678-1234-5678-1234-567812345678").unwrap();
        assert_eq!(
            py_uuid_or_none(&Some(id)),
            "12345678-1234-5678-1234-567812345678"
        );
        assert_eq!(py_uuid_or_none(&None), "None");
    }
    // Moved from `api::app_issues::handlers_engage` with `py_str_value`.
    #[test]
    fn py_str_value_matches_cpython() {
        assert_eq!(py_str_value(&Value::Null), "None");
        assert_eq!(py_str_value(&json!(true)), "True");
        assert_eq!(py_str_value(&json!(77)), "77");
        assert_eq!(py_str_value(&json!("internal")), "internal");
        assert_eq!(py_str_value(&json!(["INTERNAL"])), "['INTERNAL']");
        assert_eq!(py_str_value(&json!({"a": 1})), "{'a': 1}");
        assert_eq!(py_str_value_repr(&json!("don't")), "\"don't\"");
        assert_eq!(py_str_value_repr(&json!("say \"hi\"")), "'say \"hi\"'");
        assert_eq!(py_str_value_repr(&json!("a\nb")), "'a\\nb'");
        assert_eq!(py_str_value(&json!(1.5)), "1.5");
        assert_eq!(py_str_value(&json!(1e16)), "1e+16");
    }

    // PIDASHCONV-1644: container strings escape per CPython
    // `str.isprintable` (via `py_printable`), like `py_repr_str`.
    // Every literal below was verified against the 3.12 interpreter
    // (`repr([ch])`); the container probe shape matches the issue.
    #[test]
    fn py_str_value_escapes_cpython_nonprintables() {
        let cases = [
            // The issue's four vectors: Cf format chars + private-use.
            ("\u{200b}", "'\\u200b'"),
            ("\u{200d}", "'\\u200d'"),
            ("\u{feff}", "'\\ufeff'"),
            ("\u{e000}", "'\\ue000'"),
            // Agreement pins (escaped before and after): nbsp, LSEP.
            ("\u{a0}", "'\\xa0'"),
            ("\u{2028}", "'\\u2028'"),
            // 1624 parity mirrors: unassigned, soft hyphen, Zs pair,
            // astral private-use + tag.
            ("\u{378}", "'\\u0378'"),
            ("\u{ad}", "'\\xad'"),
            ("\u{1680}", "'\\u1680'"),
            ("\u{2000}", "'\\u2000'"),
            ("\u{f0000}", "'\\U000f0000'"),
            ("\u{e0000}", "'\\U000e0000'"),
            // Narrow no-break space: Zs but not White_Space, so the old
            // gate passed it raw; CPython escapes it (review probe).
            ("\u{202f}", "'\\u202f'"),
            // No-change pins: controls escaped before and after.
            ("\0", "'\\x00'"),
            ("\u{7f}", "'\\x7f'"),
            ("\u{85}", "'\\x85'"),
        ];
        for (text, want) in cases {
            assert_eq!(
                py_str_value_repr(&Value::String(text.to_owned())),
                want,
                "element {text:?}"
            );
            assert_eq!(
                py_str_value(&Value::Array(vec![Value::String(text.to_owned())])),
                format!("[{want}]"),
                "container {text:?}"
            );
        }
        // Dict keys share the escaper (`repr({'\u200b': 1})`).
        assert_eq!(py_str_value(&json!({"\u{200b}": 1})), "{'\\u200b': 1}");
        // Printable near-misses stay raw (mirrors the 1624 pins).
        for text in [" ", "\u{377}", "\u{fe00}", "é", "😀", ""] {
            let want = format!("'{text}'");
            assert_eq!(
                py_str_value_repr(&Value::String(text.to_owned())),
                want,
                "printable {text:?}"
            );
        }
    }

    #[test]
    fn py_float_repr_probed_matches_cpython_oracle() {
        // CPython `repr` over edge + random f64s (seed 654): parsing
        // each spelling must reproduce it exactly.
        for text in [
            "0.0",
            "-0.0",
            "1.0",
            "-1.0",
            "0.1",
            "-0.1",
            "0.5",
            "4.5",
            "77.0",
            "1e+16",
            "-1e+16",
            "1000000000000000.0",
            "9999999999999998.0",
            "0.0001",
            "9.9999e-05",
            "1e-05",
            "1.5e-07",
            "1.23",
            "123.456",
            "1.1",
            "2.2",
            "0.3",
            "1e+300",
            "1.7976931348623157e+308",
            "5e-324",
            "2.2250738585072014e-308",
            "1e+21",
            "123456789.12345679",
            "0.00010000000001",
            "3.141592653589793",
            "100.0",
            "1000000.0",
            "1000000.0",
            "1234567890000000.0",
            "1.0000000000000002",
            "2.0000000000000004",
            "3.1969326087014466e+19",
            "-648.4462954478394",
            "2.7257935151841477e+19",
            "579.4076537460154",
            "8.527238532839549e+19",
            "-913.3955515106092",
            "8.896161802149298e+19",
            "-715.3670336061672",
            "3.588060462234755e+19",
            "435.64988671404353",
            "7.755666951748308e+19",
            "-689.2765889623878",
            "-4.330334504373883e+19",
            "-816.9972547462557",
            "-7.212821885073056e+19",
            "-1679.6052986552581",
            "3.2445178360908284e+19",
            "-204.03326175159876",
            "-8.659247373857653e+18",
            "1208.8390033969868",
            "-4.647260496917678e+19",
            "2331.221077226743",
            "8.5327628826864e+19",
            "1001.189002479752",
            "-8.234019824386068e+19",
            "-495.73367760484905",
            "9.352414724532316e+19",
            "346.0259035320625",
            "-7.1336788737953415e+19",
            "-956.186083621841",
            "-7.678412411071945e+19",
            "743.1470832768787",
            "-2.5012238015527862e+19",
            "-151.39194187298384",
            "5.543620478186435e+19",
            "-402.4753197702496",
            "-9.86234000471552e+19",
            "-1687.6484285668644",
            "-1.2613779274247045e+19",
            "295.5344355622931",
            "-4.708884174573601e+18",
            "143.2457353449458",
            "9.272274689849475e+19",
            "782.9417038372993",
            "-2.365484285927129e+19",
            "213.25467548742972",
            "-4.4287013372000076e+19",
            "-75.27500601863878",
            "7.3002688662026355e+19",
            "-617.7363223782181",
            "2.8132118711075865e+19",
            "642.045311657188",
            "-4.9760184971786445e+19",
            "-493.23593034675645",
            "-5.92865673699603e+19",
            "1271.787532431871",
            "8.862838211447603e+18",
            "-102.63510865808792",
            "2.4099300144340025e+19",
            "-2281.3345229971756",
            "-5.346252045636608e+19",
            "2132.4041415069833",
            "-6.090543569457227e+19",
            "1863.0368182127777",
            "6.0170257457244996e+19",
            "-182.18689918793999",
            "6.525439356052767e+17",
            "-1362.5914219885572",
            "5.1796889258414604e+19",
            "-319.24195464384303",
            "-8.253703237953189e+19",
            "287.06500363070336",
            "-2.591301605754159e+19",
            "1392.893079979731",
            "2.655060505738576e+18",
            "2036.4351785039644",
            "7.427269069333139e+19",
            "1347.4365990857013",
            "-8.434791962696217e+19",
            "-186.0700574057584",
        ] {
            let value: f64 = text.parse().unwrap();
            assert_eq!(py_float_repr_probed(value), text, "{text}");
        }
        assert_eq!(py_float_repr_probed(f64::NAN), "nan");
        assert_eq!(py_float_repr_probed(f64::INFINITY), "inf");
        assert_eq!(py_float_repr_probed(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn py_json_float_cases() {
        let cases: [(f64, &str); 22] = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (25.0, "25.0"),
            (25.9, "25.9"),
            (-25.9, "-25.9"),
            (0.1, "0.1"),
            (2.5, "2.5"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (-1e16, "-1e+16"),
            (1e21, "1e+21"),
            (1e100, "1e+100"),
            (0.0001, "0.0001"),
            (1e-5, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (5e-324, "5e-324"),
            (2.2250738585072014e-308, "2.2250738585072014e-308"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (123456.789, "123456.789"),
            (1.23456789012345, "1.23456789012345"),
            (std::f64::consts::PI, "3.141592653589793"),
        ];
        for (value, expected) in cases {
            assert_eq!(py_json_float(value), expected, "{value:?}");
        }
        assert_eq!(py_json_float(f64::INFINITY), "Infinity");
        assert_eq!(py_json_float(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(py_json_float(f64::NAN), "NaN");
    }

    #[test]
    fn py_json_float_rounds_exact_halfway_to_even() {
        // Exact-halfway values (true binary value ends in ...25) where
        // Rust's shortest rendering ends odd and CPython rounds to even.
        // Bit patterns captured from the 300k-vector equivalence sweep.
        let cases: [(u64, &str); 6] = [
            (0x4305edd45e85c45a, "771558860699787.2"),
            (0x431572bb837eea91, "1509281050376868.2"),
            (0xc30ace1af568d92a, "-943120572029733.2"),
            (0xc31e219d2288c649, "-2120302018572690.2"),
            (0xc309be32f6bf9a22, "-905750064395076.2"),
            (0xc2eff025ca007cb4, "-280930292859877.62"),
        ];
        for (bits, expected) in cases {
            assert_eq!(py_json_float(f64::from_bits(bits)), expected, "{bits:#x}");
        }
        // Controls: odd last digits that are not halfway keep the Rust
        // rendering.
        assert_eq!(py_json_float(0.5), "0.5");
        assert_eq!(py_json_float(2.5), "2.5");
    }

    // Canonicalized from the overlay (`mcp/protocol.rs`) with
    // `render_number`; big-int vectors prove the exact form against
    // `json.dumps` (PIDASHCONV-1623).
    #[test]
    fn render_number_prints_exact_digits() {
        fn render(number: &serde_json::Number) -> String {
            let mut out = String::new();
            render_number(&mut out, number);
            out
        }
        fn num(spelling: &str) -> serde_json::Number {
            serde_json::from_str::<Value>(spelling)
                .expect("number")
                .as_number()
                .expect("number")
                .clone()
        }
        assert_eq!(render(&num("0")), "0");
        assert_eq!(render(&num("-42")), "-42");
        assert_eq!(render(&num("9223372036854775807")), "9223372036854775807");
        assert_eq!(render(&num("-9223372036854775808")), "-9223372036854775808");
        assert_eq!(render(&num("18446744073709551615")), "18446744073709551615");
        // Past `u64`: exact digits, never scientific (the previous OSS
        // webhook copy printed `1.1805916207174113e+21` here).
        assert_eq!(
            render(&num("1180591620717411303424")),
            "1180591620717411303424"
        );
        assert_eq!(
            render(&num("-1180591620717411303424")),
            "-1180591620717411303424"
        );
        assert_eq!(
            render(&num("340282366920938463463374607431768211455")),
            "340282366920938463463374607431768211455"
        );
        assert_eq!(render(&num("1.5")), "1.5");
        assert_eq!(render(&num("1e16")), "1e+16");
        assert_eq!(render(&num("1e-05")), "1e-05");
        // Shared pre-existing quirk (see `render_number` docs): the
        // overflow spelling survives instead of `Infinity`.
        assert_eq!(render(&num("1e999")), "1e+999");
    }
}
