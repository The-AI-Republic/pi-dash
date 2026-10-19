#![forbid(unsafe_code)]

//! Python text-formatting helpers shared by the ported task bodies.
//!
//! [`format_eta`] renders a UTC timestamp the way Python's
//! `datetime.isoformat` does. The `py_*` functions render a JSON value the
//! way Python's `repr` does (`None`/`True`/`False`, quote choice, `\xhh`
//! escapes, float exponent form), used where a ported body stringifies a
//! value into a log document or webhook payload exactly as Python would.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

/// Format a UTC timestamp the way Python `datetime.isoformat` renders it:
/// ISO-8601 with an explicit `+00:00` offset (never `Z`), fractional
/// seconds only when nonzero (Python always uses six fraction digits when
/// they are present).
pub fn format_eta(eta: DateTime<Utc>) -> String {
    let base = eta.format("%Y-%m-%dT%H:%M:%S").to_string();
    if eta.timestamp_subsec_nanos() == 0 {
        format!("{base}+00:00")
    } else {
        // Truncated to microseconds: Python datetimes cannot represent
        // finer precision.
        format!("{base}.{:06}+00:00", eta.timestamp_subsec_micros())
    }
}

pub fn py_float_repr(float: f64) -> String {
    let text = serde_json::Number::from_f64(float)
        .map(|number| number.to_string())
        .unwrap_or_else(|| "null".to_owned());
    if let Some(exponent_at) = text.find(['e', 'E']) {
        let (mantissa, exponent) = text.split_at(exponent_at);
        let shift: i32 = exponent[1..].parse().unwrap_or(0);
        return format!("{mantissa}e{shift:+03}");
    }
    // Python switches to exponent form once the decimal exponent drops
    // below -4 (`1e-05`), while `serde_json` still prints that decade
    // fixed (`0.00001`). A fixed fraction with four or more leading
    // zeros is exactly that decade, so rewrite it to the exponent form.
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    if let Some(fraction) = digits.strip_prefix("0.") {
        let zeros = fraction.bytes().take_while(|byte| *byte == b'0').count();
        if zeros >= 4 {
            let sig = fraction[zeros..].trim_end_matches('0');
            if sig.is_empty() {
                return text;
            }
            let head = &sig[..1];
            let tail = &sig[1..];
            let shift = zeros + 1;
            if tail.is_empty() {
                return format!("{sign}{head}e-{shift:02}");
            }
            return format!("{sign}{head}.{tail}e-{shift:02}");
        }
    }
    text
}

/// Python `repr` of one JSON value, as kombu's `saferepr` renders the
/// `argsrepr`/`kwargsrepr` headers: `None`/`True`/`False`, quoted
/// strings, `[...]` lists and `{'k': v}` dicts. Floats render through
/// [`py_float_repr`] (`1e+16`, never serde's `1e16`).
pub fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => {
            if n.is_f64() {
                // `is_f64` guarantees `as_f64`; NaN/infinity are
                // unreachable in `serde_json::Value`.
                py_float_repr(n.as_f64().unwrap_or(0.0))
            } else {
                n.to_string()
            }
        }
        Value::String(s) => py_str(s),
        Value::Array(items) => {
            let inner = items.iter().map(py_repr).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        Value::Object(map) => dict_repr(map),
    }
}

/// Python `repr` of a dict: `{'key': value}` with insertion-ordered keys.
pub fn dict_repr(map: &Map<String, Value>) -> String {
    let inner = map
        .iter()
        .map(|(k, v)| format!("{}: {}", py_str(k), py_repr(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{inner}}}")
}

/// Python `repr` of the positional args tuple: `()`, `(one,)` or `(a, b)`.
/// Kombu always passes a tuple (`.delay(*args)`), so the empty case is
/// `()` even though the body carries a JSON list.
fn tuple_repr(args: &[Value]) -> String {
    match args {
        [] => "()".to_owned(),
        [single] => format!("({},)", py_repr(single)),
        _ => {
            let inner = args.iter().map(py_repr).collect::<Vec<_>>().join(", ");
            format!("({inner})")
        }
    }
}

/// Code points Python's `repr` escapes outside the C0 controls, DEL
/// and C1 (all `char::is_control`): Cf format controls, Zl/Zp
/// separators, Zs spaces other than U+0020, and Co private-use — as
/// (inclusive) ranges, merged where neighbors share printability.
/// Surrogates cannot occur in `str`; unassigned (Cn) points also escape
/// in Python but are not enumerable here (residual edge: a `repr` input
/// holding a code point unassigned in Unicode 15 prints raw — such text
/// is already pathological).
const UNPRINTABLE_RANGES: [(u32, u32); 20] = [
    (0x00A0, 0x00A0),
    (0x00AD, 0x00AD),
    (0x061C, 0x061C),
    (0x1680, 0x1680),
    (0x180E, 0x180E),
    (0x2000, 0x200F),
    (0x2028, 0x202F),
    (0x205F, 0x206F),
    (0x3000, 0x3000),
    (0xE000, 0xF8FF),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x13438),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0000, 0xE0FFF),
    (0xF0000, 0xFFFFF),
    (0x100000, 0x10FFFF),
];

