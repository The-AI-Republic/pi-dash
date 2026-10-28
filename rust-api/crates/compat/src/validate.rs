//! DRF field validation (OVL-04 part 3, PIDASHCONV-1542).
//!
//! Canonical home of the shared validation shapes: the stock DRF
//! messages, the error accumulators, and the `app_intake` push-style
//! `char`/`datetime` validators. OSS validation is genuinely
//! fragmented — four `FieldErrors` representations (`Map`, ordered
//! pairs, string lists, custom structs) times three validator styles
//! (push-style, `Result`, outcome enums) — so only shapes with
//! byte-identical copies move here:
//!
//! - moved: `push_error` (map, 5 defs), `push_pairs_error` (ordered
//!   pairs, 2 defs), `char_internal` (3 defs), `validate_char` and
//!   `validate_datetime` + `datetime_format_message` (2 defs each),
//!   the message consts (3 trios);
//! - kept local: `validate_char_field` x3, `validate_choice` x2,
//!   `validate_uuid` x1, `validate_str_choice` x3 (the project/workflow
//!   pair formats numbers differently), `check_choice` x3, the
//!   `Result`/outcome-enum `validate_char` x4, the overwriting
//!   `push_error` (types), the list-based `push_error` (module_links),
//!   `push_errors`, and the `validate_boolean` pair — whose bodies
//!   genuinely diverge (`1.0`/`y`/`yes` accepted in one, rejected in
//!   the other; at least one side is a ported bug, out of scope here).

use serde_json::{Map, Value};

/// DRF `required` message.
pub const REQUIRED_MSG: &str = "This field is required.";
/// DRF `null` message.
pub const NULL_MSG: &str = "This field may not be null.";
/// DRF `blank` message.
pub const BLANK_MSG: &str = "This field may not be blank.";
/// DRF `invalid` message for string parsing.
pub const INVALID_STR_MSG: &str = "Not a valid string.";
/// DRF `invalid` message for booleans.
pub const INVALID_BOOL_MSG: &str = "Must be a valid boolean.";

/// DRF `max_length` message.
pub fn max_length_message(max_length: usize) -> String {
    format!("Ensure this field has no more than {max_length} characters.")
}

/// DRF `DateTimeField` format message (moved from `app_intake`).
pub fn datetime_format_message() -> String {
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        .to_owned()
}

/// Append `message` to the `field` error list, creating it (moved from
/// `app_intake`; the `space`/`mcp`/`services` copies shared this
/// append semantic — `message` is `impl Into<String>` so the `String`
/// and `&str` copies collapse here).
pub fn push_error(errors: &mut Map<String, Value>, field: &str, message: impl Into<String>) {
    errors
        .entry(field.to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("error list")
        .push(Value::String(message.into()));
}

/// Append `message` to the `field` error list in the ordered-pairs
/// representation (moved from `app_issues/handlers_relations.rs`; the
/// `app_modules` copy shared the wrap dance — `message` is
/// `impl Into<Value>` so the `Value` and `String` copies collapse).
pub fn push_pairs_error(errors: &mut Vec<(String, Value)>, field: &str, message: impl Into<Value>) {
    let message = message.into();
    match errors.iter_mut().find(|(name, _)| name == field) {
        Some((_, Value::Array(messages))) => messages.push(message),
        Some((_, slot)) => {
            let prior = std::mem::replace(slot, Value::Null);
            *slot = Value::Array(vec![prior, message]);
        }
        None => errors.push((field.to_owned(), Value::Array(vec![message]))),
    }
}

/// DRF `CharField` scalar render (moved from `app_intake`): strings
/// trimmed, numbers via `to_string`, everything else (incl. null)
/// rejected.
#[allow(clippy::result_unit_err, reason = "moved signature, kept verbatim")]
pub fn char_internal(value: &Value) -> Result<String, ()> {
    match value {
        Value::String(text) => Ok(text.trim().to_string()),
        Value::Number(number) => Ok(number.to_string().trim().to_string()),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => Err(()),
        Value::Null => Err(()),
    }
}

/// DRF `CharField.to_internal_value`, push-style (moved from
/// `app_intake`): null/blank/invalid/max_length push their stock
/// messages and yield `None`, else the trimmed text.
pub fn validate_char(
    errors: &mut Map<String, Value>,
    field: &str,
    value: &Value,
    max_length: usize,
    allow_blank: bool,
    allow_null: bool,
) -> Option<String> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, NULL_MSG);
        }
        return None;
    }
    match char_internal(value) {
        Err(()) => {
            push_error(errors, field, INVALID_STR_MSG);
            None
        }
        Ok(text) => {
            if text.is_empty() && !allow_blank {
                push_error(errors, field, BLANK_MSG);
                return None;
            }
            // `[:255]` counts code points; over-long is a 400, never a
            // panic.
            if text.chars().count() > max_length {
                push_error(errors, field, max_length_message(max_length));
                return None;
            }
            Some(text)
        }
    }
}

