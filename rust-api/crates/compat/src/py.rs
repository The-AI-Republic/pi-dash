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
//!   truth-table-identical bodies), `py_strip` (all 23 defs: the
//!   spellings differ and three add a redundant `\u{85}`, which
//!   `char::is_whitespace` already covers) + `py_is_space` (4),
//!   `py_json_string` (7), `python_dumps` + `python_dump_into` +
//!   `python_dump_str` (5 each), `py_repr_string` (2), `py_bool` (2),
//!   `py_uuid_or_none` (the `None`-rendering `opt_uuid` pair);
//! - kept local: the `shape`/`shape_relations` str/repr set (escapes
//!   `\u{2028}` through, own float algorithm), the invites str/repr
//!   set (same `\u{2028}` divergence plus inf/nan number handling),
//!   the webhooks `py_str` (JSON-array rendering), the invites
//!   `py_truthy` (`None`-means-inf default), the naive/partial
//!   `py_repr` pairs, `py_str_repr` x2+x2, the divergent `dumps`
//!   triples, and every singleton (`py_int`, `py_str_value`,
//!   `py_repr_value`, `is_truthy` x12 — twelve distinct bodies,
//!   `py_number_str`, `python_int`, `render_number`, `py_dumps`,
//!   `py_title`, `*_is_falsy`).

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

/// Python printability for `repr()` escaping: controls, DEL and the
/// U+2028/2029 line boundaries are non-printable.
pub fn py_printable(c: char) -> bool {
    !(c.is_control() || c == '\u{7f}' || c == '\u{2028}' || c == '\u{2029}')
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
}