fn is_py_printable(c: char) -> bool {
    if c.is_control() {
        return false;
    }
    let code = c as u32;
    !UNPRINTABLE_RANGES
        .iter()
        .any(|(lo, hi)| code >= *lo && code <= *hi)
}

/// Python `repr` of a string: double-quoted when it holds `'` but no
/// `"`, else single-quoted; `\n`/`\r`/`\t`/`\\` short escapes, the
/// active quote escaped, and every other unprintable as `\xhh` / `\uxxxx`
/// / `\U00xxxxxx` (lowercase hex, like CPython).
fn py_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        if c == quote {
            out.push('\\');
            out.push(c);
            continue;
        }
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if !is_py_printable(c) => {
                let code = c as u32;
                if code <= 0xFF {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code <= 0xFFFF {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use serde_json::json;

    #[test]
    fn eta_keeps_six_fraction_digits_like_cpython() {
        // Python `isoformat` renders exact-millisecond times with six
        // digits (`.173000`); chrono's `AutoSi` would trim to `.173`.
        let eta = Utc.with_ymd_and_hms(2026, 9, 26, 19, 55, 30).unwrap()
            + Duration::milliseconds(173);
        assert_eq!(format_eta(eta), "2026-09-26T19:55:30.173000+00:00");
        let whole = Utc.with_ymd_and_hms(2026, 9, 26, 19, 55, 30).unwrap();
        assert_eq!(format_eta(whole), "2026-09-26T19:55:30+00:00");
    }

    #[test]
    fn kwargsrepr_keeps_python_insertion_order() {
        // Verified against kombu: `{'b': 2, 'a': 1}` renders in insertion
        // order, not sorted. Requires serde_json `preserve_order`.
        let mut kwargs = Map::new();
        kwargs.insert("b".to_owned(), json!(2));
        kwargs.insert("a".to_owned(), json!(1));
        assert_eq!(dict_repr(&kwargs), "{'b': 2, 'a': 1}");
    }

    #[test]
    fn python_repr_matches_cpython_for_json_values() {
        assert_eq!(py_repr(&Value::Null), "None");
        assert_eq!(py_repr(&json!(true)), "True");
        assert_eq!(py_repr(&json!(1)), "1");
        assert_eq!(py_repr(&json!("a'b")), "\"a'b\"");
        assert_eq!(py_repr(&json!("a\nb")), "'a\\nb'");
        assert_eq!(py_repr(&json!([1, "x"])), "[1, 'x']");
        assert_eq!(py_repr(&json!({"a": 2})), "{'a': 2}");
        assert_eq!(tuple_repr(&[]), "()");
        assert_eq!(tuple_repr(&[json!(1)]), "(1,)");
        assert_eq!(tuple_repr(&[json!(1), json!("x")]), "(1, 'x')");
    }

    #[test]
    fn string_repr_matches_cpython_quote_choice_and_escapes() {
        // Quote choice: double quotes when the text holds `'` but no `"`.
        assert_eq!(py_str("a'b"), "\"a'b\"");
        assert_eq!(py_str("a\"b"), "'a\"b'");
        assert_eq!(py_str("a'b\"c"), "'a\\'b\"c'");
        assert_eq!(py_str("plain"), "'plain'");
        // DEL, C1 and Zs-nonspace escape; printable Unicode stays raw.
        assert_eq!(py_str("a\x7fb"), "'a\\x7fb'");
        assert_eq!(py_str("a\u{80}b"), "'a\\x80b'");
        assert_eq!(py_str("a\u{a0}b"), "'a\\xa0b'");
        assert_eq!(py_str("a b"), "'a b'");
        assert_eq!(py_str("café"), "'café'");
        assert_eq!(py_str("一"), "'一'");
        // Cf / Zl / Co escape with lowercase hex.
        assert_eq!(py_str("a\u{200b}b"), "'a\\u200bb'");
        assert_eq!(py_str("a\u{2028}b"), "'a\\u2028b'");
        assert_eq!(py_str("a\u{e000}b"), "'a\\ue000b'");
        assert_eq!(py_str("a\u{110bd}b"), "'a\\U000110bdb'");
        // Backslash and the active quote escape.
        assert_eq!(py_str("a\\b"), "'a\\\\b'");
        assert_eq!(py_str("it's"), "\"it's\"");
    }

    #[test]
    fn float_repr_matches_cpython_exponents() {
        assert_eq!(py_float_repr(65535.0), "65535.0");
        assert_eq!(py_float_repr(1e16), "1e+16");
        assert_eq!(py_float_repr(0.00001), "1e-05");
        assert_eq!(py_float_repr(-0.00001), "-1e-05");
        assert_eq!(py_float_repr(0.001), "0.001");
        assert_eq!(py_repr(&json!(1e16)), "1e+16");
        assert_eq!(py_repr(&json!(65535.0)), "65535.0");
        // Integers never take the float path.
        assert_eq!(py_repr(&json!(65535)), "65535");
    }
}
