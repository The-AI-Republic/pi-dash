//! D-14 runner-session shapes: response JSON, control envelopes, stream keys.
//!
//! The D-14 "serializers layer" — no DRF serializers exist in this domain
//! (verified by grep), so the inline-JSON builders below ARE the layer:
//!
//! * [`responses`] — session-open 201 bodies, poll 200 envelopes, and the
//!   error-body + status set (`views/sessions.py:97-119, :266-285,
//!   :595-601`, `views/machine_sessions.py:121-131, :339-345`).
//! * [`envelopes`] — outbox type sets, `_serialize` /
//!   `_decode_read_result`, `_ensure_envelope`, assign / resume / cancel
//!   / revoke / remove_runner frames, eviction bodies
//!   (`services/outbox.py:38-67, :120-189, :544-556`,
//!   `services/machine_outbox.py:49-65, :373-385`,
//!   `services/pubsub.py:39-42, :81-108, :110-176`,
//!   `services/matcher.py:306-336`,
//!   `services/session_service.py:400-451, :473-508`).
//! * [`keys`] — Redis key builders + stream-id utils
//!   (`services/outbox.py:89-114, :687-737`,
//!   `services/machine_outbox.py:84-104, :339-341`,
//!   `services/pubsub.py:34-36`).
//!
//! Fixtures replayed byte-identical by the unit tests alongside each
//! module: `rust-api/fixtures/runner_sessions/fx-rses-02-shapes.json`
//! (FX-RSES-02), `-03-envelopes.json` (FX-RSES-03), `-04-keys.json`
//! (FX-RSES-04).
//!
//! ## JSON rendering
//!
//! Two Django renderers are in play, verified against the installed
//! DRF/Django sources (no project override of `UNICODE_JSON` /
//! `COMPACT_JSON` in `settings/common.py`):
//!
//! * DRF `Response` (open/delete endpoints + DRF dispatch errors):
//!   compact separators `(',', ':')`, `ensure_ascii=False`, plus a
//!   post-pass escaping U+2028/U+2029
//!   (`rest_framework/renderers.py`, `compat.py:156-158`).
//! * Django `JsonResponse` (both poll views) and every bare
//!   `json.dumps` call (outbox payloads, eviction bodies): default
//!   separators `(', ', ': ')`, `ensure_ascii=True`.
//!
//! [`render_compact_utf8`] and [`render_spaced_ascii`] reproduce both
//! byte-exact, including CPython float `repr` spelling. Recursion depth
//! is bounded in practice: every nested value reaching the renderer was
//! parsed by `serde_json` (128-level cap) or built here (depth ≤ 4).

pub mod envelopes;
pub mod keys;
pub mod responses;

pub use envelopes::{
    build_assign_msg, cancel_frame, decode_read_result, ensure_envelope, eviction_body,
    is_offline_reject_machine_type, is_offline_reject_runner_type, is_valid_machine_type,
    is_valid_runner_type, redeliver_cancel_frame, remove_runner_frame, resume_ack_frame,
    revoke_frame, serialize, terminal_cancel_reason, DecodeError, DecodedMessage,
    SerializedMessage, StreamEntry, StreamRead, CLOSE_CODE_PROTOCOL_UNSUPPORTED,
    CLOSE_CODE_TICKET_INVALID, OFFLINE_REJECT_MACHINE, OFFLINE_REJECT_RUNNER,
    REASON_CANCELLATION_PENDING_ON_RECONNECT, REASON_UNKNOWN_RUN_ON_RECONNECT,
    REMOVE_RUNNER_DEFAULT_REASON, REVOKE_DEFAULT_REASON, VALID_TYPES_MACHINE, VALID_TYPES_RUNNER,
};
pub use keys::{decrement_stream_id, format_id, id_for_secs_ago, min_stream_id, split_id};
pub use responses::{
    dev_machine_mismatch_drf, dev_machine_mismatch_poll, json_parse_error_poll, machine_open_201,
    method_not_allowed_drf, method_not_allowed_poll, no_content, poll_200, project_mismatch_drf,
    protocol_version_unsupported_drf, runner_id_mismatch_drf, runner_id_mismatch_poll,
    runner_open_201, runner_state_locked_drf, runner_state_locked_poll, session_evicted_poll,
    unauthorized_drf, unauthorized_poll, HttpResponse, STATUS_BAD_REQUEST, STATUS_CONFLICT,
    STATUS_CREATED, STATUS_FORBIDDEN, STATUS_METHOD_NOT_ALLOWED, STATUS_NO_CONTENT, STATUS_OK,
    STATUS_SERVICE_UNAVAILABLE, STATUS_UNAUTHORIZED, STATUS_UPGRADE_REQUIRED,
};