/// DRF `DateTimeField.to_internal_value`: ISO-8601 with `Z`/offsets
/// (plus a bare date, read as midnight UTC, like DRF's date fallback).
/// Moved from `app_intake` (the pair differed only in a comment).
pub fn validate_datetime(
    errors: &mut Map<String, Value>,
    field: &str,
    value: &Value,
    allow_null: bool,
) -> Option<chrono::DateTime<chrono::Utc>> {
    if value.is_null() {
        if !allow_null {
            push_error(errors, field, NULL_MSG);
        }
        return None;
    }
    let raw = match value {
        Value::String(text) => text,
        _ => {
            push_error(errors, field, datetime_format_message());
            return None;
        }
    };
    let text = raw.trim().to_string();
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0).expect("midnight").and_utc());
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&text) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // DRF's `strptime(value, 'iso-8601')` fallthrough (`to_internal_value`
    // runs it when `parse_datetime` returns `None` — PIDASHCONV-773):
    // the literal matches case-insensitively and yields naive
    // 1900-01-01, read as UTC like the date arm above. Checked on the RAW
    // text — Django's `strptime` sees the value before any strip (this
    // port's trim is a pre-existing quirk), so a padded literal still
    // fails. Exact match (probed); ASCII-only (765 unicode-gap family).
    if raw.eq_ignore_ascii_case("iso-8601") {
        let naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .expect("1900-01-01 valid");
        return Some(naive.and_utc());
    }
    // DRF also accepts the `+HHMM`/`+HH:MM` offsets and fractional
    // forms RFC 3339 covers; anything else is the format error.
    push_error(errors, field, datetime_format_message());
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(errors: &Map<String, Value>, field: &str) -> Vec<String> {
        errors[field]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("str").to_owned())
            .collect()
    }

    #[test]
    fn messages_match_drf_bytes() {
        assert_eq!(REQUIRED_MSG, "This field is required.");
        assert_eq!(NULL_MSG, "This field may not be null.");
        assert_eq!(BLANK_MSG, "This field may not be blank.");
        assert_eq!(INVALID_STR_MSG, "Not a valid string.");
        assert_eq!(INVALID_BOOL_MSG, "Must be a valid boolean.");
        assert_eq!(
            max_length_message(255),
            "Ensure this field has no more than 255 characters."
        );
        assert_eq!(
            datetime_format_message(),
            "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        );
    }

    #[test]
    fn push_error_appends_per_field() {
        let mut errors = Map::new();
        push_error(&mut errors, "name", "first".to_owned());
        push_error(&mut errors, "name", "second");
        push_error(&mut errors, "other", "x".to_owned());
        assert_eq!(field(&errors, "name"), vec!["first", "second"]);
        assert_eq!(field(&errors, "other"), vec!["x"]);
    }

    #[test]
    fn push_pairs_error_appends_and_wraps() {
        let mut errors: Vec<(String, Value)> = Vec::new();
        push_pairs_error(&mut errors, "a", "one".to_owned());
        push_pairs_error(&mut errors, "a", Value::String("two".to_owned()));
        assert_eq!(errors, vec![("a".to_owned(), json!(["one", "two"]))]);
        // A non-array slot wraps (prior value preserved first).
        let mut errors = vec![("b".to_owned(), json!({"x": 1}))];
        push_pairs_error(&mut errors, "b", "late".to_owned());
        assert_eq!(errors, vec![("b".to_owned(), json!([{"x": 1}, "late"]))]);
    }

    #[test]
    fn char_internal_renders_scalars() {
        assert_eq!(char_internal(&json!("  padded  ")), Ok("padded".to_owned()));
        assert_eq!(char_internal(&json!(7)), Ok("7".to_owned()));
        assert_eq!(char_internal(&json!(1.5)), Ok("1.5".to_owned()));
        assert_eq!(char_internal(&json!(true)), Err(()));
        assert_eq!(char_internal(&json!([1])), Err(()));
        assert_eq!(char_internal(&json!({"a": 1})), Err(()));
        assert_eq!(char_internal(&json!(null)), Err(()));
    }

    #[test]
    fn validate_char_pushes_stock_messages() {
        let mut errors = Map::new();
        assert_eq!(
            validate_char(&mut errors, "f", &json!("  hi  "), 255, false, false),
            Some("hi".to_owned())
        );
        assert!(errors.is_empty());

        // null
        let mut errors = Map::new();
        assert_eq!(
            validate_char(&mut errors, "f", &json!(null), 255, false, false),
            None
        );
        assert_eq!(field(&errors, "f"), vec![NULL_MSG]);
        let mut errors = Map::new();
        assert_eq!(
            validate_char(&mut errors, "f", &json!(null), 255, false, true),
            None
        );
        assert!(errors.is_empty());

        // invalid + blank + over-long
        let mut errors = Map::new();
        assert_eq!(
            validate_char(&mut errors, "f", &json!([1]), 255, false, false),
            None
        );
        assert_eq!(field(&errors, "f"), vec![INVALID_STR_MSG]);
        let mut errors = Map::new();
        assert_eq!(
            validate_char(&mut errors, "f", &json!("   "), 255, false, false),
            None
        );
        assert_eq!(field(&errors, "f"), vec![BLANK_MSG]);
        let mut errors = Map::new();
        assert_eq!(
            validate_char(&mut errors, "f", &json!("abc"), 2, false, false),
            None
        );
        assert_eq!(field(&errors, "f"), vec![max_length_message(2)]);
    }

    #[test]
    fn validate_datetime_accepts_drf_shapes() {
        let mut errors = Map::new();
        let dt =
            validate_datetime(&mut errors, "f", &json!("2026-03-04"), false).expect("bare date");
        assert_eq!(dt.to_rfc3339(), "2026-03-04T00:00:00+00:00");
        let mut errors = Map::new();
        let dt = validate_datetime(&mut errors, "f", &json!("2026-03-04T05:06:07Z"), false)
            .expect("rfc3339");
        assert_eq!(dt.to_rfc3339(), "2026-03-04T05:06:07+00:00");
        // iso-8601 literal quirk + null + garbage
        let mut errors = Map::new();
        let dt = validate_datetime(&mut errors, "f", &json!("ISO-8601"), false).expect("quirk");
        assert_eq!(dt.to_rfc3339(), "1900-01-01T00:00:00+00:00");
        let mut errors = Map::new();
        assert_eq!(
            validate_datetime(&mut errors, "f", &json!(null), false),
            None
        );
        assert_eq!(field(&errors, "f"), vec![NULL_MSG]);
        let mut errors = Map::new();
        assert_eq!(
            validate_datetime(&mut errors, "f", &json!("tomorrow"), false),
            None
        );
        assert_eq!(field(&errors, "f"), vec![datetime_format_message()]);
        let mut errors = Map::new();
        assert_eq!(validate_datetime(&mut errors, "f", &json!(7), false), None);
        assert_eq!(field(&errors, "f"), vec![datetime_format_message()]);
    }
}