use serde_json::Value;
use std::fmt::Write as _;

/// Render a value like DRF's `JSONRenderer`: compact separators,
/// raw UTF-8, U+2028/U+2029 escaped.
pub(crate) fn render_compact_utf8(value: &Value) -> String {
    let mut out = String::new();
    render_value(value, &mut out, false, ",", ":");
    out.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Render a value like Django's `JsonResponse` / bare `json.dumps`:
/// default separators, non-ASCII escaped.
pub(crate) fn render_spaced_ascii(value: &Value) -> String {
    let mut out = String::new();
    render_value(value, &mut out, true, ", ", ": ");
    out
}

fn render_value(value: &Value, out: &mut String, ascii: bool, item_sep: &str, kv_sep: &str) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&render_number(n)),
        Value::String(s) => render_string(s, ascii, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(item_sep);
                }
                // `dumps` of a nested value uses the same separators.
                render_value(item, out, ascii, item_sep, kv_sep);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (key, val)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(item_sep);
                }
                render_string(key, ascii, out);
                out.push_str(kv_sep);
                render_value(val, out, ascii, item_sep, kv_sep);
            }
            out.push('}');
        }
    }
}

fn render_number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    match n.as_f64() {
        Some(f) => render_float_py(f, true),
        // Unreachable without `arbitrary_precision`: every Number is
        // i64, u64 or f64.
        None => n.to_string(),
    }
}

/// CPython `repr()` spelling for a finite float, with `json`-mode
/// (`NaN`/`Infinity`) or `str()`-mode (`nan`/`inf`) non-finite words.
///
/// Shortest digits come from Ryū (via `serde_json::Number`, already
/// in the dep tree): the shortest spelling that round-trips, ties
/// broken toward the true value — exactly `repr`'s rule. Rust's own
/// `{:e}` is shortest too but breaks ties differently (it renders
/// `153838026194641.13` where `repr` gives `...412`), so it cannot
/// be the digit source. Only the fixed/exponent choice (`-4 <= e10
/// <= 15` is fixed) and the exponent shape (`e±XX`, two digits
/// minimum) are applied here, so no re-rounding occurs.
pub(crate) fn render_float_py(f: f64, json: bool) -> String {
    if f.is_nan() {
        return if json { "NaN" } else { "nan" }.to_string();
    }
    if f.is_infinite() {
        if f.is_sign_negative() {
            return if json { "-Infinity" } else { "-inf" }.to_string();
        }
        return if json { "Infinity" } else { "inf" }.to_string();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let ryu = serde_json::Number::from_f64(f)
        .expect("finite float")
        .to_string();
    let neg = f.is_sign_negative();
    let mant = ryu.trim_start_matches('-');
    let (mant, exp10) = match mant.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().expect("ryu exponent")),
        None => (mant, 0),
    };
    let frac_len = mant.split_once('.').map_or(0, |(_, fr)| fr.len() as i32);
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_start_matches('0').to_string();
    // `f` is finite and nonzero, so a nonzero digit always survives.
    debug_assert!(!digits.is_empty());
    let exp = exp10 - frac_len + digits.len() as i32 - 1;
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if (-4..=15).contains(&exp) {
        // Fixed notation; the point sits after `exp + 1` digits.
        let point = exp + 1;
        if point <= 0 {
            out.push_str("0.");
            for _ in point..0 {
                out.push('0');
            }
            out.push_str(&digits);
        } else if point as usize >= digits.len() {
            out.push_str(&digits);
            for _ in digits.len()..point as usize {
                out.push('0');
            }
            out.push_str(".0");
        } else {
            let point = point as usize;
            out.push_str(&digits[..point]);
            out.push('.');
            out.push_str(&digits[point..]);
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(out, "e{exp:+03}");
    }
    out
}

/// CPython string quoting: short escapes, `\u00xx` for other C0
/// controls, and — in `ascii` mode — `\uXXXX` for everything from
/// DEL up (astral chars as surrogate pairs), all lowercase hex.
fn render_string(s: &str, ascii: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if ascii && (c as u32) >= 0x7f => {
                let n = c as u32;
                if n > 0xffff {
                    let n = n - 0x10000;
                    let _ = write!(
                        out,
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (n >> 10),
                        0xdc00 + (n & 0x3ff)
                    );
                } else {
                    let _ = write!(out, "\\u{n:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renderer_separators() {
        let value = json!({"a": [1, true, null], "b": {}});
        assert_eq!(render_compact_utf8(&value), r#"{"a":[1,true,null],"b":{}}"#);
        assert_eq!(
            render_spaced_ascii(&value),
            r#"{"a": [1, true, null], "b": {}}"#
        );
    }

    #[test]
    fn renderer_string_escaping() {
        // Control escapes shared by both modes.
        let value = json!("a\"b\\c\nd\re\tf\x08g\x0ch\x00i\x1bj");
        let compact = render_compact_utf8(&value);
        assert_eq!(
            compact,
            "\"a\\\"b\\\\c\\nd\\re\\tf\\bg\\fh\\u0000i\\u001bj\""
        );
        assert_eq!(render_spaced_ascii(&value), compact);
        // DEL + non-ASCII: raw in compact (DRF UNICODE_JSON), escaped
        // in spaced (json.dumps ensure_ascii).
        let value = json!("~\u{7f}\u{e9}\u{1d11e}");
        assert_eq!(render_compact_utf8(&value), "\"~\u{7f}\u{e9}\u{1d11e}\"");
        assert_eq!(
            render_spaced_ascii(&value),
            "\"~\\u007f\\u00e9\\ud834\\udd1e\""
        );
        // DRF post-pass: U+2028/U+2029 always escaped in compact.
        let value = json!("\u{2028}\u{2029}x");
        assert_eq!(render_compact_utf8(&value), "\"\\u2028\\u2029x\"");
        assert_eq!(render_spaced_ascii(&value), "\"\\u2028\\u2029x\"");
    }

    #[test]
    fn renderer_float_spelling() {
        for (f, want) in [
            (0.1, "0.1"),
            (1.0, "1.0"),
            (-1.5, "-1.5"),
            (123.456, "123.456"),
            (100.0, "100.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1234567890123456.0, "1234567890123456.0"),
            // Shortest-round-trip ties: two spellings parse back, and
            // `repr` picks the one closest to the true value (Rust's
            // `{:e}` picks the other — hence the Ryū digit source).
            (153838026194641.12, "153838026194641.12"),
            (-1108296573107658.2, "-1108296573107658.2"),
            (1000000000000000.2, "1000000000000000.2"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_eq!(render_float_py(f, true), want, "float {f}");
        }
        assert_eq!(render_float_py(-0.0, true), "-0.0");
        assert_eq!(render_float_py(0.0, true), "0.0");
        // str() mode: lowercase non-finite words.
        assert_eq!(render_float_py(f64::NAN, false), "nan");
        assert_eq!(render_float_py(f64::INFINITY, false), "inf");
        assert_eq!(render_float_py(f64::NEG_INFINITY, false), "-inf");
        // Through a value, in both separators.
        let value = json!({"f": 0.1});
        assert_eq!(render_compact_utf8(&value), r#"{"f":0.1}"#);
        assert_eq!(render_spaced_ascii(&value), r#"{"f": 0.1}"#);
    }
}
