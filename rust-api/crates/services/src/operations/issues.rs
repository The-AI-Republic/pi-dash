//! Issue operations: create, partial update and project move.
//!
//! The serializer step of the v1 work-item write endpoints, moved from
//! `pidash_api::v1_work_items::handlers_core` (OVL-14, PIDASHCONV-1649):
//! `IssueSerializer(data, context={...}).save()` for create,
//! `IssueSerializer(issue, data, partial=True).save()` for update
//! (`app/serializers/issue.py`, driven by `IssueListCreateAPIEndpoint`
//! and `IssueDetailAPIEndpoint`, `views/issue.py:470-540,790-909`).
//! The move flow (`move_issue`: state/label remap, sequence, handoff)
//! joins this module with the `handlers_move_search` move.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone as _, Utc};
use chrono_tz::Tz;
use sea_orm::EntityTrait;
use serde_json::{Map, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use pidash_compat::py::py_strip;
use pidash_db::entity::issues;

use crate::app_issues::{SourceIssueRow, ASSIGNEE_IDS_SQL, LABEL_IDS_SQL};
use crate::domain::activity::TaskEnqueueHook;
use crate::domain::issues as domain_issues;
use crate::domain::transition::IssueTransitionHook;
use crate::domain::{self, WriteCtx};
use crate::html_normalize::lxml_roundtrip;
use crate::orchestration::blockers::{has_open_blockers_sql, summary_sql, BlockerRow};
use crate::read::{self, DecodedIssue, WriteProject};
use crate::v1_work_items::python_number_str;
use crate::v1_work_items::shape_issue as shape;
use crate::v1_work_items::shape_issue::{
    BlockerSummary, IssueRow as ApiIssueRow, RepresentationInput, SummaryItem,
};

use super::common::{activate_timezone, db_error, now_utc};
use super::deps::Deps;
use super::error::{json_string, Error, Outcome, PAYLOAD_NOT_VALID_BODY};

/// Create/patch external-duplicate 409 message (`views/issue.py:505,836`).
const EXTERNAL_DUP_MESSAGE: &str =
    "Issue with the same external id and external source already exists";

/// CPython `type(data).__name__` for the non-dict / non-list messages.
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "int"
            } else if number.is_f64() {
                "float"
            } else {
                // Arbitrary-precision integers past `u64` (the `preserve_order`
                // + `arbitrary_precision` build): still Python `int`s.
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Minimal CPython `str()` for error interpolation (`ChoiceField` misses,
/// UUID `"..." is not a valid UUID`): strings verbatim, numbers via
/// [`python_number_str`], `True`/`False`/`None`, composites via CPython
/// `repr` (single quotes, `True`/`False`/`None`, insertion order).
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => python_number_str(number),
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_quoted).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_repr_quoted(&Value::String(key.clone())),
                        py_repr_quoted(item)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// CPython `repr()` of one value (see [`py_repr`]): strings single-quoted
/// with backslash/`'` escapes, everything else like [`py_repr`].
fn py_repr_quoted(value: &Value) -> String {
    match value {
        Value::String(text) => {
            // CPython picks single quotes unless the string holds one (then
            // double, escaping embedded doubles). Non-printables escape as
            // `\x..`/`\u....`; keep the common arms exact.
            let mut escaped = String::with_capacity(text.len() + 2);
            for ch in text.chars() {
                match ch {
                    '\\' => escaped.push_str("\\\\"),
                    '\n' => escaped.push_str("\\n"),
                    '\r' => escaped.push_str("\\r"),
                    '\t' => escaped.push_str("\\t"),
                    c if c.is_control() => {
                        let code = c as u32;
                        if code <= 0xff {
                            escaped.push_str(&format!("\\x{code:02x}"));
                        } else if code <= 0xffff {
                            escaped.push_str(&format!("\\u{code:04x}"));
                        } else {
                            escaped.push_str(&format!("\\U{code:08x}"));
                        }
                    }
                    c => escaped.push(c),
                }
            }
            if !text.contains('\'') || text.contains('"') && !text.contains('\'') {
                format!("'{escaped}'")
            } else if !text.contains('"') {
                format!("\"{}\"", escaped.replace('"', "\\\""))
            } else {
                format!("'{}'", escaped.replace('\'', "\\'"))
            }
        }
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr_quoted).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{}: {}",
                        py_repr_quoted(&Value::String(key.clone())),
                        py_repr_quoted(item)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        _ => py_repr(value),
    }
}

/// Run `normalize_description_input` the way the views do (before the
/// serializer exists), with the exact non-dict control flow: `in` on
/// `None`/int/float/bool raises `TypeError` → 500; `dict(data)` on a
/// non-mapping raises `ValueError` → 500. Only mappings (and
/// markdown-key-free strings/lists) reach the serializer.
fn normalize_view_body(
    value: &Value,
    convert: &dyn Fn(&str) -> Result<String, String>,
) -> Result<(Value, bool), Error> {
    use shape::{LegacyInput, MarkdownInput};
    // Python `in`: dicts test keys, strings test substrings, lists test
    // element equality, everything else raises `TypeError`.
    let contains = |key: &str| -> Result<bool, Error> {
        match value {
            Value::Object(map) => Ok(map.contains_key(key)),
            Value::String(text) => Ok(text.contains(key)),
            Value::Array(items) => Ok(items
                .iter()
                .any(|item| item == &Value::String(key.to_owned()))),
            _ => Err(Error::ServerError),
        }
    };
    if !contains("description_markdown")? && !contains("description")? {
        return Ok((value.clone(), false));
    }
    // `dict(data)`: mappings pass through, everything else 500s.
    let Value::Object(map) = value else {
        return Err(Error::ServerError);
    };
    let markdown = match map.get("description_markdown") {
        None => MarkdownInput::Absent,
        Some(Value::Null) => MarkdownInput::Null,
        Some(Value::String(text)) => MarkdownInput::Text(text),
        Some(_) => MarkdownInput::NonString,
    };
    let legacy = match map.get("description") {
        None => LegacyInput::Absent,
        Some(Value::Null) => LegacyInput::Null,
        Some(Value::String(text)) => LegacyInput::Text(text),
        Some(_) => LegacyInput::NonString,
    };
    let html_present = map.contains_key("description_html");
    match shape::normalize_description_input(markdown, legacy, html_present, convert) {
        Ok(outcome) => {
            use shape::NormalizeAction;
            let mut next = map.clone();
            match outcome.action {
                NormalizeAction::Unchanged => {}
                NormalizeAction::DropKeys => {
                    next.shift_remove("description_markdown");
                    next.shift_remove("description");
                }
                NormalizeAction::SetHtml(html) => {
                    next.shift_remove("description_markdown");
                    next.shift_remove("description");
                    next.insert("description_html".to_owned(), Value::String(html));
                }
            }
            Ok((Value::Object(next), outcome.from_markdown))
        }
        Err(error) => Err(Error::FieldErrors(error.body())),
    }
}

/// The activity `requested_data` text for a normalized write body: form
/// bodies dump list fields as `QueryDict` last-wins scalars, JSON bodies
/// dump as-is (arrays are real data there).
fn requested_data_text(normalized: &Value) -> String {
    pidash_compat::py::django_dumps(normalized)
}

/// Project a form body the way `normalize_description_input` sees it:
/// `data.dict()` takes the LAST value per key — the kernel's list-field
/// arrays collapse to last-wins scalars before normalize runs.
fn form_dict_for_normalize(value: &Value) -> Value {
    let mut projected = value.clone();
    if let Value::Object(map) = &mut projected {
        pidash_compat::http::project_list_scalars(map, &["assignees", "labels"]);
    }
    projected
}

/// DRF `CharField` failure messages, in check order: blank → null →
/// invalid → max_length → null-characters (`fields.py` `CharField`,
/// `ProhibitNullCharactersValidator`). `stripped` reports whether the blank
/// arm trims (every model `CharField`/`TextField` here trims).
fn char_field_errors(
    value: &Value,
    allow_blank: bool,
    allow_null: bool,
    max_length: Option<usize>,
) -> Vec<String> {
    // `run_validation` tests blank before the null arm
    // (`data == '' or (trim_whitespace and str(data).strip() == '')`).
    if let Value::String(text) = value {
        if (text.is_empty() || py_strip(text).is_empty()) && !allow_blank {
            return vec!["This field may not be blank.".to_owned()];
        }
    } else if value.is_null() {
        if !allow_null {
            return vec!["This field may not be null.".to_owned()];
        }
        return Vec::new();
    }
    match value {
        Value::String(text) => {
            let stripped = py_strip(text);
            // `run_validators` extends ONE list across all validators, in
            // append order: `MaxLengthValidator` before
            // `ProhibitNullCharactersValidator`.
            let mut out = Vec::new();
            if let Some(max) = max_length {
                // `MaxLengthValidator`: `len()` counts code points.
                if stripped.chars().count() > max {
                    out.push(format!(
                        "Ensure this field has no more than {max} characters."
                    ));
                }
            }
            if stripped.contains('\0') {
                out.push("Null characters are not allowed.".to_owned());
            }
            out
        }
        Value::Bool(_) => vec!["Not a valid string.".to_owned()],
        Value::Number(number) => {
            let text = python_number_str(number);
            if let Some(max) = max_length {
                if text.chars().count() > max {
                    return vec![format!(
                        "Ensure this field has no more than {max} characters."
                    )];
                }
            }
            Vec::new()
        }
        _ => vec!["Not a valid string.".to_owned()],
    }
}

/// The `CharField` blank/null/invalid arms only (no validators): for
/// fields with model validators, which run before `MaxLengthValidator`.
fn char_shape_errors(value: &Value, allow_blank: bool, allow_null: bool) -> Vec<String> {
    if let Value::String(text) = value {
        if (text.is_empty() || py_strip(text).is_empty()) && !allow_blank {
            return vec!["This field may not be blank.".to_owned()];
        }
        return Vec::new();
    }
    if value.is_null() {
        if !allow_null {
            return vec!["This field may not be null.".to_owned()];
        }
        return Vec::new();
    }
    match value {
        Value::Number(_) => Vec::new(),
        _ => vec!["Not a valid string.".to_owned()],
    }
}

/// Coerce a validated string input (`str(data)` + strip; numbers stringify).
fn char_field_value(value: &Value) -> String {
    match value {
        Value::String(text) => py_strip(text).to_owned(),
        Value::Number(number) => python_number_str(number),
        _ => String::new(),
    }
}

/// Parse a JSON value the way `int(re_decimal.sub('', str(data)))` does
/// (`IntegerField.to_internal_value`, `fields.py:910-918`): an optional
/// `.0*` + whitespace suffix is stripped, then Python `int()` runs
/// (whitespace, sign, digits with single underscores between digits).
/// Returns the unbounded value; out-of-`i64` magnitudes saturate (the
/// min/max arms keep their verdicts; `sequence_id` carries its own
/// overflow flag for the save-time 500).
fn parse_drf_int(value: &Value) -> Result<i64, &'static str> {
    parse_drf_int_full(value).map(|(number, _)| number)
}

/// [`parse_drf_int`] plus the save-time overflow flag: true when the
/// validated magnitude exceeds `i64` (Python validates it fine; the
/// `int4` column raises at save → 500).
fn parse_drf_int_full(value: &Value) -> Result<(i64, bool), &'static str> {
    const MAX_STRING_LENGTH: usize = 1000;
    if let Value::String(text) = value {
        if text.len() > MAX_STRING_LENGTH {
            return Err("String value too large.");
        }
    }
    let text: String = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => python_number_str(number),
        _ => return Err("A valid integer is required."),
    };
    // `re_decimal = re.compile(r'\.0*\s*$')`: only a trailing `.0*`
    // suffix strips (the `$` anchors; `re.sub` scans but only the last
    // dot can match to the end).
    let mut core = text.as_str();
    if let Some(dot) = core.rfind('.') {
        let (head, tail) = core.split_at(dot);
        let tail = &tail[1..];
        let mut zeros = tail;
        while zeros.starts_with('0') {
            zeros = &zeros[1..];
        }
        if py_strip(zeros).is_empty() {
            core = head;
        }
    }
    let number = parse_python_int(core).ok_or("A valid integer is required.")?;
    Ok((number, python_int_overflows_i64(core)))
}

/// Python `int(text, 10)`: surrounding whitespace, an optional sign, then
/// digits with single `_` separators between digits only. Saturates past
/// `i64` (see [`parse_drf_int`]).
pub fn parse_python_int(text: &str) -> Option<i64> {
    // `int()` strips exactly `str.strip()`.
    let text = py_strip(text);
    let (negative, digits) = match text.strip_prefix(['+', '-']) {
        Some(rest) => (text.starts_with('-'), rest),
        None => (false, text),
    };
    if digits.is_empty() {
        return None;
    }
    // Single underscores between digits only (`int('1__0')` fails).
    let mut cleaned = String::with_capacity(digits.len());
    let mut prev_underscore = true;
    for ch in digits.chars() {
        if ch == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        prev_underscore = false;
        cleaned.push(ch);
    }
    if prev_underscore {
        return None;
    }
    let magnitude: i128 = cleaned.parse().ok()?;
    let signed = if negative { -magnitude } else { magnitude };
    Some(signed.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

/// Whether `parse_python_int` saturated (the magnitude exceeds `i64`):
/// the save-time overflow arm for `sequence_id`.
fn python_int_overflows_i64(text: &str) -> bool {
    let text = py_strip(text);
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let cleaned: String = digits.chars().filter(|ch| *ch != '_').collect();
    let trimmed = cleaned.trim_start_matches('0');
    if trimmed.len() > 19 {
        return true;
    }
    if trimmed.len() < 19 {
        return false;
    }
    if text.starts_with('-') {
        trimmed > "9223372036854775808"
    } else {
        trimmed > "9223372036854775807"
    }
}

/// Parse a JSON value the way `float(data)` does
/// (`FloatField.to_internal_value`, `fields.py:947-957`). Gigantic ints
/// overflow (`OverflowError`); `inf`/`nan` spellings are valid (the render
/// layer 500s them, per the S1 contract).
fn parse_drf_float(value: &Value) -> Result<f64, &'static str> {
    const MAX_STRING_LENGTH: usize = 1000;
    match value {
        Value::Bool(true) => Ok(1.0),
        Value::Bool(false) => Ok(0.0),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return Ok(int as f64);
            }
            if let Some(uint) = number.as_u64() {
                return Ok(uint as f64);
            }
            if number.is_f64() {
                // JSON `1e999` parses to inf in Python too — valid here.
                return Ok(number.as_f64().unwrap_or(f64::NAN));
            }
            // Arbitrary-precision integers past `u64`: `float()` overflows
            // past `f64::MAX`, else converts.
            let raw = number.to_string();
            match raw.parse::<f64>() {
                Ok(finite) if finite.is_finite() => Ok(finite),
                _ => Err("Integer value too large to convert to float"),
            }
        }
        Value::String(text) => {
            if text.len() > MAX_STRING_LENGTH {
                return Err("String value too large.");
            }
            parse_python_float(text).ok_or("A valid number is required.")
        }
        _ => Err("A valid number is required."),
    }
}

/// Python `float(text)`: whitespace, sign, digits with underscores,
/// fractions, exponents, `inf`/`infinity`/`nan` in any case.
fn parse_python_float(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lowered = text.to_ascii_lowercase();
    for special in ["inf", "infinity", "nan"] {
        if lowered == special
            || lowered == format!("+{special}")
            || lowered == format!("-{special}")
        {
            let negative = lowered.starts_with('-');
            return Some(match special {
                "nan" => f64::NAN,
                _ if negative => f64::NEG_INFINITY,
                _ => f64::INFINITY,
            });
        }
    }
    // Underscores between digits/around the point only; Rust parses the rest.
    let mut cleaned = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let ch = bytes[index] as char;
        if ch == '_' {
            let prev = index.checked_sub(1).map(|i| bytes[i] as char);
            let next = bytes.get(index + 1).map(|b| *b as char);
            let ok_prev = matches!(prev, Some(c) if c.is_ascii_digit() || c == '.');
            let ok_next = matches!(next, Some(c) if c.is_ascii_digit() || c == '.');
            if !(ok_prev && ok_next) {
                return None;
            }
            index += 1;
            continue;
        }
        cleaned.push(ch);
        index += 1;
    }
    cleaned.parse::<f64>().ok()
}

/// Parse a JSON value the way `BooleanField.to_internal_value` does
/// (`fields.py:700-707`): the exact `TRUE_VALUES` / `FALSE_VALUES` sets
/// (note `0.0` is false but `1.0` is invalid, and `1`/`0` are ints only).
fn parse_drf_bool(value: &Value) -> Result<bool, &'static str> {
    const INVALID: &str = "Must be a valid boolean.";
    match value {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => {
            if number.as_i64() == Some(1) {
                return Ok(true);
            }
            if number.as_i64() == Some(0) {
                return Ok(false);
            }
            if number.as_f64() == Some(0.0) {
                return Ok(false);
            }
            Err(INVALID)
        }
        Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "t" | "y" | "yes" | "true" | "on" | "1" => Ok(true),
            "f" | "n" | "no" | "false" | "off" | "0" => Ok(false),
            _ => Err(INVALID),
        },
        _ => Err(INVALID),
    }
}

// ---------------------------------------------------------------------------
// DRF date / datetime / choice / pk fields
// ---------------------------------------------------------------------------

/// `DateField` invalid message (`fields.py:1219` over
/// `DATE_INPUT_FORMATS = [iso-8601]`).
const DATE_INVALID_MESSAGE: &str =
    "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.";
/// `DateTimeField` invalid message (`fields.py:1129` over
/// `DATETIME_INPUT_FORMATS = [iso-8601]`).
const DATETIME_INVALID_MESSAGE: &str =
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

/// Parse a JSON value the way `DateField.to_internal_value` does
/// (`fields.py:1231-1258`): `parse_date` (fromisoformat, then the
/// `date_re` fallback) or the invalid message. Non-strings always miss
/// (`TypeError` suppressed).
fn parse_drf_date(value: &Value) -> Result<NaiveDate, &'static str> {
    match value {
        Value::String(text) => parse_django_date(text).ok_or(DATE_INVALID_MESSAGE),
        _ => Err(DATE_INVALID_MESSAGE),
    }
}

/// `django.utils.dateparse.parse_date` (Django 4.2.30, `dateparse.py:67-78`):
/// `date.fromisoformat` first (extended, basic, ordinal and week dates),
/// then the `date_re` fallback (`YYYY-M-D`, 1-2 digit month/day). Well
/// formatted but impossible (month 13, Feb 30, year 0) is `None` too.
fn parse_django_date(text: &str) -> Option<NaiveDate> {
    fromisodate(text).or_else(|| date_re_fallback(text))
}

/// Python 3.12 `date.fromisoformat`: the four spellings, strictly.
fn fromisodate(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    // Extended `YYYY-MM-DD`.
    if bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        let year: i32 = text[0..4].parse().ok()?;
        let month: u32 = text[5..7].parse().ok()?;
        let day: u32 = text[8..10].parse().ok()?;
        return NaiveDate::from_ymd_opt(year, month, day);
    }
    // Basic `YYYYMMDD`.
    if bytes.len() == 8 && bytes.iter().all(|b| b.is_ascii_digit()) {
        let year: i32 = text[0..4].parse().ok()?;
        let month: u32 = text[4..6].parse().ok()?;
        let day: u32 = text[6..8].parse().ok()?;
        return NaiveDate::from_ymd_opt(year, month, day);
    }
    // Ordinal `YYYY-DDD` / `YYYYDDD`.
    if bytes.len() >= 7 && bytes[0..4].iter().all(|b| b.is_ascii_digit()) && bytes[4] != b'W' {
        let rest = &text[4..];
        if let Some(ordinal) = ordinal_day(rest) {
            let year: i32 = text[0..4].parse().ok()?;
            return NaiveDate::from_yo_opt(year, ordinal);
        }
    }
    // Week `YYYY-Www-D` / `YYYYWwwD` (dash-consistent spellings only).
    isoweek_date(text)
}

/// The ordinal day of a `YYYY-DDD` / `YYYYDDD` tail, if well formed.
fn ordinal_day(rest: &str) -> Option<u32> {
    if let Some(day) = rest.strip_prefix('-') {
        if day.len() == 3 && day.bytes().all(|b| b.is_ascii_digit()) {
            return day.parse().ok();
        }
        return None;
    }
    if rest.len() == 3 && rest.bytes().all(|b| b.is_ascii_digit()) {
        return rest.parse().ok();
    }
    None
}

/// Python 3.12 week-date `fromisoformat`: `YYYY-Www-D` or `YYYYWwwD`.
fn isoweek_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    let (year, week, weekday) =
        if bytes.len() == 10 && bytes[4] == b'-' && bytes[5] == b'W' && bytes[8] == b'-' {
            (
                text[0..4].parse::<i32>().ok()?,
                text[6..8].parse::<u32>().ok()?,
                text[9..10].parse::<u32>().ok()?,
            )
        } else if bytes.len() == 8
            && bytes[0..4].iter().all(|b| b.is_ascii_digit())
            && bytes[4] == b'W'
        {
            (
                text[0..4].parse::<i32>().ok()?,
                text[5..7].parse::<u32>().ok()?,
                text[7..8].parse::<u32>().ok()?,
            )
        } else {
            return None;
        };
    let weekday = chrono::Weekday::try_from(u8::try_from(weekday.saturating_sub(1)).ok()?).ok()?;
    NaiveDate::from_isoywd_opt(year, week, weekday)
}

/// The `date_re` fallback (`dateparse.py:13,76-78`):
/// `^\d{4}-\d{1,2}-\d{1,2}$` then the constructor (which rejects
/// impossible dates and year 0).
fn date_re_fallback(text: &str) -> Option<NaiveDate> {
    let mut parts = text.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !(1..=2).contains(&month.len()) || !month.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !(1..=2).contains(&day.len()) || !day.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

/// Parse a JSON value the way `DateTimeField.to_internal_value` +
/// `enforce_timezone` do (`fields.py:1145-1190`): `parse_datetime`, then
/// naive inputs take the request timezone (`USE_TZ`, the activated user
/// zone) and aware inputs keep their instant. Returns the UTC instant.
fn parse_drf_datetime(value: &Value, tz: &Tz, tz_name: &str) -> Result<DateTime<Utc>, String> {
    let text = match value {
        Value::String(text) => text.as_str(),
        _ => return Err(DATETIME_INVALID_MESSAGE.to_owned()),
    };
    let parsed = pidash_compat::datetime::parse_django_datetime(text)
        .ok_or_else(|| DATETIME_INVALID_MESSAGE.to_owned())?;
    if let Some(offset_micros) = parsed.offset_micros {
        let utc = parsed.naive - chrono::Duration::microseconds(offset_micros);
        return Ok(chrono::DateTime::<Utc>::from_naive_utc_and_offset(utc, Utc));
    }
    // Naive: `make_aware` in the request zone. Gaps answer the
    // `make_aware` message; folds take the earlier side (fold=0).
    match tz.from_local_datetime(&parsed.naive) {
        chrono::LocalResult::Single(aware) => Ok(aware.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
        chrono::LocalResult::None => {
            Err(format!("Invalid datetime for the timezone \"{tz_name}\"."))
        }
    }
}

/// Parse a JSON value the way `ChoiceField.to_internal_value` does
/// (`fields.py:1400-1408`): `str(data)` must be a choice key. The caller
/// handles `""` first (`allow_blank` returns it verbatim). The miss
/// message echoes the raw input (`"{input}" is not a valid choice.`).
fn parse_drf_choice<'a>(value: &Value, choices: &[&'a str]) -> Result<&'a str, String> {
    let key = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => python_number_str(number),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
    };
    match choices.iter().find(|choice| **choice == key) {
        Some(choice) => Ok(*choice),
        None => Err(format!("\"{key}\" is not a valid choice.")),
    }
}

/// `uuid.UUID(value)` for the `hex`/`int` input forms
/// (`django/db/models/fields/__init__.py:2684-2691`): braces, `urn:uuid:`
/// prefixes and hyphens-anywhere strip, then 32 hex chars; ints take the
/// `int=` form (negative or >2¹²⁸ fails). Anything else fails exactly like
/// Django's `ValidationError` arm.
fn parse_uuid_input(value: &Value) -> Result<Uuid, ()> {
    match value {
        Value::String(text) => parse_uuid_hex(text).ok_or(()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(());
                }
                return Ok(Uuid::from_u128(int as u128));
            }
            if let Some(uint) = number.as_u64() {
                return Ok(Uuid::from_u128(u128::from(uint)));
            }
            // Arbitrary-precision integers: the `int=` form up to 2¹²⁸-1.
            if !number.is_f64() {
                if let Ok(big) = number.to_string().parse::<u128>() {
                    return Ok(Uuid::from_u128(big));
                }
            }
            Err(())
        }
        _ => Err(()),
    }
}

/// The `hex` form of `uuid.UUID` (CPython 3.12 `uuid.py`): every `urn:`
/// and `uuid:` substring is removed (case-sensitive), brace characters
/// strip from both ends, hyphens strip anywhere, then exactly 32 hex
/// digits (case-insensitive) must remain. No whitespace stripping.
fn parse_uuid_hex(text: &str) -> Option<Uuid> {
    let mut hex = text.replace("urn:", "").replace("uuid:", "");
    hex = hex
        .trim_matches(|ch| ch == '{' || ch == '}')
        .replace('-', "");
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Uuid::parse_str(&hex).ok()
}

/// Django's `“%(value)s” is not a valid UUID.` field message for a PK
/// input that fails `to_python` (fancy quotes verbatim).
fn invalid_uuid_message(value: &Value) -> String {
    match value {
        Value::String(text) => format!("\u{201c}{text}\u{201d} is not a valid UUID."),
        _ => format!("\u{201c}{}\u{201d} is not a valid UUID.", py_repr(value)),
    }
}

// ---------------------------------------------------------------------------
// Issue write parsing (`IssueSerializer.to_internal_value`)
// ---------------------------------------------------------------------------

/// The validated write: every writable field as `Option` (`None` = absent;
/// nullable fields nest a second `Option` for explicit null). Defaults are
/// applied by the caller on create only (partial skips absent keys).
#[derive(Debug, Clone, Default)]
struct ParsedWrite {
    assignees: Option<Vec<Uuid>>,
    labels: Option<Vec<Uuid>>,
    issue_type: Option<Option<Uuid>>,
    deleted_at: Option<Option<DateTime<Utc>>>,
    point: Option<Option<i64>>,
    name: Option<String>,
    description_html: Option<String>,
    priority: Option<String>,
    complexity_score: Option<i64>,
    start_date: Option<Option<NaiveDate>>,
    target_date: Option<Option<NaiveDate>>,
    sequence_id: Option<i64>,
    sequence_overflow: bool,
    sort_order: Option<f64>,
    completed_at: Option<Option<DateTime<Utc>>>,
    archived_at: Option<Option<NaiveDate>>,
    is_draft: Option<bool>,
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    git_work_branch: Option<String>,
    created_via: Option<Option<String>>,
    agent_executor: Option<Option<String>>,
    created_by: Option<Option<Uuid>>,
    parent: Option<Option<Uuid>>,
    state: Option<Option<Uuid>>,
    estimate_point: Option<Option<Uuid>>,
    assigned_pod: Option<Option<Uuid>>,
}

/// One queued existence probe: the parsed UUID plus the raw echo for the
/// miss message.
#[derive(Debug, Clone)]
struct PkProbe {
    id: Uuid,
    raw: String,
}

/// The tables `PrimaryKeyRelatedField` existence checks read (each with
/// its Python manager scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PkTable {
    Users,
    Labels,
    IssueTypes,
    States,
    Issues,
    EstimatePoints,
    Pods,
}

/// Batch one table's probes: returns the ids that exist under the
/// field's manager scope.
async fn probe_exists(pool: &PgPool, table: PkTable, ids: &[Uuid]) -> Result<Vec<Uuid>, Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = match table {
        // `User.objects` is Django's plain `UserManager` (no liveness
        // scope — deactivated users still validate).
        PkTable::Users => r#"SELECT "id" FROM "users" WHERE "id" = ANY($1)"#,
        PkTable::Labels => {
            r#"SELECT "id" FROM "labels" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
        PkTable::IssueTypes => {
            r#"SELECT "id" FROM "issue_types" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
        // `State.objects` is the triage-excluding `StateManager`
        // (`db/models/state.py:91-100`): triage ids miss here (field
        // error), never reaching `validate()`.
        PkTable::States => {
            r#"SELECT "id" FROM "states" WHERE "id" = ANY($1) AND "deleted_at" IS NULL AND NOT ("group" = 'triage')"#
        }
        PkTable::Issues => {
            r#"SELECT "id" FROM "issues" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
        PkTable::EstimatePoints => {
            r#"SELECT "id" FROM "estimate_points" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#
        }
        // `Pod.objects` is the soft-deletion `PodManager`.
        PkTable::Pods => r#"SELECT "id" FROM "pod" WHERE "id" = ANY($1) AND "deleted_at" IS NULL"#,
    };
    sqlx::query_scalar(sql)
        .bind(ids)
        .fetch_all(pool)
        .await
        .map_err(|error| db_error(error, "pk-exists"))
}

/// Parse one PK input (`PrimaryKeyRelatedField.to_internal_value`,
/// `relations.py:235-261`): bools fail the type arm, `uuid.UUID`
/// failures become the Django `"..." is not a valid UUID.` message
/// (caught per-field by the serializer), successes queue an existence
/// probe.
fn parse_pk_input(value: &Value) -> Result<PkProbe, String> {
    if value.is_boolean() {
        return Err("Incorrect type. Expected pk value, received bool.".to_owned());
    }
    match parse_uuid_input(value) {
        Ok(id) => Ok(PkProbe {
            id,
            raw: match value {
                Value::String(text) => text.clone(),
                _ => py_repr(value),
            },
        }),
        Err(()) => Err(invalid_uuid_message(value)),
    }
}

/// Parse one `assignees`/`labels` list (`ListField`, `fields.py:1772+`):
/// non-lists fail `not_a_list`, children parse as PKs, child failures
/// collect per index.
enum ListParse {
    Value(Vec<PkProbe>),
    Messages(Vec<String>),
    Indexed(Vec<(usize, Vec<String>)>),
}

fn parse_pk_list(value: &Value) -> ListParse {
    match value {
        Value::Array(items) => {
            let mut probes = Vec::with_capacity(items.len());
            let mut failures: Vec<(usize, Vec<String>)> = Vec::new();
            for (index, item) in items.iter().enumerate() {
                match parse_pk_input(item) {
                    Ok(probe) => probes.push(probe),
                    Err(message) => failures.push((index, vec![message])),
                }
            }
            if failures.is_empty() {
                ListParse::Value(probes)
            } else {
                ListParse::Indexed(failures)
            }
        }
        Value::Null => ListParse::Messages(vec!["This field may not be null.".to_owned()]),
        _ => ListParse::Messages(vec![format!(
            "Expected a list of items but got type \"{}\".",
            json_type_name(value)
        )]),
    }
}

/// The parsed shape of one `assignees`/`labels` list.
enum ListShape {
    Absent,
    Value(Vec<PkProbe>),
    Messages(Vec<String>),
    Indexed(Vec<(usize, Vec<String>)>),
}

/// Parse an optional `assignees`/`labels` list.
fn parse_optional_list(value: Option<&Value>) -> ListShape {
    match value {
        None => ListShape::Absent,
        Some(value) => match parse_pk_list(value) {
            ListParse::Value(probes) => ListShape::Value(probes),
            ListParse::Messages(messages) => ListShape::Messages(messages),
            ListParse::Indexed(failures) => ListShape::Indexed(failures),
        },
    }
}

/// The parsed shape of one field before existence probing: either a value
/// (possibly with queued probes) or immediate messages.
#[derive(Debug)]
enum FieldParse<T> {
    Absent,
    Value(T),
    Errors(Vec<String>),
}

/// Parse the normalized write body into [`ParsedWrite`]: shape first (in
/// `FIELDS_IN_ORDER`), then batched existence probes, then the ordered
/// error dict (or the value). `partial` is the PATCH flag (absent keys
/// skip; defaults never apply). `from_form` selects the HTML `get_value`
/// blank arms for the nullable fields (`''` → null).
async fn parse_issue_write(
    pool: &PgPool,
    body: &Map<String, Value>,
    partial: bool,
    from_form: bool,
    tz: &Tz,
    tz_name: &str,
) -> Result<ParsedWrite, Error> {
    // Per-field shape results, in `FIELDS_IN_ORDER` (declared
    // `assignees`, `labels`, `type_id` first, then model order):
    // read-only (`id`, `url`, `created_at`, `updated_at`, `updated_by`,
    // `project`, `workspace`, `description_binary`) and unknown keys are
    // ignored, exactly like DRF's writable-fields iteration.
    let get = |key: &str| body.get(key);

    // --- assignees / labels (ListField, required=False) ---
    let assignees_shape = parse_optional_list(get("assignees"));
    let labels_shape = parse_optional_list(get("labels"));

    // --- type_id (declared PKRF, source=type, required=False, allow_null) ---
    let type_id_shape = parse_optional_pk(get("type_id"), from_form);

    // --- deleted_at (DateTime, allow_null) ---
    let deleted_at_shape = parse_optional_datetime(get("deleted_at"), from_form, tz, tz_name);

    // --- point (Integer, allow_null, 0..=12) ---
    let point_shape = parse_optional_int(get("point"), from_form, true, Some((0, 12)));

    // --- name (CharField max 255, REQUIRED) ---
    let name_shape: FieldParse<String> = match get("name") {
        None if partial => FieldParse::Absent,
        None => FieldParse::Errors(vec!["This field is required.".to_owned()]),
        Some(value) => {
            let errors = char_field_errors(value, false, false, Some(255));
            if errors.is_empty() {
                FieldParse::Value(char_field_value(value))
            } else {
                FieldParse::Errors(errors)
            }
        }
    };

    // --- description_html (TextField, allow_blank, default "<p></p>") ---
    let description_html_shape: FieldParse<String> = match get("description_html") {
        None => FieldParse::Absent,
        Some(value) => {
            let errors = char_field_errors(value, true, false, None);
            if errors.is_empty() {
                FieldParse::Value(char_field_value(value))
            } else {
                FieldParse::Errors(errors)
            }
        }
    };

    // --- priority (Choice, default "none") ---
    let priority_shape: FieldParse<String> = match get("priority") {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                FieldParse::Errors(vec!["This field may not be null.".to_owned()])
            } else {
                match parse_drf_choice(value, &["urgent", "high", "medium", "low", "none"]) {
                    Ok(choice) => FieldParse::Value(choice.to_owned()),
                    Err(message) => FieldParse::Errors(vec![message]),
                }
            }
        }
    };

    // --- complexity_score (Integer, default 0, 0..=10) ---
    let complexity_shape: FieldParse<i64> =
        match parse_optional_int(get("complexity_score"), from_form, false, Some((0, 10))) {
            FieldParse::Absent => FieldParse::Absent,
            FieldParse::Value(Some(number)) => FieldParse::Value(number),
            // Non-nullable: null errors above, so `None` is unreachable.
            FieldParse::Value(None) => {
                FieldParse::Errors(vec!["This field may not be null.".to_owned()])
            }
            FieldParse::Errors(messages) => FieldParse::Errors(messages),
        };

    // --- start_date / target_date (Date, allow_null) ---
    let start_shape = parse_optional_date(get("start_date"), from_form);
    let target_shape = parse_optional_date(get("target_date"), from_form);

    // --- sequence_id (Integer, default 1) ---
    let sequence_shape = parse_optional_int_full(get("sequence_id"), from_form);

    // --- sort_order (Float, default 65535) ---
    let sort_shape: FieldParse<f64> = match get("sort_order") {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                FieldParse::Errors(vec!["This field may not be null.".to_owned()])
            } else {
                match parse_drf_float(value) {
                    Ok(number) => FieldParse::Value(number),
                    Err(message) => FieldParse::Errors(vec![message.to_owned()]),
                }
            }
        }
    };

    // --- completed_at (DateTime, allow_null) ---
    let completed_shape = parse_optional_datetime(get("completed_at"), from_form, tz, tz_name);

    // --- archived_at (Date, allow_null) ---
    let archived_shape = parse_optional_date(get("archived_at"), from_form);

    // --- is_draft (Boolean, default false) ---
    let draft_shape: FieldParse<bool> = match get("is_draft") {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                FieldParse::Errors(vec!["This field may not be null.".to_owned()])
            } else {
                match parse_drf_bool(value) {
                    Ok(flag) => FieldParse::Value(flag),
                    Err(message) => FieldParse::Errors(vec![message.to_owned()]),
                }
            }
        }
    };

    // --- external_source / external_id (Char 255, allow_null+allow_blank) ---
    let ext_source_shape = parse_optional_char(get("external_source"), Some(255));
    let ext_id_shape = parse_optional_char(get("external_id"), Some(255));

    // --- git_work_branch (Char 128, allow_blank, default "", +RegexValidator) ---
    let branch_shape: FieldParse<String> = match get("git_work_branch") {
        None => FieldParse::Absent,
        Some(value) => {
            let shape = char_shape_errors(value, true, false);
            if shape.is_empty() {
                let coerced = char_field_value(value);
                let errors = branch_errors(&coerced);
                if errors.is_empty() {
                    FieldParse::Value(coerced)
                } else {
                    FieldParse::Errors(errors)
                }
            } else {
                FieldParse::Errors(shape)
            }
        }
    };

    // --- created_via (Char 32, allow_null+allow_blank) ---
    let via_shape = parse_optional_char(get("created_via"), Some(32));

    // --- agent_executor (Choice, allow_null+allow_blank) ---
    let executor_shape: FieldParse<Option<String>> = match get("agent_executor") {
        None => FieldParse::Absent,
        Some(value) => {
            // HTML `''` with `allow_blank` stays `''` (never the null arm).
            if value.is_null() {
                FieldParse::Value(None)
            } else if value == &Value::String(String::new()) {
                FieldParse::Value(Some(String::new()))
            } else {
                match parse_drf_choice(value, &["local_runner", "cloud_agent", "managed_runner"]) {
                    Ok(choice) => FieldParse::Value(Some(choice.to_owned())),
                    Err(message) => FieldParse::Errors(vec![message]),
                }
            }
        }
    };

    // --- created_by (PKRF User, allow_null) ---
    let created_by_shape = parse_optional_pk(get("created_by"), from_form);

    // --- parent (PKRF Issue, allow_null) ---
    let parent_shape = parse_optional_pk(get("parent"), from_form);

    // --- state (PKRF State, allow_null) ---
    let state_shape = parse_optional_pk(get("state"), from_form);

    // --- estimate_point (PKRF EstimatePoint, allow_null) ---
    let estimate_shape = parse_optional_pk(get("estimate_point"), from_form);

    // --- type (auto PKRF, source=type, allow_null; wins over type_id) ---
    let type_shape = parse_optional_pk(get("type"), from_form);

    // --- assigned_pod (PKRF Pod, allow_null) ---
    let pod_shape = parse_optional_pk(get("assigned_pod"), from_form);

    // Batch the existence probes (one query per table).
    let mut assignee_ids: Vec<Uuid> = Vec::new();
    let mut label_ids: Vec<Uuid> = Vec::new();
    let mut user_ids: Vec<Uuid> = Vec::new();
    let mut type_ids: Vec<Uuid> = Vec::new();
    let mut state_ids: Vec<Uuid> = Vec::new();
    let mut issue_ids: Vec<Uuid> = Vec::new();
    let mut estimate_ids: Vec<Uuid> = Vec::new();
    let mut pod_ids: Vec<Uuid> = Vec::new();
    if let ListShape::Value(probes) = &assignees_shape {
        assignee_ids.extend(probes.iter().map(|probe| probe.id));
    }
    if let ListShape::Value(probes) = &labels_shape {
        label_ids.extend(probes.iter().map(|probe| probe.id));
    }
    if let FieldParse::Value(Some(probe)) = &type_id_shape {
        type_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &type_shape {
        type_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &created_by_shape {
        user_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &parent_shape {
        issue_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &state_shape {
        state_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &estimate_shape {
        estimate_ids.push(probe.id);
    }
    if let FieldParse::Value(Some(probe)) = &pod_shape {
        pod_ids.push(probe.id);
    }
    let assignee_ids = dedup(assignee_ids);
    let label_ids = dedup(label_ids);
    let user_ids = dedup(user_ids);
    let type_ids = dedup(type_ids);
    let state_ids = dedup(state_ids);
    let issue_ids = dedup(issue_ids);
    let estimate_ids = dedup(estimate_ids);
    let pod_ids = dedup(pod_ids);
    let (
        assignees_live,
        labels_live,
        users_live,
        types_live,
        states_live,
        issues_live,
        estimates_live,
        pods_live,
    ) = tokio::join!(
        probe_exists(pool, PkTable::Users, &assignee_ids),
        probe_exists(pool, PkTable::Labels, &label_ids),
        probe_exists(pool, PkTable::Users, &user_ids),
        probe_exists(pool, PkTable::IssueTypes, &type_ids),
        probe_exists(pool, PkTable::States, &state_ids),
        probe_exists(pool, PkTable::Issues, &issue_ids),
        probe_exists(pool, PkTable::EstimatePoints, &estimate_ids),
        probe_exists(pool, PkTable::Pods, &pod_ids),
    );
    let assignees_live = assignees_live?;
    let labels_live = labels_live?;
    let users_live = users_live?;
    let types_live = types_live?;
    let states_live = states_live?;
    let issues_live = issues_live?;
    let estimates_live = estimates_live?;
    let pods_live = pods_live?;
    let has = |live: &[Uuid], id: &Uuid| live.contains(id);

    // Assemble the ordered error dict (or the value). `type` wins over
    // `type_id` for the value; both report errors under their own names.
    let mut entries: Vec<(&str, Value)> = Vec::new();
    let mut parsed = ParsedWrite::default();

    // assignees
    match assignees_shape {
        ListShape::Absent => {}
        ListShape::Value(probes) => {
            let mut failures: Vec<(usize, Vec<String>)> = Vec::new();
            let mut ids = Vec::with_capacity(probes.len());
            for (index, probe) in probes.iter().enumerate() {
                if has(&assignees_live, &probe.id) {
                    ids.push(probe.id);
                } else {
                    failures.push((
                        index,
                        vec![format!(
                            "Invalid pk \"{}\" - object does not exist.",
                            probe.raw
                        )],
                    ));
                }
            }
            if failures.is_empty() {
                parsed.assignees = Some(ids);
            } else {
                entries.push(("assignees", index_errors_value(failures)));
            }
        }
        ListShape::Messages(messages) => {
            entries.push((
                "assignees",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
        ListShape::Indexed(failures) => {
            entries.push(("assignees", index_errors_value(failures)));
        }
    }
    // labels
    match labels_shape {
        ListShape::Absent => {}
        ListShape::Value(probes) => {
            let mut failures: Vec<(usize, Vec<String>)> = Vec::new();
            let mut ids = Vec::with_capacity(probes.len());
            for (index, probe) in probes.iter().enumerate() {
                if has(&labels_live, &probe.id) {
                    ids.push(probe.id);
                } else {
                    failures.push((
                        index,
                        vec![format!(
                            "Invalid pk \"{}\" - object does not exist.",
                            probe.raw
                        )],
                    ));
                }
            }
            if failures.is_empty() {
                parsed.labels = Some(ids);
            } else {
                entries.push(("labels", index_errors_value(failures)));
            }
        }
        ListShape::Messages(messages) => {
            entries.push((
                "labels",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
        ListShape::Indexed(failures) => {
            entries.push(("labels", index_errors_value(failures)));
        }
    }
    // type_id
    let mut type_value: Option<Option<Uuid>> = None;
    match type_id_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            type_value = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&types_live, &probe.id) {
                type_value = Some(Some(probe.id));
            } else {
                entries.push((
                    "type_id",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "type_id",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // type (wins over type_id for the value)
    match type_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            type_value = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&types_live, &probe.id) {
                type_value = Some(Some(probe.id));
            } else {
                entries.push((
                    "type",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "type",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // NOTE: `type` errors assemble here in field order, but the value
    // merge happens after every field below declares its shape — the
    // push order below follows `FIELDS_IN_ORDER` exactly. `type`'s entry
    // above is currently misplaced (it must come after `estimate_point`);
    // collect it aside and re-insert in order at the end.
    let mut type_entry: Option<(&str, Value)> = None;
    if entries.last().map(|(name, _)| *name) == Some("type") {
        type_entry = entries.pop();
    }
    parsed.issue_type = type_value;

    macro_rules! scalar_field {
        ($shape:expr, $name:literal, $slot:expr) => {
            match $shape {
                FieldParse::Absent => {}
                FieldParse::Value(value) => {
                    $slot = Some(value);
                }
                FieldParse::Errors(messages) => {
                    entries.push((
                        $name,
                        Value::Array(messages.into_iter().map(Value::String).collect()),
                    ));
                }
            }
        };
    }
    scalar_field!(deleted_at_shape, "deleted_at", parsed.deleted_at);
    scalar_field!(point_shape, "point", parsed.point);
    scalar_field!(name_shape, "name", parsed.name);
    scalar_field!(
        description_html_shape,
        "description_html",
        parsed.description_html
    );
    scalar_field!(priority_shape, "priority", parsed.priority);
    scalar_field!(
        complexity_shape,
        "complexity_score",
        parsed.complexity_score
    );
    scalar_field!(start_shape, "start_date", parsed.start_date);
    scalar_field!(target_shape, "target_date", parsed.target_date);
    match sequence_shape {
        FieldParse::Absent => {}
        FieldParse::Value((number, overflow)) => {
            parsed.sequence_id = Some(number);
            parsed.sequence_overflow = overflow;
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "sequence_id",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    scalar_field!(sort_shape, "sort_order", parsed.sort_order);
    scalar_field!(completed_shape, "completed_at", parsed.completed_at);
    scalar_field!(archived_shape, "archived_at", parsed.archived_at);
    scalar_field!(draft_shape, "is_draft", parsed.is_draft);
    scalar_field!(ext_source_shape, "external_source", parsed.external_source);
    scalar_field!(ext_id_shape, "external_id", parsed.external_id);
    scalar_field!(branch_shape, "git_work_branch", parsed.git_work_branch);
    scalar_field!(via_shape, "created_via", parsed.created_via);
    scalar_field!(executor_shape, "agent_executor", parsed.agent_executor);
    // created_by
    match created_by_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.created_by = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&users_live, &probe.id) {
                parsed.created_by = Some(Some(probe.id));
            } else {
                entries.push((
                    "created_by",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "created_by",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // parent
    match parent_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.parent = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&issues_live, &probe.id) {
                parsed.parent = Some(Some(probe.id));
            } else {
                entries.push((
                    "parent",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "parent",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // state
    match state_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.state = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&states_live, &probe.id) {
                parsed.state = Some(Some(probe.id));
            } else {
                entries.push((
                    "state",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "state",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // estimate_point
    match estimate_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.estimate_point = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&estimates_live, &probe.id) {
                parsed.estimate_point = Some(Some(probe.id));
            } else {
                entries.push((
                    "estimate_point",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "estimate_point",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }
    // `type` re-inserted in field order (after `estimate_point`).
    if let Some(entry) = type_entry {
        entries.push(entry);
    }
    // assigned_pod
    match pod_shape {
        FieldParse::Absent => {}
        FieldParse::Value(None) => {
            parsed.assigned_pod = Some(None);
        }
        FieldParse::Value(Some(probe)) => {
            if has(&pods_live, &probe.id) {
                parsed.assigned_pod = Some(Some(probe.id));
            } else {
                entries.push((
                    "assigned_pod",
                    Value::Array(vec![Value::String(format!(
                        "Invalid pk \"{}\" - object does not exist.",
                        probe.raw
                    ))]),
                ));
            }
        }
        FieldParse::Errors(messages) => {
            entries.push((
                "assigned_pod",
                Value::Array(messages.into_iter().map(Value::String).collect()),
            ));
        }
    }

    if entries.is_empty() {
        Ok(parsed)
    } else {
        Err(Error::FieldErrors(shape::field_errors_body(&entries)))
    }
}

/// Deduplicate batched probe ids (stable order).
fn dedup(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids.dedup();
    ids
}

/// Build the `{index: [messages]}` detail for `ListField` child failures
/// (indices ascend; `preserve_order` keeps insertion order, and callers
/// pass ascending indices).
fn index_errors_value(failures: Vec<(usize, Vec<String>)>) -> Value {
    let mut detail = Map::new();
    for (index, messages) in failures {
        detail.insert(
            index.to_string(),
            Value::Array(messages.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(detail)
}

/// Parse an optional nullable PK: absent → `Absent`, null → `None`, form
/// `''` → `None` (`allow_null` arm of `get_value`), else the probe (or
/// its message).
fn parse_optional_pk(value: Option<&Value>, from_form: bool) -> FieldParse<Option<PkProbe>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Value(None);
            }
            if from_form && value == &Value::String(String::new()) {
                return FieldParse::Value(None);
            }
            match parse_pk_input(value) {
                Ok(probe) => FieldParse::Value(Some(probe)),
                Err(message) => FieldParse::Errors(vec![message]),
            }
        }
    }
}

/// Parse an optional integer with optional min/max validators
/// (`point`: nullable; `complexity_score`: non-nullable). Form `''` on
/// the nullable field coerces to the null arm; non-nullables never see
/// `''` (`skip_blank_fields` drops it first).
fn parse_optional_int(
    value: Option<&Value>,
    from_form: bool,
    allow_null: bool,
    min_max: Option<(i64, i64)>,
) -> FieldParse<Option<i64>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                if !allow_null {
                    return FieldParse::Errors(vec!["This field may not be null.".to_owned()]);
                }
                return FieldParse::Value(None);
            }
            if from_form && value == &Value::String(String::new()) {
                debug_assert!(allow_null);
                return FieldParse::Value(None);
            }
            match parse_drf_int(value) {
                Ok(number) => {
                    if let Some((min, max)) = min_max {
                        if number < min {
                            return FieldParse::Errors(vec![format!(
                                "Ensure this value is greater than or equal to {min}."
                            )]);
                        }
                        if number > max {
                            return FieldParse::Errors(vec![format!(
                                "Ensure this value is less than or equal to {max}."
                            )]);
                        }
                    }
                    FieldParse::Value(Some(number))
                }
                Err(message) => FieldParse::Errors(vec![message.to_owned()]),
            }
        }
    }
}

/// Parse the optional `sequence_id` (non-nullable, no min/max): null
/// errors; the overflow flag rides along for the save-time 500.
fn parse_optional_int_full(value: Option<&Value>, from_form: bool) -> FieldParse<(i64, bool)> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Errors(vec!["This field may not be null.".to_owned()]);
            }
            debug_assert!(!(from_form && value == &Value::String(String::new())));
            match parse_drf_int_full(value) {
                Ok(pair) => FieldParse::Value(pair),
                Err(message) => FieldParse::Errors(vec![message.to_owned()]),
            }
        }
    }
}

/// Parse an optional nullable date: absent → `Absent`, null → `None`,
/// form `''` → `None`, else the date (or its message).
fn parse_optional_date(value: Option<&Value>, from_form: bool) -> FieldParse<Option<NaiveDate>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Value(None);
            }
            if from_form && value == &Value::String(String::new()) {
                return FieldParse::Value(None);
            }
            match parse_drf_date(value) {
                Ok(day) => FieldParse::Value(Some(day)),
                Err(message) => FieldParse::Errors(vec![message.to_owned()]),
            }
        }
    }
}

/// Parse an optional nullable datetime (naive → request zone).
fn parse_optional_datetime(
    value: Option<&Value>,
    from_form: bool,
    tz: &Tz,
    tz_name: &str,
) -> FieldParse<Option<DateTime<Utc>>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Value(None);
            }
            if from_form && value == &Value::String(String::new()) {
                return FieldParse::Value(None);
            }
            match parse_drf_datetime(value, tz, tz_name) {
                Ok(instant) => FieldParse::Value(Some(instant)),
                Err(message) => FieldParse::Errors(vec![message]),
            }
        }
    }
}

/// Parse an optional `allow_blank` + `allow_null` char (`external_source`,
/// `external_id`, `created_via`): absent → `Absent`, null → `None`,
/// else the coerced string (or its messages).
fn parse_optional_char(
    value: Option<&Value>,
    max_length: Option<usize>,
) -> FieldParse<Option<String>> {
    match value {
        None => FieldParse::Absent,
        Some(value) => {
            if value.is_null() {
                return FieldParse::Value(None);
            }
            let errors = char_field_errors(value, true, true, max_length);
            if errors.is_empty() {
                FieldParse::Value(Some(char_field_value(value)))
            } else {
                FieldParse::Errors(errors)
            }
        }
    }
}

/// The `git_work_branch` validators in run order: the model's
/// `RegexValidator` first, then `MaxLengthValidator`, then the
/// null-characters check (`fields.py:run_validators` collects all).
fn branch_errors(coerced: &str) -> Vec<String> {
    let mut errors = Vec::new();
    // `RegexValidator(regex=r"^[A-Za-z0-9._/-]*$")` — `[^...]` scan
    // avoids the `regex` crate for a single class.
    if !coerced
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
    {
        errors.push("Branch name may contain only letters, numbers, and . _ / -".to_owned());
    }
    if coerced.chars().count() > 128 {
        errors.push("Ensure this field has no more than 128 characters.".to_owned());
    }
    if coerced.contains('\0') {
        errors.push("Null characters are not allowed.".to_owned());
    }
    errors
}

// ---------------------------------------------------------------------------
// Markdown → Tiptap HTML (`utils/markdown_converter.py:208-469`)
// ---------------------------------------------------------------------------

/// Tiptap's empty document (`markdown_converter.py:205`).
const EMPTY_DOCUMENT_HTML: &str = "<p></p>";

/// markdown-it's `escapeHtml`: exactly `&<>"` (`markdown-it-py`
/// `common/utils.py`). Notably NOT `'`.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// `_is_safe_url` (`markdown_converter.py:208-214`): relative URLs and the
/// sanitizer's schemes only (`SAFE_PROTOCOLS =
/// {"http","https","mailto","tel"}`, `content_validator.py:159`).
/// `urlsplit` strips ASCII tab/CR/LF everywhere, then leading C0 controls
/// and spaces; an invalid IPv6 literal raises → unsafe. The NFKC netloc
/// arm is not ported (it needs Unicode normalization tables; a residual
/// divergence on NFKC-unsafe non-ASCII hosts, where the sanitizer's own
/// independent scheme check still applies).
fn is_safe_url(url: &str) -> bool {
    // Strip ASCII \t\r\n everywhere, then leading C0 + space.
    let scrubbed: String = url
        .chars()
        .filter(|ch| !matches!(ch, '\t' | '\r' | '\n'))
        .collect();
    let scrubbed = scrubbed.trim_start_matches(|ch: char| ch.is_ascii_control() || ch == ' ');
    // Scheme: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`.
    let mut chars = scrubbed.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    if !first.is_ascii_alphabetic() {
        return true;
    }
    let mut end = 1;
    for ch in chars {
        if ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.') {
            end += ch.len_utf8();
        } else if ch == ':' {
            let scheme = scrubbed[..end].to_ascii_lowercase();
            if !matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel") {
                return false;
            }
            // A recognized scheme: only an invalid IPv6 literal still
            // raises (`urlsplit` validates brackets eagerly).
            return ipv6_literally_valid(&scrubbed[end + 1..]);
        } else {
            return true;
        }
    }
    true
}

/// Whether every `[...]` host literal in the post-scheme rest is a valid
/// IPv6 literal (the `urlsplit` eager arm; ports raise → unsafe).
fn ipv6_literally_valid(rest: &str) -> bool {
    // The authority is the `//host` prefix when present.
    let authority = rest
        .strip_prefix("//")
        .map(|after| after.split(['/', '?', '#']).next().unwrap_or(after));
    let Some(authority) = authority else {
        return true;
    };
    // Userinfo splits at the last `@`; zone ids (`%25eth0`) split at `%`.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if !host.starts_with('[') {
        return true;
    }
    let Some(close) = host.find(']') else {
        return false;
    };
    let inner = host[1..close].split('%').next().unwrap_or("");
    // After `]` only `:port` (digits, validated lazily — not here) or
    // nothing may follow; anything else raises too.
    let tail = &host[close + 1..];
    if !(tail.is_empty() || tail.starts_with(':')) {
        return false;
    }
    inner.parse::<std::net::Ipv6Addr>().is_ok()
}

/// One resolved markdown event: pulldown's stream after the task-list
/// pass (markers stripped or restored as literal text).
#[derive(Debug, Clone)]
enum MdEvent {
    StartHeading(u32),
    EndHeading,
    StartParagraph,
    EndParagraph,
    StartQuote,
    EndQuote,
    CodeBlock(Option<String>, String),
    StartList(Option<u64>, bool),
    EndList(bool),
    StartItem(bool, bool),
    EndItem(bool),
    StartTable,
    EndTable,
    StartHead,
    EndHead,
    StartRow,
    EndRow,
    StartCell(bool),
    EndCell(bool),
    StartEm,
    EndEm,
    StartStrong,
    EndStrong,
    StartStrike,
    EndStrike,
    StartLink(String, String),
    EndLink,
    Image(String, String, String),
    Text(String),
    Code(String),
    HtmlInline(String),
    HtmlBlock(String),
    SoftBreak,
    HardBreak,
    Rule,
}

/// Render agent-written markdown as Tiptap `description_html`
/// (`markdown_to_html`, `markdown_converter.py:444-469`): the CommonMark +
/// GFM-tables/strikethrough + tasklists parse, the `TiptapHTMLRenderer`
/// shapes, then `validate_html_content`. `Err` is the `ValueError`
/// message the serializer reports under `description_markdown`.
///
/// Engine note: the parser is pulldown-cmark (CommonMark + the same GFM
/// options) with the renderer rules ported token-for-token below; raw HTML
/// is disabled like `MarkdownIt("commonmark", {"html": False})`. The
/// `differential_markdown_corpus` test pins 228 oracle vectors from the
/// live Python converter.
fn markdown_to_html_port(markdown: &str) -> Result<String, String> {
    use pulldown_cmark::{Options, Parser};
    if markdown.trim().is_empty() {
        return Ok(EMPTY_DOCUMENT_HTML.to_owned());
    }
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<pulldown_cmark::Event> = Parser::new_ext(markdown, options).collect();
    let resolved = resolve_task_events(&events);
    let mut html = String::new();
    render_tiptap(&resolved, &mut html);
    if html.is_empty() {
        return Ok(EMPTY_DOCUMENT_HTML.to_owned());
    }
    if html.len() > 10 * 1024 * 1024 {
        return Err("HTML content exceeds maximum size limit (10MB)".to_owned());
    }
    match crate::space::sanitize::sanitize_html(&html) {
        crate::space::sanitize::Sanitize::Clean(clean) => {
            if clean.is_empty() {
                Ok(EMPTY_DOCUMENT_HTML.to_owned())
            } else {
                Ok(clean)
            }
        }
        crate::space::sanitize::Sanitize::Invalid => Err("Failed to sanitize HTML".to_owned()),
    }
}

/// Lower pulldown events to [`MdEvent`], deciding task lists
/// (`_resolve_task_lists`, `markdown_converter.py:232-272`): a bullet list
/// becomes a `taskList` only when EVERY item carries a marker; elsewhere
/// the marker is restored as literal `[x]`/`[ ]` text. In task items the
/// marker is dropped and the following text's leading space lstrips (the
/// plugin strips `[ ]` but leaves the separator).
fn resolve_task_events(events: &[pulldown_cmark::Event]) -> Vec<MdEvent> {
    use pulldown_cmark::{Event, Tag, TagEnd};
    // First pass: per-list task decisions. A list (bullet only) is a task
    // list when it has at least one item and every item's first inline
    // payload is a `TaskListMarker`.
    #[derive(Debug)]
    struct ListFrame {
        ordered_start: Option<u64>,
        item_has_marker: Vec<bool>,
        item_indices: Vec<usize>,
    }
    let mut list_stack: Vec<ListFrame> = Vec::new();
    // `task_list[i]` / `task_item[i]` / `checked[i]` by event index.
    let mut list_decision: Vec<Option<bool>> = vec![None; events.len()];
    let mut item_task: Vec<Option<bool>> = vec![None; events.len()];
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::List(start)) => list_stack.push(ListFrame {
                ordered_start: *start,
                item_has_marker: Vec::new(),
                item_indices: Vec::new(),
            }),
            Event::End(TagEnd::List(_)) => {
                if let Some(frame) = list_stack.pop() {
                    let as_task = frame.ordered_start.is_none()
                        && !frame.item_indices.is_empty()
                        && frame.item_has_marker.iter().all(|has| *has);
                    // Find the matching start event to tag it (search back
                    // for the `Start(List)` at this depth).
                    let mut depth = 0;
                    for back in (0..index).rev() {
                        match &events[back] {
                            Event::End(TagEnd::List(_)) => depth += 1,
                            Event::Start(Tag::List(_)) => {
                                if depth == 0 {
                                    list_decision[back] = Some(as_task);
                                    break;
                                }
                                depth -= 1;
                            }
                            _ => {}
                        }
                    }
                    list_decision[index] = Some(as_task);
                    for (item_idx, has) in
                        frame.item_indices.iter().zip(frame.item_has_marker.iter())
                    {
                        if *has {
                            item_task[*item_idx] = Some(as_task);
                        }
                    }
                }
            }
            Event::Start(Tag::Item) => {
                if let Some(frame) = list_stack.last_mut() {
                    frame.item_indices.push(index);
                    // The marker, when present, is the item's first event.
                    let has = matches!(events.get(index + 1), Some(Event::TaskListMarker(_)));
                    frame.item_has_marker.push(has);
                }
            }
            _ => {}
        }
    }
    // Second pass: lower, consuming markers.
    let mut out: Vec<MdEvent> = Vec::with_capacity(events.len());
    let mut index = 0;
    // Item (task state, checked flag, open-event index, injected paragraph
    // open) by nesting stack.
    let mut item_stack: Vec<(bool, bool, usize, bool)> = Vec::new();
    // Whether the next text's leading space lstrips (post-marker).
    let mut lstrip_next_text = false;
    // Image alt-text capture: (dest, title, alt parts).
    let mut image_stack: Vec<(String, String, String)> = Vec::new();
    // Table-cell head flags by nesting stack.
    let mut cell_stack: Vec<bool> = Vec::new();
    // Real-paragraph and table-cell depth: injection only fires for bare
    // inline content directly inside a list item.
    let mut para_depth: u32 = 0;
    let mut cell_depth: u32 = 0;
    while index < events.len() {
        // Task-list markers are consumed here, never lowered directly.
        if let Event::TaskListMarker(checked) = events[index] {
            // The marker is the item's first event; its text (literal or
            // following) sits inside the injected paragraph.
            if para_depth == 0 && cell_depth == 0 {
                if let Some(frame) = item_stack.last_mut() {
                    if !frame.3 {
                        out.push(MdEvent::StartParagraph);
                        frame.3 = true;
                    }
                }
            }
            // Find the enclosing item's task state.
            let is_task = item_stack
                .last()
                .map(|(task, _, _, _)| *task)
                .unwrap_or(false);
            if !is_task {
                out.push(MdEvent::Text(if checked {
                    "[x]".to_owned()
                } else {
                    "[ ]".to_owned()
                }));
            } else if let Some(top) = item_stack.last_mut() {
                top.1 = checked;
            }
            lstrip_next_text = is_task;
            index += 1;
            continue;
        }
        // Tight-list paragraphs (`markdown_converter.py:456-460` renders
        // every paragraph token, `hidden` or not): pulldown omits
        // `Start/End(Paragraph)` inside tight items, so inject them
        // around bare inline content. Real paragraphs, table cells (the
        // renderer wraps those), nested-item opens and empty items never
        // inject.
        let is_item_end = matches!(&events[index], Event::End(TagEnd::Item));
        let is_block_start = matches!(
            &events[index],
            Event::Start(
                Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::BlockQuote(_)
                    | Tag::CodeBlock(_)
                    | Tag::HtmlBlock
                    | Tag::List(_)
                    | Tag::FootnoteDefinition(_)
                    | Tag::Table(_)
            ) | Event::Rule
                | Event::Html(_)
        );
        let is_inline_content = matches!(
            &events[index],
            Event::Text(_)
                | Event::Code(_)
                | Event::SoftBreak
                | Event::HardBreak
                | Event::InlineHtml(_)
                | Event::FootnoteReference(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
                | Event::Start(
                    Tag::Emphasis
                        | Tag::Strong
                        | Tag::Strikethrough
                        | Tag::Link { .. }
                        | Tag::Image { .. }
                )
                | Event::End(
                    TagEnd::Emphasis
                        | TagEnd::Strong
                        | TagEnd::Strikethrough
                        | TagEnd::Link
                        | TagEnd::Image
                )
        );
        if is_item_end {
            if let Some(frame) = item_stack.last_mut() {
                if frame.3 {
                    out.push(MdEvent::EndParagraph);
                    frame.3 = false;
                }
            }
        } else if is_block_start {
            if para_depth == 0 {
                if let Some(frame) = item_stack.last_mut() {
                    if frame.3 {
                        out.push(MdEvent::EndParagraph);
                        frame.3 = false;
                    }
                }
            }
        } else if is_inline_content && para_depth == 0 && cell_depth == 0 {
            if let Some(frame) = item_stack.last_mut() {
                if !frame.3 {
                    out.push(MdEvent::StartParagraph);
                    frame.3 = true;
                }
            }
        }
        match &events[index] {
            Event::Start(Tag::Paragraph) => {
                para_depth += 1;
                out.push(MdEvent::StartParagraph);
            }
            Event::End(TagEnd::Paragraph) => {
                para_depth = para_depth.saturating_sub(1);
                out.push(MdEvent::EndParagraph);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                out.push(MdEvent::StartHeading(*level as u32))
            }
            Event::End(TagEnd::Heading(_)) => out.push(MdEvent::EndHeading),
            Event::Start(Tag::BlockQuote(_)) => out.push(MdEvent::StartQuote),
            Event::End(TagEnd::BlockQuote(_)) => out.push(MdEvent::EndQuote),
            Event::Start(Tag::CodeBlock(kind)) => {
                use pulldown_cmark::CodeBlockKind;
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                // Join the code text (pulldown splits it across events).
                let mut body = String::new();
                let mut cursor = index + 1;
                while cursor < events.len() {
                    match &events[cursor] {
                        Event::Text(text) | Event::Code(text) => body.push_str(text),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                    cursor += 1;
                }
                // markdown-it keeps the newline before the closing fence;
                // the editor does not store one (`fence`, `:368-376`).
                if let Some(stripped) = body.strip_suffix('\n') {
                    body = stripped.to_owned();
                }
                let fenced = matches!(kind, CodeBlockKind::Fenced(_));
                out.push(MdEvent::CodeBlock(
                    if fenced && !language.is_empty() {
                        Some(language)
                    } else if fenced {
                        Some(String::new())
                    } else {
                        None
                    },
                    body,
                ));
                index = cursor;
            }
            Event::End(TagEnd::CodeBlock) => {}
            Event::Start(Tag::List(start)) => {
                let as_task = list_decision[index].unwrap_or(false);
                out.push(MdEvent::StartList(*start, as_task));
            }
            Event::End(TagEnd::List(_)) => {
                let as_task = list_decision[index].unwrap_or(false);
                out.push(MdEvent::EndList(as_task));
            }
            Event::Start(Tag::Item) => {
                let as_task = item_task[index].unwrap_or(false);
                let open = out.len();
                item_stack.push((as_task, false, open, false));
                out.push(MdEvent::StartItem(as_task, false));
            }
            Event::End(TagEnd::Item) => {
                let (as_task, checked, open, _) =
                    item_stack
                        .pop()
                        .unwrap_or((false, false, usize::MAX, false));
                // Patch the checked flag onto this item's open event.
                if let Some(MdEvent::StartItem(_, slot)) = out.get_mut(open) {
                    *slot = checked;
                }
                out.push(MdEvent::EndItem(as_task));
            }
            Event::Start(Tag::Table(_)) => out.push(MdEvent::StartTable),
            Event::End(TagEnd::Table) => out.push(MdEvent::EndTable),
            Event::Start(Tag::TableHead) => out.push(MdEvent::StartHead),
            Event::End(TagEnd::TableHead) => out.push(MdEvent::EndHead),
            Event::Start(Tag::TableRow) => out.push(MdEvent::StartRow),
            Event::End(TagEnd::TableRow) => out.push(MdEvent::EndRow),
            Event::Start(Tag::TableCell) => {
                cell_depth += 1;
                // Header vs body: inside `TableHead` until its end.
                let mut in_head = false;
                let mut depth = 0;
                for back in (0..index).rev() {
                    match &events[back] {
                        Event::Start(Tag::TableHead) if depth == 0 => {
                            in_head = true;
                            break;
                        }
                        Event::End(TagEnd::TableHead) => depth += 1,
                        Event::Start(Tag::TableHead) => {
                            depth -= 1;
                        }
                        Event::Start(Tag::Table(_)) => break,
                        _ => {}
                    }
                }
                cell_stack.push(in_head);
                out.push(MdEvent::StartCell(in_head));
            }
            Event::End(TagEnd::TableCell) => {
                // The renderer pairs opens and closes structurally; carry
                // the flag from the matching open via the cell stack.
                cell_depth = cell_depth.saturating_sub(1);
                let head = cell_stack.pop().unwrap_or(false);
                out.push(MdEvent::EndCell(head));
            }
            Event::Start(Tag::Emphasis) => out.push(MdEvent::StartEm),
            Event::End(TagEnd::Emphasis) => out.push(MdEvent::EndEm),
            Event::Start(Tag::Strong) => out.push(MdEvent::StartStrong),
            Event::End(TagEnd::Strong) => out.push(MdEvent::EndStrong),
            Event::Start(Tag::Strikethrough) => out.push(MdEvent::StartStrike),
            Event::End(TagEnd::Strikethrough) => out.push(MdEvent::EndStrike),
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => {
                out.push(MdEvent::StartLink(dest_url.to_string(), title.to_string()));
            }
            Event::End(TagEnd::Link) => out.push(MdEvent::EndLink),
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                image_stack.push((dest_url.to_string(), title.to_string(), String::new()));
            }
            Event::End(TagEnd::Image) => {
                let (dest, title, alt) = image_stack.pop().unwrap_or_default();
                if image_stack.is_empty() {
                    out.push(MdEvent::Image(dest, title, alt));
                } else if let Some(parent) = image_stack.last_mut() {
                    parent.2.push_str(&alt);
                }
            }
            Event::Text(text) => {
                let mut content = text.to_string();
                if lstrip_next_text {
                    let stripped = content.trim_start_matches([' ', '\t']).to_owned();
                    content = stripped;
                    lstrip_next_text = false;
                }
                if let Some(image) = image_stack.last_mut() {
                    image.2.push_str(&content);
                } else {
                    out.push(MdEvent::Text(content));
                }
            }
            Event::Code(content) => {
                if let Some(image) = image_stack.last_mut() {
                    image.2.push_str(content);
                } else {
                    out.push(MdEvent::Code(content.to_string()));
                }
            }
            Event::Html(content) => out.push(MdEvent::HtmlBlock(content.to_string())),
            Event::InlineHtml(content) => {
                if image_stack.is_empty() {
                    out.push(MdEvent::HtmlInline(content.to_string()));
                }
            }
            Event::SoftBreak => {
                if let Some(image) = image_stack.last_mut() {
                    image.2.push(' ');
                } else {
                    out.push(MdEvent::SoftBreak);
                }
            }
            Event::HardBreak => out.push(MdEvent::HardBreak),
            Event::Rule => out.push(MdEvent::Rule),
            Event::FootnoteReference(_) => {}
            Event::TaskListMarker(_) => {}
            _ => {}
        }
        index += 1;
    }
    out
}

/// Render resolved events as the HTML Tiptap's editor emits
/// (`TiptapHTMLRenderer`, `markdown_converter.py:275-431`): compact (no
/// inter-block newlines), only the attributes the editor's parse rules
/// read. Tight-list paragraphs render too (the `hidden = False` pass,
/// `:456-460` — pulldown emits them regardless).
fn render_tiptap(events: &[MdEvent], html: &mut String) {
    let mut index = 0;
    while index < events.len() {
        match &events[index] {
            MdEvent::StartHeading(level) => html.push_str(&format!("<h{level}>")),
            MdEvent::EndHeading => {
                // The level is unknown at the close; re-scan for the open.
                let mut level = 1;
                for back in events[..index].iter().rev() {
                    match back {
                        MdEvent::StartHeading(open) => {
                            level = *open;
                            break;
                        }
                        MdEvent::EndHeading => break,
                        _ => {}
                    }
                }
                html.push_str(&format!("</h{level}>"));
            }
            MdEvent::StartParagraph => {
                // A lone safe image hoists out of the paragraph (Tiptap
                // images are blocks): `_lone_image`, `:313-322`.
                if !is_lone_image(events, index) {
                    html.push_str("<p>");
                }
            }
            MdEvent::EndParagraph => {
                if !is_lone_image_close(events, index) {
                    html.push_str("</p>");
                }
            }
            MdEvent::StartQuote => html.push_str("<blockquote>"),
            MdEvent::EndQuote => html.push_str("</blockquote>"),
            MdEvent::CodeBlock(language, body) => {
                let body = escape_html(body);
                match language {
                    Some(language) if !language.is_empty() => html.push_str(&format!(
                        "<pre><code class=\"language-{}\">{body}</code></pre>",
                        escape_html(language)
                    )),
                    _ => html.push_str(&format!("<pre><code>{body}</code></pre>")),
                }
            }
            MdEvent::StartList(start, as_task) => {
                // `bullet_list_open` / `ordered_list_open`, `:341-350`:
                // task lists are `<ul data-type="taskList">`; ordered
                // lists carry `start` unless it is 1.
                match start {
                    None if *as_task => html.push_str("<ul data-type=\"taskList\">"),
                    None => html.push_str("<ul>"),
                    Some(1) => html.push_str("<ol>"),
                    Some(number) => html.push_str(&format!("<ol start=\"{number}\">")),
                }
            }
            MdEvent::EndList(as_task) => {
                // Ordered lists close `</ol>`; bullet/task lists `</ul>`.
                // Re-scan for the open kind.
                let mut ordered = false;
                let mut depth = 0;
                for back in events[..index].iter().rev() {
                    match back {
                        MdEvent::EndList(_) => depth += 1,
                        MdEvent::StartList(start, _) => {
                            if depth == 0 {
                                ordered = start.is_some();
                                break;
                            }
                            depth -= 1;
                        }
                        _ => {}
                    }
                }
                let _ = as_task;
                html.push_str(if ordered { "</ol>" } else { "</ul>" });
            }
            MdEvent::StartItem(as_task, checked) => {
                if !as_task {
                    html.push_str("<li>");
                } else {
                    let checkbox = if *checked {
                        "<input type=\"checkbox\" checked=\"checked\">"
                    } else {
                        "<input type=\"checkbox\">"
                    };
                    html.push_str(&format!(
                        "<li data-type=\"taskItem\" data-checked=\"{}\"><label>{checkbox}<span></span></label><div>",
                        if *checked { "true" } else { "false" }
                    ));
                }
            }
            MdEvent::EndItem(as_task) => {
                html.push_str(if *as_task { "</div></li>" } else { "</li>" });
            }
            MdEvent::StartTable => html.push_str("<table><tbody>"),
            MdEvent::EndTable => html.push_str("</tbody></table>"),
            MdEvent::StartHead | MdEvent::EndHead => {}
            MdEvent::StartRow => html.push_str("<tr>"),
            MdEvent::EndRow => html.push_str("</tr>"),
            MdEvent::StartCell(head) => {
                html.push_str(if *head { "<th><p>" } else { "<td><p>" });
            }
            MdEvent::EndCell(head) => {
                html.push_str(if *head { "</p></th>" } else { "</p></td>" });
            }
            MdEvent::StartEm => html.push_str("<em>"),
            MdEvent::EndEm => html.push_str("</em>"),
            MdEvent::StartStrong => html.push_str("<strong>"),
            MdEvent::EndStrong => html.push_str("</strong>"),
            MdEvent::StartStrike => html.push_str("<s>"),
            MdEvent::EndStrike => html.push_str("</s>"),
            MdEvent::StartLink(dest, title) => {
                // `link_open`, `:408-415`: unsafe hrefs drop the attribute
                // (the link text stays); titles pass through when set.
                let mut attrs = String::new();
                if is_safe_url(dest) {
                    attrs.push_str(&format!(" href=\"{}\"", escape_html(dest)));
                }
                if !title.is_empty() {
                    attrs.push_str(&format!(" title=\"{}\"", escape_html(title)));
                }
                html.push_str(&format!("<a{attrs}>"));
            }
            MdEvent::EndLink => html.push_str("</a>"),
            MdEvent::Image(dest, title, alt) => {
                // `image`, `:417-431`: unsafe or empty sources keep the
                // alt text (the sanitizer would strip the src and the
                // editor would drop the node).
                if !is_safe_url(dest) || dest.is_empty() {
                    html.push_str(&escape_html(alt));
                } else {
                    let mut attrs = format!(" src=\"{}\"", escape_html(dest));
                    if !alt.is_empty() {
                        attrs.push_str(&format!(" alt=\"{}\"", escape_html(alt)));
                    }
                    if !title.is_empty() {
                        attrs.push_str(&format!(" title=\"{}\"", escape_html(title)));
                    }
                    html.push_str(&format!("<img{attrs}>"));
                }
            }
            MdEvent::Text(text) => html.push_str(&escape_html(text)),
            MdEvent::Code(code) => html.push_str(&format!("<code>{}</code>", escape_html(code))),
            // Raw HTML is disabled at parse time; escape defensively
            // (`html_inline` / `html_block`, `:305-309`).
            MdEvent::HtmlInline(content) => html.push_str(&escape_html(content)),
            MdEvent::HtmlBlock(content) => {
                html.push_str(&format!("<p>{}</p>", escape_html(content.trim())));
            }
            // ProseMirror collapses a paragraph newline to a space anyway.
            MdEvent::SoftBreak => html.push(' '),
            MdEvent::HardBreak => html.push_str("<br>"),
            // `CustomHorizontalRule` renders a wrapper div (`:336-337`).
            MdEvent::Rule => html.push_str("<div data-type=\"horizontalRule\"><div></div></div>"),
        }
        index += 1;
    }
}

/// Whether the paragraph at `open` holds a lone safe image
/// (`TiptapHTMLRenderer._lone_image`, `markdown_converter.py:313-332`):
/// the paragraph is top-level (depth 0) and its single child is an image
/// with a safe non-empty source. Depth here counts blockquote/list/table
/// nesting only (inline nesting never contains a paragraph open).
fn is_lone_image(events: &[MdEvent], open: usize) -> bool {
    if paragraph_depth(events, open) != 0 {
        return false;
    }
    // The paragraph's content is exactly one image event.
    if !matches!(events.get(open + 1), Some(MdEvent::Image(_, _, _))) {
        return false;
    }
    if !matches!(events.get(open + 2), Some(MdEvent::EndParagraph)) {
        return false;
    }
    if let Some(MdEvent::Image(dest, _, _)) = events.get(open + 1) {
        return !dest.is_empty() && is_safe_url(dest);
    }
    false
}

/// The close-side lone-image check (`paragraph_close`, `:329-332`).
fn is_lone_image_close(events: &[MdEvent], close: usize) -> bool {
    if close < 2 {
        return false;
    }
    if !matches!(events.get(close - 1), Some(MdEvent::Image(_, _, _))) {
        return false;
    }
    if let Some(MdEvent::StartParagraph) = events.get(close - 2) {
        return is_lone_image(events, close - 2);
    }
    false
}

/// Block-nesting depth of the paragraph opening at `open` (blockquotes,
/// list items and table cells; the top level is 0).
fn paragraph_depth(events: &[MdEvent], open: usize) -> usize {
    let mut depth: usize = 0;
    for event in &events[..open] {
        match event {
            MdEvent::StartQuote | MdEvent::StartItem(_, _) | MdEvent::StartCell(_) => depth += 1,
            MdEvent::EndQuote | MdEvent::EndItem(_) | MdEvent::EndCell(_) => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    depth
}

#[cfg(test)]
mod difftest {
    use super::*;

    /// Tight-list paragraphs (`markdown_converter.py:456-460` renders
    /// every paragraph token): pulldown omits them, the lowering injects
    /// them — pinned live against `markdown_to_html` (Django 4.2 venv).
    #[test]
    fn markdown_tight_list_paragraphs() {
        for (input, expected) in [
            ("- a\n- b", "<ul><li><p>a</p></li><li><p>b</p></li></ul>"),
            ("- a\n\n- b", "<ul><li><p>a</p></li><li><p>b</p></li></ul>"),
            ("- ", "<ul><li></li></ul>"),
            (
                "- a\n  - b",
                "<ul><li><p>a</p><ul><li><p>b</p></li></ul></li></ul>",
            ),
            (
                "- [x] done\n- [ ] todo",
                "<ul data-type=\"taskList\"><li data-type=\"taskItem\" data-checked=\"true\"><label><input type=\"checkbox\" checked=\"checked\"><span></span></label><div><p>done</p></div></li><li data-type=\"taskItem\" data-checked=\"false\"><label><input type=\"checkbox\"><span></span></label><div><p>todo</p></div></li></ul>",
            ),
        ] {
            assert_eq!(markdown_to_html_port(input).as_deref(), Ok(expected), "{input:?}");
        }
    }

    #[test]
    fn differential_markdown_corpus_scratch() {
        // Scratch corpus (regenerated by the markdown probe scripts, never
        // committed): skip loudly when absent so CI stays green.
        let Ok(raw) = std::fs::read_to_string("/tmp/md-corpus.json") else {
            eprintln!("skipping differential_markdown_corpus_scratch: /tmp/md-corpus.json absent");
            return;
        };
        let vectors: Vec<Map<String, Value>> = serde_json::from_str(&raw).expect("json");
        let mut failures = 0;
        for (index, vector) in vectors.iter().enumerate() {
            let input = vector.get("input").and_then(Value::as_str).unwrap_or("");
            let expected = vector
                .get("output")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let expected_error = vector
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let actual = markdown_to_html_port(input);
            let ok = match (&actual, &expected, &expected_error) {
                (Ok(html), Some(want), _) => html == want,
                (Err(message), _, Some(want)) => message == want,
                _ => false,
            };
            if !ok {
                failures += 1;
                if failures <= 60 {
                    println!("--- vector {index} input={input:?}");
                    println!("    want: {expected:?} / err {expected_error:?}");
                    println!("    got:  {actual:?}");
                }
            }
        }
        println!("{}/{} passed", vectors.len() - failures, vectors.len());
        assert_eq!(failures, 0, "differential failures");
    }

    #[test]
    fn scratch_offsets() {
        use pulldown_cmark::{Options, Parser};
        for input in [
            "[a *b*](javascript:x)",
            "![a](javascript:x)",
            "[a][b]\n\n[b]: /url",
            "[a]",
            "- [ ]  two",
            "<http://example.com/a b>",
        ] {
            println!("=== {input:?}");
            let options =
                Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
            for (event, range) in Parser::new_ext(input, options).into_offset_iter() {
                println!("    {range:?} {event:?} slice={:?}", &input[range.clone()]);
            }
        }
    }
}

/// The non-dict write body (`serializers.py` `to_internal_value`): DRF
/// answers `{"non_field_errors": ["Invalid data. Expected a dictionary,
/// but got <type>."]}` with the CPython type name.
fn non_dict_body(value: &Value) -> String {
    format!(
        "{{\"non_field_errors\":[{}]}}",
        json_string(&format!(
            "Invalid data. Expected a dictionary, but got {}.",
            json_type_name(value)
        ))
    )
}

/// The create-time default type
/// (`serializers/issue.py:297-300`): `IssueType.objects.filter(
/// project_issue_types__project_id=..., is_default=True).first()` —
/// unordered `.first()` (no `Meta.ordering` on `IssueType`), both tables
/// under the soft-deletion scope.
async fn fetch_default_issue_type(pool: &PgPool, project_id: &Uuid) -> Result<Option<Uuid>, Error> {
    sqlx::query_scalar(
        r#"SELECT "it"."id" FROM "issue_types" AS "it" WHERE "it"."deleted_at" IS NULL AND "it"."is_default" AND EXISTS(SELECT 1 FROM "project_issue_types" AS "pit" WHERE "pit"."issue_type_id" = "it"."id" AND "pit"."project_id" = $1 AND "pit"."deleted_at" IS NULL) LIMIT 1"#,
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-default-type"))
    .map(|row: Option<Uuid>| row)
}

/// `Issue.has_active_run` (`db/models/issue.py:232-248`):
/// `AgentRun.objects.filter(work_item=self,
/// status__in=NON_TERMINAL_STATUSES).exists()` — the plain manager, no
/// liveness scope. The set is `runner/services/matcher.py:54-66` (the
/// retired `waiting_for_worktree` still gates, PDASHOSS01-137).
const NON_TERMINAL_STATUSES: &[&str] = &[
    "queued",
    "assigned",
    "waiting_for_worktree",
    "running",
    "cancel_requested",
    "awaiting_approval",
    "awaiting_reauth",
    "paused_awaiting_input",
];

async fn issue_has_active_run(pool: &PgPool, issue_id: &Uuid) -> Result<bool, Error> {
    sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "agent_run" WHERE "work_item_id" = $1 AND "status" = ANY($2))"#,
    )
    .bind(issue_id)
    .bind(NON_TERMINAL_STATUSES)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-active-run"))
    .map(|found: Option<bool>| found.unwrap_or(false))
}

/// The pod's project for the `_same_uuid` check
/// (`serializers/issue.py:199-203`): both sides are typed UUIDs here, so
/// the canonical-compare fallback arms are unreachable — plain equality.
async fn fetch_pod_project_id(pool: &PgPool, pod_id: &Uuid) -> Result<Option<Uuid>, Error> {
    sqlx::query_scalar(r#"SELECT "project_id" FROM "pod" WHERE "id" = $1"#)
        .bind(pod_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-pod-project"))
        .map(|row: Option<Uuid>| row)
}

/// The `ProjectMember` filter backing the assignee allow-list
/// (`serializers/issue.py:246-252`): active members with `role >= 15` in
/// `ProjectMember.Meta.ordering = ("-created_at",)`.
async fn filter_assignee_ids(
    pool: &PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Error> {
    sqlx::query_scalar(
        r#"SELECT "member_id" FROM "project_members" WHERE "project_id" = $1 AND "is_active" AND "role" >= 15 AND "member_id" = ANY($2) AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(project_id)
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "write-assignee-filter"))
}

/// The `Label` filter backing the label allow-list
/// (`serializers/issue.py:255-259`), in `Label.Meta.ordering =
/// ("-created_at",)`.
async fn filter_label_ids(
    pool: &PgPool,
    project_id: &Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, Error> {
    sqlx::query_scalar(
        r#"SELECT "id" FROM "labels" WHERE "project_id" = $1 AND "id" = ANY($2) AND "deleted_at" IS NULL ORDER BY "created_at" DESC"#,
    )
    .bind(project_id)
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(|error| db_error(error, "write-label-filter"))
}

/// Port of `IssueSerializer.validate()` (`serializers/issue.py:182-288`),
/// in source order — first failure wins, every arm a 400. `parsed` is the
/// field-level value (mutated in place exactly like `data`: the lxml and
/// sanitizer substitutes, the assignee/label allow-lists). `instance` is
/// the patch/delete current row (`None` on create, skipping the reassign
/// gate). The `description_binary` branch is dead: DRF maps `BinaryField`
/// to a read-only `ModelField` (probed live), so the key never reaches
/// `validated_data`.
async fn run_write_validate(
    pool: &PgPool,
    parsed: &mut ParsedWrite,
    project_id: &Uuid,
    workspace_id: &Uuid,
    instance: Option<&DecodedIssue>,
    from_markdown: bool,
) -> Result<(), Error> {
    use shape::ValidateError;
    let fail = |error: ValidateError| Error::FieldErrors(error.body().to_owned());

    // Start / target (`:183-188`).
    if let (Some(Some(start)), Some(Some(target))) = (parsed.start_date, parsed.target_date) {
        if start > target {
            return Err(fail(ValidateError::StartExceedsTarget));
        }
    }

    // Assigned pod (`:193-216`).
    if let Some(pod) = parsed.assigned_pod {
        if let Some(pod_id) = pod {
            let pod_project = fetch_pod_project_id(pool, &pod_id).await?;
            if pod_project.as_ref() != Some(project_id) {
                return Err(fail(ValidateError::PodWrongProject));
            }
        }
        // The reassign gate only exists on update (`self.instance`), and
        // `has_active_run` only queries on a real change — assign,
        // reassign, or clear, compared as `str()` (`None` spells `None`).
        if let Some(current) = instance {
            let new_text = pod.map(|id| id.to_string());
            if new_text != current.assigned_pod
                && issue_has_active_run(
                    pool,
                    &current.id.parse::<Uuid>().map_err(|_| Error::ServerError)?,
                )
                .await?
            {
                return Err(fail(ValidateError::PodReassignActiveRun));
            }
        }
    }

    // lxml round-trip (`:222-229`): every non-`None` value round-trips
    // (empty strings raise `ParserError` like any other failure) unless
    // the HTML came from markdown.
    if let Some(html) = parsed.description_html.take() {
        let mut html = html;
        if !from_markdown {
            match lxml_roundtrip(&html) {
                Some(roundtripped) => html = roundtripped,
                None => return Err(fail(ValidateError::InvalidHtml)),
            }
            // Sanitizer substitute (`:232-238`): only truthy HTML
            // validates, and the clean output always substitutes.
            if !html.is_empty() {
                match crate::space::sanitize::sanitize_html(&html) {
                    crate::space::sanitize::Sanitize::Clean(clean) => html = clean,
                    crate::space::sanitize::Sanitize::Invalid => {
                        return Err(fail(ValidateError::HtmlContentInvalid));
                    }
                }
            }
        }
        parsed.description_html = Some(html);
    }

    // Assignee / label allow-lists (`:246-259`): falsy inputs (absent or
    // `[]`) skip the filter and keep their value.
    if let Some(ids) = parsed.assignees.take() {
        let filtered = if ids.is_empty() {
            ids
        } else {
            filter_assignee_ids(pool, project_id, &ids).await?
        };
        parsed.assignees = Some(filtered);
    }
    if let Some(ids) = parsed.labels.take() {
        let filtered = if ids.is_empty() {
            ids
        } else {
            filter_label_ids(pool, project_id, &ids).await?
        };
        parsed.labels = Some(filtered);
    }

    // State / parent / estimate project checks (`:261-286`), each under
    // its field's manager scope (`StateManager` excludes triage;
    // `Issue.objects` keeps drafts, archived and triage rows).
    if let Some(Some(state_id)) = parsed.state {
        let ok: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "states" WHERE "project_id" = $1 AND "id" = $2 AND "deleted_at" IS NULL AND NOT ("group" = 'triage'))"#,
        )
        .bind(project_id)
        .bind(state_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-state-check"))?;
        if !ok.unwrap_or(false) {
            return Err(fail(ValidateError::StateWrongProject));
        }
    }
    if let Some(Some(parent_id)) = parsed.parent {
        let ok: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "issues" WHERE "workspace_id" = $1 AND "project_id" = $2 AND "id" = $3 AND "deleted_at" IS NULL)"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(parent_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-parent-check"))?;
        if !ok.unwrap_or(false) {
            return Err(fail(ValidateError::ParentWrongProject));
        }
    }
    if let Some(Some(estimate_id)) = parsed.estimate_point {
        let ok: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM "estimate_points" WHERE "workspace_id" = $1 AND "project_id" = $2 AND "id" = $3 AND "deleted_at" IS NULL)"#,
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(estimate_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| db_error(error, "write-estimate-check"))?;
        if !ok.unwrap_or(false) {
            return Err(fail(ValidateError::EstimatePointWrongProject));
        }
    }

    Ok(())
}

/// Coerce the raw `created_at` override
/// (`views/issue.py:511-514` — `DateTimeField.get_prep_value`, probed
/// live): strings parse as datetimes (date-only falls back to midnight),
/// naive takes the request zone, aware keeps its instant; `null` stores
/// `NULL` (the not-null column then 400s); every other JSON type raises
/// `TypeError` into the 500.
fn coerce_created_at_override(
    value: &Value,
    tz: &Tz,
    tz_name: &str,
) -> Result<Option<DateTime<Utc>>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    let Value::String(text) = value else {
        return Err(Error::ServerError);
    };
    match parse_drf_datetime(value, tz, tz_name) {
        Ok(instant) => Ok(Some(instant)),
        Err(_) => match parse_django_date(text) {
            Some(date) => {
                let naive = date.and_hms_opt(0, 0, 0).expect("midnight exists");
                match tz.from_local_datetime(&naive) {
                    chrono::LocalResult::Single(aware) => Ok(Some(aware.with_timezone(&Utc))),
                    chrono::LocalResult::Ambiguous(first, _) => Ok(Some(first.with_timezone(&Utc))),
                    chrono::LocalResult::None => Err(Error::ServerError),
                }
            }
            None => Err(Error::BadError("Please provide valid detail".to_owned())),
        },
    }
}

/// Coerce the raw `created_by` override (`UUIDField.to_python`, probed
/// live): strings parse flexibly (braces, `urn:`, unhyphenated),
/// non-negative ints (bools spell 0/1) take the `int=` form, `null`
/// stores `NULL` (the column is nullable); everything else is the
/// `ValidationError` 400.
fn coerce_created_by_override(value: &Value) -> Result<Option<Uuid>, Error> {
    let invalid = || Error::BadError("Please provide valid detail".to_owned());
    match value {
        Value::Null => Ok(None),
        Value::String(text) => parse_uuid_hex(text).ok_or_else(invalid).map(Some),
        Value::Bool(flag) => Ok(Some(Uuid::from_u128(u128::from(*flag as u8)))),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(invalid());
                }
                return Ok(Some(Uuid::from_u128(int as u128)));
            }
            if let Some(uint) = number.as_u64() {
                return Ok(Some(Uuid::from_u128(u128::from(uint))));
            }
            if number.is_f64() {
                return Err(invalid());
            }
            number
                .to_string()
                .parse::<u128>()
                .map(Uuid::from_u128)
                .map(Some)
                .map_err(|_| invalid())
        }
        Value::Array(_) | Value::Object(_) => Err(invalid()),
    }
}

/// Map an override-coercion failure to its chain stop: the constant 400
/// detail arm, else the 500 arm (the only two the coercions produce).
fn override_stop(denial: Error) -> domain_issues::ChainStop {
    match denial {
        Error::BadError(_) => domain_issues::ChainStop::OverrideDetail,
        _ => domain_issues::ChainStop::OverrideFailed,
    }
}

/// Python `str()` for the external-dupe guards' raw scalars (`CharField`
/// stringifies filter values: numbers spell plainly, bools `True`/`False`).
/// Composites are unreachable (the serializer 400s them before the guard).
fn raw_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(python_number_str(number)),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        _ => None,
    }
}

/// Python truthiness for the guards' `request.data.get(...)` tests.
fn raw_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                return int != 0;
            }
            if let Some(uint) = number.as_u64() {
                return uint != 0;
            }
            number.as_f64().is_some_and(|float| float != 0.0)
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// The create/patch external-duplicate guard's `exists()` + `.first()`
/// pair (`views/issue.py:484-503,823-841`): same filter (project, slug,
/// source, id under the soft-deletion scope), the first in
/// `Meta.ordering = ("-created_at",)`.
async fn external_dup_first(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
    source: &str,
    external_id: &str,
) -> Result<Option<Uuid>, Error> {
    let exists: Option<bool> = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "issues" INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") WHERE "issues"."project_id" = $1 AND "workspaces"."slug" = $2 AND "issues"."external_source" = $3 AND "issues"."external_id" = $4 AND "issues"."deleted_at" IS NULL)"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-dup-exists"))?;
    if !exists.unwrap_or(false) {
        return Ok(None);
    }
    sqlx::query_scalar(
        r#"SELECT "issues"."id" FROM "issues" INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") WHERE "issues"."project_id" = $1 AND "workspaces"."slug" = $2 AND "issues"."external_source" = $3 AND "issues"."external_id" = $4 AND "issues"."deleted_at" IS NULL ORDER BY "issues"."created_at" DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-dup-first"))
    .map(|row: Option<Uuid>| row)
}

/// The 409 body (`views/issue.py:502-508,836-841`): insertion order
/// `error`, `id`.
fn external_dup_body(dup_id: &Uuid) -> String {
    format!(
        "{{\"error\":{},\"id\":{}}}",
        json_string(EXTERNAL_DUP_MESSAGE),
        json_string(&dup_id.to_string())
    )
}

/// The tenant scope and negotiated payload for an issue write: what
/// the caller resolved before calling the operation (auth, gate, route
/// params, body negotiation, settings).
#[derive(Debug, Clone)]
pub struct IssueWrite<'a> {
    /// The workspace slug from the route.
    pub slug: &'a str,
    /// The gated project id (after identifier rewrite).
    pub project_id: Uuid,
    /// The gated workspace id.
    pub workspace_id: Uuid,
    /// The fetched project row. The caller fetches it before parsing the
    /// body, so a missing project 404s before a malformed body 400s
    /// (`Project.objects.get` precedes normalization, `:471-474`); the
    /// create stores its workspace id and default assignee on the row.
    pub project: WriteProject,
    /// The negotiated payload with its `from_form` flag.
    pub body: &'a Value,
    /// Whether the payload arrived as an HTML form.
    pub from_form: bool,
    /// The settings-derived app origin (`base_host`), if configured.
    pub origin: Option<String>,
    /// The settings-derived web base for rendered URLs, if configured.
    pub web_base: Option<String>,
}

/// Per-caller options for [`create`] (design D6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateOptions {
    /// Emit the `issue_activity` created leg. The v1 view passes `true`;
    /// callers that enqueue their own shaped activity (the MCP tools)
    /// pass `false`.
    pub emit_activity: bool,
    /// Emit the webhook (`model_activity`) leg. The v1 view passes
    /// `true`; origin-less callers that enqueue their own shaped legs
    /// (the MCP tools) pass `false`, which skips the leg entirely — no
    /// origin is required and no [`ChainStop::Origin`] is recorded.
    ///
    /// [`ChainStop::Origin`]: domain_issues::ChainStop::Origin
    pub emit_model_activity: bool,
}

/// Per-caller options for [`update`] (design D6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateOptions {
    /// Emit the `issue_activity` update leg (see [`CreateOptions`]).
    pub emit_activity: bool,
}

/// The pre-resolved update target: the issue row, its rendered pre-image,
/// and the run attribution. The caller resolves all three before parsing
/// the body: the row fetches 404, the run header 400s, and the pre-image
/// render 500s, each before a malformed body 400s (`views/issue.py:792-811`).
#[derive(Debug, Clone)]
pub struct UpdateTarget<'a> {
    /// The pre-fetched issue row (the handler 404s before parsing).
    pub current: &'a DecodedIssue,
    /// The `current_instance` pre-image (`:808-811`), rendered with the
    /// same settings-derived web base as the response.
    pub current_instance: String,
    /// The resolved `X-Pi-Dash-Run-Id` (`:794-807`).
    pub moved_by_run: Option<Uuid>,
}

/// Map a domain-write failure onto the save arms: integrity violations
/// and dangling-row misses (a state validated by probe, gone at save —
/// the port's `UPDATE` FK decided a 400 there too) are the
/// invalid-payload 400, anything else 500s. (The handlers'
/// `domain_save_denial`, returning [`Error`].)
fn domain_save_error(error: domain::DomainError) -> Error {
    if error.is_integrity_violation() || error.is_not_found() {
        Error::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned())
    } else {
        Error::ServerError
    }
}

/// Fetch one row for the write paths via the public read service.
async fn fetch_issue_for_write(
    pool: &PgPool,
    slug: &str,
    project_id: &Uuid,
    pk: &Uuid,
    tz: &Tz,
) -> Result<DecodedIssue, Error> {
    read::fetch_issue_for_write(pool, slug, project_id, pk, tz)
        .await
        .map_err(|error| match error {
            read::Error::NotFound => Error::NotFound,
            read::Error::Failed(_) => Error::ServerError,
            read::Error::Db(err) => {
                let (site, source) = err.split();
                db_error(source, site)
            }
        })
}

/// Render a write response / `current_instance` image via the public
/// read service.
async fn render_write_response(
    pool: &PgPool,
    decoded: &DecodedIssue,
    tz: &Tz,
    web_base: Option<&str>,
) -> Result<Value, Error> {
    read::render_write_response(pool, decoded, tz, web_base)
        .await
        .map_err(|error| match error {
            read::Error::NotFound | read::Error::Failed(_) => Error::ServerError,
            read::Error::Db(err) => {
                let (site, source) = err.split();
                db_error(source, site)
            }
        })
}

/// The create's `model_activity` spec, unless the caller suppresses the
/// leg (design D6): origin-less callers (the MCP tools) pass
/// `emit_model_activity: false` and enqueue their own shaped legs, so no
/// spec is built, no origin is required, and the chain records no
/// [`ChainStop::Origin`](domain_issues::ChainStop::Origin).
fn create_model_activity(
    model: Uuid,
    requested: Value,
    actor: Uuid,
    slug: String,
    origin: Option<String>,
    emit_model_activity: bool,
) -> Option<domain_issues::ModelActivitySpec> {
    if !emit_model_activity {
        return None;
    }
    Some(domain_issues::ModelActivitySpec {
        model,
        requested,
        current: Value::Null,
        actor,
        slug,
        origin,
    })
}

/// `POST .../issues/` (`IssueListCreateAPIEndpoint.post`,
/// `views/issue.py:470-540`): `IssueSerializer(data,
/// context={project_id, workspace_id, default_assignee_id}).save()` plus
/// the render.
///
/// The caller (the v1 handler or an overlay tool) already ran auth, the
/// gate and body negotiation; `write` carries what it resolved.
/// `options` carries the per-caller deltas (design D6).
pub async fn create(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &IssueWrite<'_>,
    options: CreateOptions,
) -> Result<Outcome, Error> {
    let slug = write.slug;
    let project_id = write.project_id;
    let workspace_id = write.workspace_id;
    let body = write.body;
    let from_form = write.from_form;
    let tz = activate_timezone(ctx.actor.timezone.as_deref())?;
    let tz_name = ctx.actor.timezone.as_deref().unwrap_or("UTC");
    // The project row arrives pre-fetched (see [`IssueWrite::project`]).
    let project = write.project;
    let norm_input = if from_form {
        form_dict_for_normalize(body)
    } else {
        body.clone()
    };
    let (data, from_markdown) = normalize_view_body(&norm_input, &markdown_to_html_port)?;
    let Value::Object(map) = &data else {
        return Err(Error::FieldErrors(non_dict_body(&data)));
    };
    let mut parsed = parse_issue_write(&deps.pool, map, false, from_form, &tz, tz_name).await?;
    // The create-only model default that reaches `validate()`: a missing
    // `description_html` validates (and stores) as `"<p></p>"`.
    if parsed.description_html.is_none() {
        parsed.description_html = Some("<p></p>".to_owned());
    }
    run_write_validate(
        &deps.pool,
        &mut parsed,
        &project_id,
        &workspace_id,
        None,
        from_markdown,
    )
    .await?;

    // External-duplicate guard (`:483-503`): raw truthy values, the
    // conflicting row's id in the 409.
    if let (Some(raw_id), Some(raw_source)) = (body.get("external_id"), body.get("external_source"))
    {
        if raw_truthy(raw_id) && raw_truthy(raw_source) {
            if let (Some(external_id), Some(source)) = (raw_text(raw_id), raw_text(raw_source)) {
                if let Some(dup_id) =
                    external_dup_first(&deps.pool, &project_id, slug, &source, &external_id).await?
                {
                    return Err(Error::Conflict(external_dup_body(&dup_id)));
                }
            }
        }
    }

    // `serializer.save()` → `create()` (`:291-366`) + `Issue.save`
    // (`db/models/issue.py:267-351`): the assembled row rides
    // `domain::issues::create` — the save hooks resolve the pod/state
    // defaults, `completed_at`, stripped text, sequence and sort order.
    // Only the serializer-level default type stays driver-side.
    let issue_type = match parsed.issue_type {
        Some(Some(id)) => Some(id),
        _ => fetch_default_issue_type(&deps.pool, &project_id).await?,
    };
    let new_id = Uuid::new_v4();
    let created_at = now_utc();
    let updated_at = now_utc();
    let fields = domain_issues::IssueFieldSet {
        id: new_id,
        created_at,
        updated_at,
        created_by_id: None,
        updated_by_id: None,
        deleted_at: parsed.deleted_at.flatten(),
        project_id,
        workspace_id: project.workspace_id,
        parent_id: parsed.parent.flatten(),
        state_id: parsed.state.flatten(),
        point: parsed.point.flatten().map(|point| point as i32),
        estimate_point_id: parsed.estimate_point.flatten(),
        name: parsed.name.clone().unwrap_or_default(),
        description_json: Value::Object(Map::new()),
        description_html: parsed.description_html.clone().unwrap_or_default(),
        description_stripped: None,
        description_binary: None,
        priority: parsed.priority.clone().unwrap_or_else(|| "none".to_owned()),
        complexity_score: parsed.complexity_score.unwrap_or(0) as i32,
        start_date: parsed.start_date.flatten(),
        target_date: parsed.target_date.flatten(),
        sequence_id: 0,
        sort_order: parsed.sort_order.unwrap_or(65535.0),
        completed_at: parsed.completed_at.flatten(),
        archived_at: parsed.archived_at.flatten(),
        is_draft: parsed.is_draft.unwrap_or(false),
        external_source: parsed.external_source.clone().flatten(),
        external_id: parsed.external_id.clone().flatten(),
        type_id: issue_type,
        git_work_branch: parsed.git_work_branch.clone().unwrap_or_default(),
        workpad: String::new(),
        created_via: parsed.created_via.clone().flatten(),
        assigned_pod_id: parsed.assigned_pod.flatten(),
        agent_executor: parsed.agent_executor.clone().flatten(),
    };
    // The raw-data overrides resolve pre-save (pure), but their failures
    // surface post-commit in the chain leg, in port order — a bad override
    // 400s/500s with the row, fire and m2m legs already persisted. The
    // absent-key defaults evaluate here rather than past the m2m legs;
    // wall-clock only.
    let override_created_at = match body.get("created_at") {
        None => Ok(Some(now_utc())),
        Some(value) => coerce_created_at_override(value, &tz, tz_name).map_err(override_stop),
    };
    let override_created_by = match body.get("created_by") {
        None => Ok(Some(ctx.actor.user_id)),
        Some(value) => coerce_created_by_override(value).map_err(override_stop),
    };
    let requested_text = requested_data_text(&data);
    // `origin` arrives pre-resolved from settings (`base_host` raises past
    // the save — the row + `issue_activity` persist on a misconfigured
    // host — so the caller passes `None` there instead of failing).
    // Django's `post_save` fires inside `Issue.objects.create`, before the
    // assignee/label `bulk_create`s (and before the `created_at` /
    // `created_by` second save, whose transition no-ops on `prev == current`),
    // so the entry prompt sees empty m2m. A new row has no prior state, so
    // creating directly in a ticking state arms the clock and creates the
    // entry run (PIDASHCONV-1216).
    let actor_id = ctx.actor.user_id;
    let default_assignee_id = project.default_assignee_id;
    let transition: Arc<dyn IssueTransitionHook> = deps.transition.clone();
    let enqueue: Arc<dyn TaskEnqueueHook> = deps.enqueue.clone();
    let emit_activity = options.emit_activity;
    let model_activity = create_model_activity(
        new_id,
        data,
        actor_id,
        slug.to_owned(),
        write.origin.clone(),
        options.emit_model_activity,
    );
    let outcome = domain::run(ctx, &deps.orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = deps.pool.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::create(
                &ctx,
                txn,
                &pool,
                domain_issues::CreateIssue {
                    fields,
                    assignees: parsed.assignees.clone().unwrap_or_default(),
                    default_assignee: default_assignee_id,
                    labels: parsed.labels.clone().unwrap_or_default(),
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(actor_id),
                        moved_by_run: None,
                        now: Utc::now(),
                    },
                    origin: None,
                    activity_requested: Value::String(requested_text),
                    activity_notification: None,
                    activity_require_origin: false,
                    skip_activity: !emit_activity,
                    skip_fire: false,
                    model_activity,
                    created_override: Some(domain_issues::CreatedOverride {
                        created_at: override_created_at,
                        created_by: override_created_by,
                    }),
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_save_error)?;
    match outcome.chain.stopped() {
        None => {}
        Some(domain_issues::ChainStop::OverrideConflict) => {
            return Err(Error::FieldErrors(PAYLOAD_NOT_VALID_BODY.to_owned()));
        }
        Some(domain_issues::ChainStop::OverrideDetail) => {
            return Err(Error::BadError("Please provide valid detail".to_owned()));
        }
        Some(_) => return Err(Error::ServerError),
    }
    // The 201 renders the PRE-override in-memory instance: refetch the
    // row, then restore the insert-time `created_at` / `created_by`.
    let mut decoded = fetch_issue_for_write(&deps.pool, slug, &project_id, &new_id, &tz).await?;
    decoded.created_at = pidash_compat::datetime::render_datetime_in(&created_at, &tz);
    decoded.created_at_dt = created_at;
    decoded.created_by = Some(ctx.actor.user_id.to_string());
    let rendered =
        render_write_response(&deps.pool, &decoded, &tz, write.web_base.as_deref()).await?;
    Ok(Outcome::created(
        serde_json::to_string(&rendered).map_err(|_| Error::ServerError)?,
    ))
}

/// The patch guard's nullable-`external_source` variant
/// (`request.data.get("external_source", issue.external_source)` —
/// `None` filters `IS NULL`).
async fn external_dup_first_nullable(
    pool: &PgPool,
    project_id: &Uuid,
    slug: &str,
    source: Option<&str>,
    external_id: &str,
) -> Result<Option<Uuid>, Error> {
    let exists: Option<bool> = sqlx::query_scalar(
        r#"SELECT EXISTS(SELECT 1 FROM "issues" INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") WHERE "issues"."project_id" = $1 AND "workspaces"."slug" = $2 AND "issues"."external_source" IS NOT DISTINCT FROM $3 AND "issues"."external_id" = $4 AND "issues"."deleted_at" IS NULL)"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-dup-exists"))?;
    if !exists.unwrap_or(false) {
        return Ok(None);
    }
    sqlx::query_scalar(
        r#"SELECT "issues"."id" FROM "issues" INNER JOIN "workspaces" ON ("issues"."workspace_id" = "workspaces"."id") WHERE "issues"."project_id" = $1 AND "workspaces"."slug" = $2 AND "issues"."external_source" IS NOT DISTINCT FROM $3 AND "issues"."external_id" = $4 AND "issues"."deleted_at" IS NULL ORDER BY "issues"."created_at" DESC LIMIT 1"#,
    )
    .bind(project_id)
    .bind(slug)
    .bind(source)
    .bind(external_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| db_error(error, "write-dup-first"))
    .map(|row: Option<Uuid>| row)
}

/// `PATCH .../issues/<pk>/` (`IssueDetailAPIEndpoint`,
/// `views/issue.py:790-909`): `IssueSerializer(issue, data,
/// partial=True).save()` plus the render.
///
/// The caller already ran auth, the gate and body negotiation (see
/// [`create`]); `target` carries the pre-resolved update inputs. The
/// resolved run id stamps `MOVED_BY_RUN_ATTR` for the post-save
/// orchestration signal (PIDASHCONV-804) and has no wire effect on its own.
pub async fn update(
    deps: &Deps,
    ctx: &WriteCtx,
    pk: Uuid,
    write: &IssueWrite<'_>,
    target: &UpdateTarget<'_>,
    options: UpdateOptions,
) -> Result<Outcome, Error> {
    let slug = write.slug;
    let project_id = write.project_id;
    let workspace_id = write.workspace_id;
    let body = write.body;
    let from_form = write.from_form;
    let current = target.current;
    let tz = activate_timezone(ctx.actor.timezone.as_deref())?;
    let tz_name = ctx.actor.timezone.as_deref().unwrap_or("UTC");
    // The issue row and the project row arrive pre-fetched (see
    // [`IssueWrite::project`]): the issue fetch precedes the project
    // fetch (`:792-793`), so the handler 404s in that order before
    // parsing the body.

    let norm_input = if from_form {
        form_dict_for_normalize(body)
    } else {
        body.clone()
    };
    let (data, from_markdown) = normalize_view_body(&norm_input, &markdown_to_html_port)?;
    let Value::Object(map) = &data else {
        return Err(Error::FieldErrors(non_dict_body(&data)));
    };
    let mut parsed = parse_issue_write(&deps.pool, map, true, from_form, &tz, tz_name).await?;
    run_write_validate(
        &deps.pool,
        &mut parsed,
        &project_id,
        &workspace_id,
        Some(current),
        from_markdown,
    )
    .await?;

    // External-duplicate guard (`:822-841`): only when the raw id is
    // truthy AND changed (`stored != str(incoming)`), the source
    // defaulting to the stored one — and the 409 carries the CURRENT
    // issue's id, not the conflicting row's.
    if let Some(raw_id) = body.get("external_id") {
        if raw_truthy(raw_id) {
            let incoming = raw_text(raw_id);
            if current.external_id.as_deref() != incoming.as_deref() {
                let source: Option<String> = match body.get("external_source") {
                    Some(value) => raw_text(value),
                    None => current.external_source.clone(),
                };
                if let Some(external_id) = incoming {
                    if external_dup_first_nullable(
                        &deps.pool,
                        &project_id,
                        slug,
                        source.as_deref(),
                        &external_id,
                    )
                    .await?
                    .is_some()
                    {
                        return Err(Error::Conflict(external_dup_body(&pk)));
                    }
                }
            }
        }
    }

    // `serializer.save()` → `update()` (`:368-416`): the m2m legs run ahead
    // of the row write inside `domain::issues::partial_update`, then the
    // full-column `save()` (`updated_at`/`updated_by` and the
    // stripped/completed recomputes always land, even for `{}`), the
    // completion pair when the state moved, the always-immediate fire, and
    // the two activities (no version task on this view).
    //
    // The save-time `sequence_id` int4 check resolves here (pure) but
    // surfaces past the m2m legs in the domain write — a bad sequence 500s
    // with the legs already persisted, as today.
    let sequence = if parsed.sequence_overflow {
        Some(Err(()))
    } else {
        parsed
            .sequence_id
            .map(|number| i32::try_from(number).map_err(|_| ()))
    };
    // The pre-save snapshot (`capture_prior_state`, `signals.py:61-71`):
    // the stored `state_id` re-read just before the save, past every
    // guard, as `pre_save` runs inside `serializer.save()`. A storage
    // failure 500s, as in Python where only the handler call is in
    // `try`.
    let prev_state_id = crate::domain::transition::capture_prior_state_pool(&deps.pool, Some(pk))
        .await
        .map_err(|_| Error::ServerError)?;
    // The full-save base row, re-read beside the snapshot: the `DecodedIssue`
    // above is render-lossy (tz-rendered datetimes, no `description_json` /
    // `workpad`), and the save writes every column. A miss 404s — the port
    // would run the m2m legs first and 404 on the final refetch instead, a
    // mid-flight-delete race either way.
    let Some(base) = issues::Entity::find_by_id(pk)
        .one(&deps.orm)
        .await
        .map_err(|error| db_error(error, "write-reread"))?
    else {
        return Err(Error::NotFound);
    };
    let mut fields = domain_issues::IssueFieldSet::from(&base);
    let audit = (base.created_by_id, base.updated_by_id);
    // The validated overlay: present keys move (nullable columns take the
    // OUTER option — explicit null stores `NULL`, absent keeps the row),
    // `state`/`completed_at` assemble for the hook arms (a provided state
    // recompletes, explicit-null/absent resolves the default and keeps the
    // validated-or-current `completed_at`).
    fields.updated_at = now_utc();
    if let Some(value) = parsed.description_html.clone() {
        fields.description_html = value;
    }
    if let Some(value) = parsed.deleted_at {
        fields.deleted_at = value;
    }
    if let Some(value) = parsed.point {
        fields.point = value.map(|point| point as i32);
    }
    if let Some(value) = parsed.name.clone() {
        fields.name = value;
    }
    if let Some(value) = parsed.priority.clone() {
        fields.priority = value;
    }
    if let Some(value) = parsed.complexity_score {
        fields.complexity_score = value as i32;
    }
    if let Some(value) = parsed.start_date {
        fields.start_date = value;
    }
    if let Some(value) = parsed.target_date {
        fields.target_date = value;
    }
    if let Some(value) = parsed.sort_order {
        fields.sort_order = value;
    }
    let row_completed = fields.completed_at;
    fields.completed_at = parsed.completed_at.flatten().or(row_completed);
    if let Some(value) = parsed.archived_at {
        fields.archived_at = value;
    }
    if let Some(value) = parsed.is_draft {
        fields.is_draft = value;
    }
    if let Some(value) = parsed.external_source.clone() {
        fields.external_source = value;
    }
    if let Some(value) = parsed.external_id.clone() {
        fields.external_id = value;
    }
    if let Some(value) = parsed.git_work_branch.clone() {
        fields.git_work_branch = value;
    }
    if let Some(value) = parsed.created_via.clone() {
        fields.created_via = value;
    }
    if let Some(value) = parsed.agent_executor.clone() {
        fields.agent_executor = value;
    }
    if let Some(value) = parsed.created_by {
        fields.created_by_id = value;
    }
    if let Some(value) = parsed.parent {
        fields.parent_id = value;
    }
    if let Some(value) = parsed.estimate_point {
        fields.estimate_point_id = value;
    }
    if let Some(value) = parsed.issue_type {
        fields.type_id = value;
    }
    if let Some(value) = parsed.assigned_pod {
        fields.assigned_pod_id = value;
    }
    fields.state_id = match parsed.state {
        Some(value) => value,
        None => base.state_id,
    };
    // `origin` arrives pre-resolved from settings (`base_host` raises past
    // the fire — the save + `issue_activity` persist on a misconfigured
    // host — so the caller passes `None` there instead of failing).
    let requested_text = requested_data_text(&data);
    let actor_id = ctx.actor.user_id;
    let transition: Arc<dyn IssueTransitionHook> = deps.transition.clone();
    let enqueue: Arc<dyn TaskEnqueueHook> = deps.enqueue.clone();
    let origin = write.origin.clone();
    let emit_activity = options.emit_activity;
    let moved_by_run = target.moved_by_run;
    let current_instance = target.current_instance.clone();
    let outcome = domain::run(ctx, &deps.orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let pool = deps.pool.clone();
        let ctx = ctx.clone();
        let slug = slug.to_owned();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            domain_issues::partial_update(
                &ctx,
                txn,
                &pool,
                domain_issues::PartialIssue {
                    m2m_audit: audit,
                    sequence,
                    fields,
                    assignees: parsed.assignees.clone(),
                    labels: parsed.labels.clone(),
                    prev_state: prev_state_id,
                    fire: domain_issues::FireSpec {
                        dispatch_immediate: true,
                        created_by: Some(actor_id),
                        moved_by_run,
                        now: Utc::now(),
                    },
                    activity: if emit_activity {
                        Some(domain_issues::UpdateActivity {
                            origin,
                            requested: requested_text,
                            current_instance,
                            raw_body: data,
                            slug,
                            notification: None,
                            require_origin: false,
                            emit_version: false,
                        })
                    } else {
                        None
                    },
                },
            )
            .await
        })
    })
    .await
    .map_err(domain_save_error)?;
    if outcome.chain.stopped().is_some() {
        return Err(Error::ServerError);
    }
    let decoded = fetch_issue_for_write(&deps.pool, slug, &project_id, &pk, &tz).await?;
    let rendered =
        render_write_response(&deps.pool, &decoded, &tz, write.web_base.as_deref()).await?;
    Ok(Outcome::ok(
        serde_json::to_string(&rendered).map_err(|_| Error::ServerError)?,
    ))
}

// ---------------------------------------------------------------------------
// Move (`IssueMoveAPIView`, `views/issue.py:910-937`): the serializer step
// of the project move — `move_issue` (state/label remap, sequence,
// handoff) plus the render. Moved from
// `pidash_api::v1_work_items::handlers_move_search` (OVL-14,
// PIDASHCONV-1649).
// ---------------------------------------------------------------------------

/// The tenant scope and negotiated payload for a project move: what
/// the caller resolved before calling the operation (auth, gate, route
/// params, body negotiation, settings).
#[derive(Debug, Clone)]
pub struct MoveWrite<'a> {
    /// The workspace slug from the route.
    pub slug: &'a str,
    /// The gated source project id (after identifier rewrite).
    pub project_id: Uuid,
    /// The issue id from the route.
    pub pk: Uuid,
    /// The move's `target_ref` (`views/issue.py:920-924`): the negotiated
    /// body's `project` member (missing reads JSON null, which the driver
    /// blanks to the required-400).
    pub target_ref: &'a Value,
    /// The settings-derived app origin (`base_host`, `is_app=True`), if
    /// configured. Origin-less callers (the MCP tools, no HTTP request)
    /// pass `None`; the domain move enqueues the `model_activity` leg
    /// with a JSON-null origin, exactly Django's `.delay(origin=None)`
    /// wire bytes.
    pub origin: Option<String>,
    /// The settings-derived web base for rendered URLs, if configured.
    pub web_base: Option<String>,
}

/// `POST .../issues/<pk>/move/` (`IssueMoveAPIView`,
/// `views/issue.py:910-937`): the `move_issue` flow (state/label remap,
/// sequence, handoff) plus the render.
///
/// The caller already ran auth, the gate and body negotiation (see
/// [`create`]); `drain`/`creation` are the move's post-commit drain and
/// span-creation dependencies, built from the binary's seams. `options`
/// is the caller's [`domain_issues::MoveOptions`], forwarded into the
/// kernel input: the v1 handler passes
/// [`domain_issues::MoveOptions::django`]; callers the views do not
/// serve (the MCP tools) pass their own (design D6).
pub async fn move_to_project(
    deps: &Deps,
    ctx: &WriteCtx,
    write: &MoveWrite<'_>,
    options: domain_issues::MoveOptions,
    drain: Arc<dyn domain_issues::MovePostCommitHook>,
    creation: domain_issues::CreationDeps,
) -> Result<Outcome, Error> {
    let slug = write.slug;
    let project_id = write.project_id;
    let pk = write.pk;
    let target_ref = write.target_ref.clone();
    let tz = activate_timezone(ctx.actor.timezone.as_deref())?;
    let web_base = write.web_base.clone();
    let now = Utc::now();
    // The pre-save snapshot, before the move's own write.
    let prev = crate::domain::transition::capture_prior_state_pool(&deps.pool, Some(pk))
        .await
        .map_err(|_| Error::ServerError)?;
    let transition: Arc<dyn IssueTransitionHook> = deps.transition.clone();
    let enqueue: Arc<dyn TaskEnqueueHook> = deps.enqueue.clone();
    let origin = write.origin.clone();
    // Owned copies for the run closure: the `for<'t>` work bound
    // unifies only moved values and local borrows, not the caller's
    // `&str` / `&Uuid` params — and the render below still needs
    // `web_base` afterwards.
    let slug_owned = slug.to_owned();
    let pk_owned = pk;
    let web_base_owned = web_base.clone();
    let outcome = domain::run(ctx, &deps.orm, |txn| {
        let transition = transition.clone();
        let enqueue = enqueue.clone();
        let drain = drain.clone();
        let ctx = ctx.clone();
        Box::pin(async move {
            txn.hooks_mut().register(transition);
            txn.hooks_mut().register(enqueue);
            txn.hooks_mut().register(drain);
            domain_issues::move_to_project(
                &ctx,
                txn,
                domain_issues::MoveInput {
                    slug: &slug_owned,
                    project_id,
                    pk: pk_owned,
                    target_ref: &target_ref,
                    origin: origin.clone(),
                    web_base_url: web_base_owned.as_deref(),
                    now,
                    prev_state: prev,
                    options,
                    creation,
                },
            )
            .await
        })
    })
    .await
    .map_err(map_move_domain_error)?;
    match outcome {
        domain_issues::MoveDomainOutcome::AlreadyThere {
            row,
            workspace_slug,
            project_identifier,
        } => {
            // Same project: no writes at all, no signals, no enqueues —
            // just the issue in the API read shape. The domain resolved
            // the serializer's lazy url pair past the commit —
            // FK-guaranteed.
            let slug = workspace_slug.ok_or(Error::ServerError)?;
            let identifier = project_identifier.ok_or(Error::ServerError)?;
            let body = render_moved_issue_api(
                &deps.pool,
                &row,
                &slug,
                &identifier,
                web_base.as_deref(),
                &tz,
            )
            .await?;
            Ok(Outcome::ok(body))
        }
        domain_issues::MoveDomainOutcome::Moved { row, chain } => {
            if chain.stopped().is_some() {
                return Err(Error::ServerError);
            }
            let body = render_moved_issue_api(
                &deps.pool,
                &row.row,
                &row.workspace_slug,
                &row.project_identifier,
                web_base.as_deref(),
                &tz,
            )
            .await?;
            Ok(Outcome::ok(body))
        }
        domain_issues::MoveDomainOutcome::Rejected(rejection) => Err(map_move_rejection(rejection)),
    }
}

/// Map the 650 driver's failures onto the view's responses: the
/// recoverable `{"error"}` + status, the source-miss 404, the
/// resolve-miss 404 `detail`, anything else the 500.
fn map_move_rejection(rejection: domain_issues::MoveRejection) -> Error {
    match rejection {
        domain_issues::MoveRejection::Issue {
            message,
            status_code,
        } => Error::Rejection {
            status: status_code,
            message,
        },
        domain_issues::MoveRejection::SourceNotFound => Error::NotFound,
        domain_issues::MoveRejection::TargetNotFound => Error::ProjectNotFound,
        // `Dual` callers only: the v1 handler passes `TargetOnly` (its
        // gate checked the source project up front), while overlay
        // drivers pre-check the source themselves — so the kernel's
        // denial is their TOCTOU twin, and the 500 matches the driver's
        // own internal mapping for those twins.
        domain_issues::MoveRejection::SourceDenied => Error::ServerError,
    }
}

/// Map a domain-run failure: a store error is the generic 500; a work
/// error is the kernel's post-save 409 (the multiple-executing-runs
/// arm — the span rolled back, as Django's `atomic` exit on raise).
fn map_move_domain_error(error: domain::DomainError) -> Error {
    match error {
        domain::DomainError::Db(_) => Error::ServerError,
        domain::DomainError::Work(message) => Error::Rejection {
            status: 409,
            message,
        },
    }
}

/// The move's 200 body: `IssueSerializer(issue).data` (`views/issue.py:937`)
/// over the refreshed (or same-project) issue — a bare instance, so no
/// `fields`/`expand` context and no relations viewer (the `relations`
/// block omits), while the blocker summary always queries
/// (`serializers/issue.py:436-493`).
async fn render_moved_issue_api(
    pool: &PgPool,
    issue: &SourceIssueRow,
    workspace_slug: &str,
    project_identifier: &str,
    web_base: Option<&str>,
    tz: &Tz,
) -> Result<String, Error> {
    // Serializer read order (`to_representation`): assignees, labels,
    // then the blocker summary (blocked_by, blocking, has_open) — the
    // same seam reads the driver used pre-move, now past the commit.
    let assignee_rows: Vec<(Uuid,)> = sqlx::query_as(ASSIGNEE_IDS_SQL)
        .bind(issue.id)
        .fetch_all(pool)
        .await
        .map_err(|_| Error::ServerError)?;
    let assignee_ids: Vec<String> = assignee_rows
        .into_iter()
        .map(|row| row.0.to_string())
        .collect();
    let label_rows: Vec<(Uuid,)> = sqlx::query_as(LABEL_IDS_SQL)
        .bind(issue.id)
        .fetch_all(pool)
        .await
        .map_err(|_| Error::ServerError)?;
    let label_ids: Vec<String> = label_rows
        .into_iter()
        .map(|row| row.0.to_string())
        .collect();
    let blocked_by_rows = sqlx::query(&issue_param(summary_sql(false)))
        .bind(issue.id)
        .fetch_all(pool)
        .await
        .map_err(|_| Error::ServerError)?;
    let blocked_by: Vec<BlockerRow> = blocked_by_rows
        .iter()
        .map(map_blocker_row)
        .collect::<Result<_, _>>()
        .map_err(|_| Error::ServerError)?;
    let blocking_rows = sqlx::query(&issue_param(summary_sql(true)))
        .bind(issue.id)
        .fetch_all(pool)
        .await
        .map_err(|_| Error::ServerError)?;
    let blocking: Vec<BlockerRow> = blocking_rows
        .iter()
        .map(map_blocker_row)
        .collect::<Result<_, _>>()
        .map_err(|_| Error::ServerError)?;
    let open_hit: Option<(i32,)> = sqlx::query_as(&issue_param(has_open_blockers_sql()))
        .bind(issue.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Error::ServerError)?;
    let has_open_blockers = open_hit.is_some();
    let blocked_by_items: Vec<SummaryItem> = blocked_by.iter().map(summary_item).collect();
    let blocking_items: Vec<SummaryItem> = blocking.iter().map(summary_item).collect();
    let blockers = BlockerSummary {
        blocked_by: blocked_by_items,
        blocking: blocking_items,
        has_open_blockers,
    };

    let id = issue.id.to_string();
    let created_at = pidash_compat::datetime::render_datetime_in(&issue.created_at, tz);
    let updated_at = pidash_compat::datetime::render_datetime_in(&issue.updated_at, tz);
    let deleted_at = issue
        .deleted_at
        .as_ref()
        .map(|dt| pidash_compat::datetime::render_datetime_in(dt, tz));
    let completed_at = issue
        .completed_at
        .as_ref()
        .map(|dt| pidash_compat::datetime::render_datetime_in(dt, tz));
    let archived_at = issue
        .archived_at
        .as_ref()
        .map(|dt| dt.date_naive().to_string());
    let start_date = issue.start_date.as_ref().map(ToString::to_string);
    let target_date = issue.target_date.as_ref().map(ToString::to_string);
    let project = issue.project_id.to_string();
    let workspace = issue.workspace_id.to_string();
    let parent = issue.parent_id.map(|id| id.to_string());
    let state = issue.state_id.map(|id| id.to_string());
    let estimate_point = issue.estimate_point_id.map(|id| id.to_string());
    let assigned_pod = issue.assigned_pod_id.map(|id| id.to_string());
    let created_by = issue.created_by_id.map(|id| id.to_string());
    let updated_by = issue.updated_by_id.map(|id| id.to_string());
    let url = crate::v1_work_items::shape_issue::issue_url(
        web_base,
        Some(workspace_slug),
        Some(project_identifier),
        Some(issue.sequence_id),
    );
    let assignee_refs: Vec<&str> = assignee_ids.iter().map(String::as_str).collect();
    let label_refs: Vec<&str> = label_ids.iter().map(String::as_str).collect();
    let row = ApiIssueRow {
        id: &id,
        type_id: issue.type_id.as_deref(),
        url,
        created_at: &created_at,
        updated_at: &updated_at,
        deleted_at: deleted_at.as_deref(),
        point: issue.point,
        name: &issue.name,
        description_html: &issue.description_html,
        description_binary: issue.description_binary.as_deref(),
        priority: &issue.priority,
        complexity_score: issue.complexity_score,
        start_date: start_date.as_deref(),
        target_date: target_date.as_deref(),
        sequence_id: issue.sequence_id,
        sort_order: issue.sort_order,
        completed_at: completed_at.as_deref(),
        archived_at: archived_at.as_deref(),
        is_draft: issue.is_draft,
        external_source: issue.external_source.as_deref(),
        external_id: issue.external_id.as_deref(),
        git_work_branch: &issue.git_work_branch,
        created_via: issue.created_via.as_deref(),
        agent_executor: issue.agent_executor.as_deref(),
        created_by: created_by.as_deref(),
        updated_by: updated_by.as_deref(),
        project: &project,
        workspace: &workspace,
        parent: parent.as_deref(),
        state: state.as_deref(),
        estimate_point: estimate_point.as_deref(),
        assigned_pod: assigned_pod.as_deref(),
    };
    let input = RepresentationInput {
        row: &row,
        fields: None,
        expand: &[],
        is_list: false,
        assignee_ids: assignee_refs.as_slice(),
        assignee_rows: &[],
        label_ids: label_refs.as_slice(),
        expanded_labels: &[],
        blockers: Some(&blockers),
        relations: None,
        expansions: &[],
    };
    // The binary/NaN arms reproduce Django 500s; the `Missing*` arms are
    // unreachable (blockers supplied, no fields filter).
    let view =
        crate::v1_work_items::shape_issue::render_issue(&input).map_err(|_| Error::ServerError)?;
    serde_json::to_string(&view).map_err(|_| Error::ServerError)
}

/// One `relations_summary` item (`orchestration/blockers.py:184-198`):
/// `{identifier, state, state_group}` — the composed `PROJ-42`
/// identifier, never the bare sequence.
fn summary_item(row: &BlockerRow) -> SummaryItem<'_> {
    SummaryItem {
        identifier: format!("{}-{}", row.project_identifier, row.sequence_id),
        state: row.state_name.as_deref(),
        state_group: row.state_group.as_deref(),
    }
}

/// Map one `blockers::summary_sql` row.
fn map_blocker_row(
    row: &sqlx::postgres::PgRow,
) -> Result<BlockerRow, crate::app_issues::issue_move::StoreError> {
    use crate::app_issues::issue_move::StoreError;
    let fail = |error: sqlx::Error| StoreError(error.to_string());
    Ok(BlockerRow {
        issue_id: row.try_get("id").map_err(fail)?,
        sequence_id: row.try_get("sequence_id").map_err(fail)?,
        project_identifier: row.try_get("identifier").map_err(fail)?,
        state_name: row.try_get("name").map_err(fail)?,
        state_group: row.try_get("group").map_err(fail)?,
    })
}

/// Replace the `:issue_id` anchor placeholder the blocker builders emit
/// with the caller's `$1` bind.
fn issue_param(sql: String) -> String {
    sql.replace(":issue_id", "$1")
}

#[cfg(test)]
mod move_tests {
    use super::*;

    #[test]
    fn map_move_rejection_maps_driver_failures() {
        let rejection = map_move_rejection(domain_issues::MoveRejection::Issue {
            message: "project is required".to_owned(),
            status_code: 400,
        });
        assert_eq!(
            rejection,
            Error::Rejection {
                status: 400,
                message: "project is required".to_owned(),
            }
        );
        assert_eq!(
            map_move_rejection(domain_issues::MoveRejection::SourceNotFound),
            Error::NotFound
        );
        assert_eq!(
            map_move_rejection(domain_issues::MoveRejection::TargetNotFound),
            Error::ProjectNotFound
        );
        assert_eq!(
            map_move_rejection(domain_issues::MoveRejection::SourceDenied),
            Error::ServerError
        );
        let rejection = map_move_domain_error(domain::DomainError::Work("busy".to_owned()));
        assert_eq!(
            rejection,
            Error::Rejection {
                status: 409,
                message: "busy".to_owned(),
            }
        );
        assert_eq!(
            map_move_domain_error(domain::DomainError::Db(sea_orm::DbErr::Custom(
                "boom".to_owned()
            ))),
            Error::ServerError
        );
    }

    #[test]
    fn blocker_anchor_rewrites() {
        assert_eq!(
            issue_param("a :issue_id b :issue_id".to_owned()),
            "a $1 b $1"
        );
        let blocked = issue_param(summary_sql(false));
        let blocking = issue_param(summary_sql(true));
        let open = issue_param(has_open_blockers_sql());
        for sql in [&blocked, &blocking, &open] {
            assert!(!sql.contains(":issue_id"), "{sql}");
            assert!(sql.contains("$1"), "{sql}");
        }
    }

    #[test]
    fn summary_item_composes_identifier() {
        let row = BlockerRow {
            issue_id: Uuid::nil(),
            sequence_id: 42,
            project_identifier: "PROJ".to_owned(),
            state_name: Some("Todo".to_owned()),
            state_group: Some("unstarted".to_owned()),
        };
        let item = summary_item(&row);
        assert_eq!(item.identifier, "PROJ-42");
        assert_eq!(item.state, Some("Todo"));
        assert_eq!(item.state_group, Some("unstarted"));
    }

    // --- Live operation tests: the caller's `MoveOptions` reach the kernel ---

    use crate::domain::issues_move::MoveCreationOutboxes;
    use crate::domain::WriteActor;
    use crate::orchestration::entries::{FireOutcome, FireRequest};
    use pidash_db::config::{CloudAgentSettings, ManagedRunnerSettings};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;

    async fn live_pool() -> Option<sqlx::PgPool> {
        let url = match std::env::var("DATABASE_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping live-db test: DATABASE_URL is not set");
                return None;
            }
        };
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(2)
                .connect(&url)
                .await
                .expect("connect to scratch DATABASE_URL"),
        )
    }

    type CommentCall = (Uuid, Option<Uuid>, Option<Uuid>);

    struct RecordingTransition {
        comments: Mutex<Vec<CommentCall>>,
        fires: Mutex<Vec<FireRequest>>,
    }

    impl IssueTransitionHook for RecordingTransition {
        fn enqueue_completion_comment(
            &self,
            issue_id: Uuid,
            prev_state_id: Option<Uuid>,
            current_state_id: Option<Uuid>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.comments
                .lock()
                .expect("lock")
                .push((issue_id, prev_state_id, current_state_id));
            Box::pin(async move {})
        }

        fn fire(
            &self,
            _actor_id: Uuid,
            request: FireRequest,
        ) -> Pin<Box<dyn Future<Output = Result<FireOutcome, ()>> + Send + '_>> {
            self.fires.lock().expect("lock").push(request);
            Box::pin(async move { Ok(FireOutcome::NoTransition) })
        }
    }

    type EnqueueCall = (String, Vec<Value>, serde_json::Map<String, Value>);

    struct RecordingEnqueue {
        calls: Mutex<Vec<EnqueueCall>>,
    }

    impl TaskEnqueueHook for RecordingEnqueue {
        fn enqueue_task(
            &self,
            task: &str,
            args: Vec<Value>,
            kwargs: serde_json::Map<String, Value>,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.calls
                .lock()
                .expect("lock")
                .push((task.to_owned(), args, kwargs));
            Box::pin(async move {})
        }
    }

    struct RecordingDrain;

    impl domain_issues::MovePostCommitHook for RecordingDrain {
        fn drain_creation(
            &self,
            _outboxes: MoveCreationOutboxes,
            _terminal_created_by: Option<Uuid>,
        ) -> Pin<Box<dyn Future<Output = Result<(), domain_issues::MoveDrainError>> + Send + '_>>
        {
            Box::pin(async move { Ok(()) })
        }

        fn drain_pod(
            &self,
            _pod_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = Result<(), domain_issues::MoveDrainError>> + Send + '_>>
        {
            Box::pin(async move { Ok(()) })
        }

        fn send_cancel(
            &self,
            _runner_id: Uuid,
            _run_id: Uuid,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(async move {})
        }
    }

    struct FakeAdmission;

    impl domain_issues::MoveAdmissionCache for FakeAdmission {
        fn bucket_count(&self, _key: &str) -> Result<Option<i64>, String> {
            Ok(None)
        }

        fn add_or_incr(&self, _key: &str, _timeout_secs: i64) -> Result<(), String> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct NoAssistant;

    impl crate::extensions::AssistantModelSeam for NoAssistant {
        fn generate_title(
            &self,
            _request: crate::extensions::TitleRequest,
        ) -> crate::extensions::SeamFuture<'_, Result<String, crate::extensions::TitleError>>
        {
            Box::pin(async move { unreachable!("move tests never generate titles") })
        }
    }

    fn test_creation_deps(pool: sqlx::PgPool) -> domain_issues::CreationDeps {
        domain_issues::CreationDeps {
            pool,
            cloud: CloudAgentSettings {
                enabled: false,
                writes_enabled: false,
                github_tools_enabled: false,
                disabled_tools: Vec::new(),
                reconcile_interval_secs: 0,
                model_request_timeout_secs: 0,
                execution_timeout_secs: 0,
                run_soft_limit_secs: 0,
                run_hard_limit_secs: 0,
                stale_grace_secs: 0,
                dispatch_lease_secs: 0,
                dispatch_backoff_secs: 0,
                dispatch_scan_interval_secs: 0,
                sweep_interval_secs: 0,
                dispatch_scan_batch: 0,
                max_queue_age_secs: 0,
                model_request_limit: 0,
                tool_call_limit: 0,
                write_call_limit: 0,
                input_token_limit: 0,
                output_token_limit: 0,
                total_token_limit: 0,
                max_output_tokens_per_request: 0,
                max_queued_per_workspace: 0,
                max_running_per_workspace: 0,
                user_creation_rate_per_minute: 0,
                workspace_creation_rate_per_minute: 0,
                tool_timeout_secs: 0,
                max_tool_result_bytes: 0,
                max_prompt_bytes: 0,
                max_final_result_bytes: 0,
                max_events: 0,
                block_private_urls: false,
            },
            managed: ManagedRunnerSettings {
                enabled: false,
                max_per_user_project: 0,
                queued_max_age_secs: 0,
                graceful_stop_secs: 0,
                sweep_interval_secs: 0,
                desktop_min_version: String::new(),
            },
            now_unix_secs: 0,
            toolsets: Arc::new(crate::extensions::NoExtraToolsets),
            assistant_seam: Arc::new(NoAssistant),
            admission: Arc::new(FakeAdmission),
        }
    }

    /// The MCP caller's options (`execute_cross_project_move`): dual
    /// guards, the `updated_by` stamp, no handoff.
    fn mcp_options() -> domain_issues::MoveOptions {
        domain_issues::MoveOptions {
            guards: domain_issues::MoveGuards::Dual,
            stamp_updated_by: true,
            handoff: false,
            dispatch_immediate: None,
        }
    }

    struct MoveScope {
        slug: String,
        actor_full: Uuid,
        actor_limited: Uuid,
        source_project: Uuid,
        target_project: Uuid,
        target_identifier: String,
        issue_id: Uuid,
    }

    /// Seed one workspace with a source project (one issue, one backlog
    /// state) and a target project (triage + default backlog states).
    /// `actor_full` is a member of both projects; `actor_limited` of the
    /// target only — so the MCP options succeed for the former and deny
    /// the latter at the kernel's `Dual` source probe.
    async fn seed_move_scope(pool: &sqlx::PgPool, tag: &str) -> MoveScope {
        let actor_full = Uuid::new_v4();
        let actor_limited = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let slug = format!("opmv-{tag}-{}", workspace_id.simple());
        let source_project = Uuid::new_v4();
        let target_project = Uuid::new_v4();
        let source_state = Uuid::new_v4();
        let issue_id = Uuid::new_v4();
        let target_identifier = format!("M{:08X}", (target_project.as_u128() & 0xffff_ffff) as u32);

        for actor in [actor_full, actor_limited] {
            let username = format!("opmv-{tag}-{}", actor.simple());
            sqlx::query(
                "INSERT INTO users (id, password, username, email, first_name, last_name, avatar, \
                 date_joined, created_at, updated_at, last_location, created_location, is_superuser, \
                 is_managed, is_password_expired, is_active, is_staff, is_email_verified, \
                 is_password_autoset, token, user_timezone, last_login_ip, last_logout_ip, \
                 last_login_medium, last_login_uagent, is_bot, display_name, is_email_valid, \
                 is_password_reset_required) VALUES ($1, '!', $2, $3, 'CT', 'User', '', now(), now(), \
                 now(), '', '', false, false, false, true, false, true, false, $4, 'UTC', '', '', '', \
                 '', false, $2, true, false)",
            )
            .bind(actor)
            .bind(username.clone())
            .bind(format!("{username}@example.test"))
            .bind("t".repeat(64))
            .execute(pool)
            .await
            .expect("seed user");
        }
        sqlx::query(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, timezone, \
             background_color, owner_id) VALUES ($1, now(), now(), 'opmv ws', $2, 'UTC', '#ffffff', \
             $3)",
        )
        .bind(workspace_id)
        .bind(&slug)
        .bind(actor_full)
        .execute(pool)
        .await
        .expect("seed workspace");
        for (id, identifier) in [
            (
                source_project,
                format!("S{:08X}", (source_project.as_u128() & 0xffff_ffff) as u32),
            ),
            (target_project, target_identifier.clone()),
        ] {
            sqlx::query(
                "INSERT INTO projects (id, name, description, network, identifier, workspace_id, \
                 created_by_id, cycle_view, module_view, issue_views_view, page_view, intake_view, \
                 archive_in, close_in, logo_props, is_time_tracking_enabled, is_issue_type_enabled, \
                 guest_view_all_features, timezone, members_can_edit_states, repo_url, base_branch, \
                 agent_default_interval_seconds, agent_default_max_ticks, agent_ticking_enabled, \
                 is_default, agent_review_default_interval_seconds, default_agent_executor, \
                 agent_test_default_interval_seconds, created_at, updated_at) VALUES ($1, \
                 $2, '', 0, $3, $4, $5, true, true, true, true, false, 0, 0, '{}', true, \
                 true, false, 'UTC', false, '', '', 0, 0, false, false, 0, '', 0, now(), now())",
            )
            .bind(id)
            .bind(format!("opmv-project-{identifier}"))
            .bind(identifier)
            .bind(workspace_id)
            .bind(actor_full)
            .execute(pool)
            .await
            .expect("seed project");
        }
        for (project, member) in [
            (source_project, actor_full),
            (target_project, actor_full),
            (target_project, actor_limited),
        ] {
            sqlx::query(
                "INSERT INTO project_members (id, role, project_id, workspace_id, member_id, \
                 view_props, default_props, sort_order, preferences, is_active, created_at, \
                 updated_at) VALUES ($1, 15, $2, $3, $4, '{}', '{}', 65535, '{}', true, now(), now())",
            )
            .bind(Uuid::new_v4())
            .bind(project)
            .bind(workspace_id)
            .bind(member)
            .execute(pool)
            .await
            .expect("seed membership");
        }
        for (id, project, name, group, sequence, default) in [
            (
                source_state,
                source_project,
                "Backlog",
                "backlog",
                100.0,
                false,
            ),
            (
                Uuid::new_v4(),
                target_project,
                "Triage",
                "triage",
                100.0,
                false,
            ),
            (
                Uuid::new_v4(),
                target_project,
                "Backlog",
                "backlog",
                200.0,
                true,
            ),
        ] {
            sqlx::query(
                "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
                 project_id, workspace_id, sequence, \"group\", \"default\", is_triage) VALUES \
                 ($1, now(), now(), $2, '', '#ff0000', $3, $4, $5, $6, $7, $8, false)",
            )
            .bind(id)
            .bind(name)
            .bind(format!("opmv-{name}-{sequence}-{project}"))
            .bind(project)
            .bind(workspace_id)
            .bind(sequence)
            .bind(group)
            .bind(default)
            .execute(pool)
            .await
            .expect("seed state");
        }
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, project_id, workspace_id, state_id, \
             name, description_json, description_html, sequence_id, sort_order, priority, \
             complexity_score, is_draft, git_work_branch, workpad) VALUES ($1, now(), now(), $2, \
             $3, $4, 'opmv issue', '{}', '<p>opmv</p>', 3, 10000.0, 'none', 0, false, '', '')",
        )
        .bind(issue_id)
        .bind(source_project)
        .bind(workspace_id)
        .bind(source_state)
        .execute(pool)
        .await
        .expect("seed issue");

        MoveScope {
            slug,
            actor_full,
            actor_limited,
            source_project,
            target_project,
            target_identifier,
            issue_id,
        }
    }

    async fn call_op_move(
        pool: &sqlx::PgPool,
        scope: &MoveScope,
        actor: Uuid,
    ) -> Result<Outcome, Error> {
        let db = pidash_db::sea_orm_connection(pool);
        let ctx = WriteCtx::new(WriteActor {
            user_id: actor,
            timezone: Some("UTC".to_owned()),
        });
        let deps = Deps {
            pool: pool.clone(),
            orm: db,
            transition: Arc::new(RecordingTransition {
                comments: Mutex::new(Vec::new()),
                fires: Mutex::new(Vec::new()),
            }),
            enqueue: Arc::new(RecordingEnqueue {
                calls: Mutex::new(Vec::new()),
            }),
        };
        let target_ref = serde_json::json!(scope.target_identifier);
        let write = MoveWrite {
            slug: &scope.slug,
            project_id: scope.source_project,
            pk: scope.issue_id,
            target_ref: &target_ref,
            origin: None,
            web_base: None,
        };
        let drain: Arc<dyn domain_issues::MovePostCommitHook> = Arc::new(RecordingDrain);
        move_to_project(
            &deps,
            &ctx,
            &write,
            mcp_options(),
            drain,
            test_creation_deps(pool.clone()),
        )
        .await
    }

    /// The MCP options reach the kernel through the operation: the move
    /// succeeds for a member of both projects and the `updated_by` stamp
    /// lands on the row (NULL under `django()`) and in the render.
    #[tokio::test]
    async fn live_mcp_options_stamp_updated_by_through_operation() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let scope = seed_move_scope(&pool, "stamp").await;
        let outcome = call_op_move(&pool, &scope, scope.actor_full)
            .await
            .expect("mcp-options move succeeds");
        assert_eq!(outcome.status, 200);
        let row: (Option<Uuid>, Uuid) =
            sqlx::query_as("SELECT updated_by_id, project_id FROM issues WHERE id = $1")
                .bind(scope.issue_id)
                .fetch_one(&pool)
                .await
                .expect("moved row");
        assert_eq!(row.0, Some(scope.actor_full));
        assert_eq!(row.1, scope.target_project);
        let body: Value = serde_json::from_str(&outcome.body).expect("json body");
        assert_eq!(
            body.get("updated_by").and_then(Value::as_str),
            Some(scope.actor_full.to_string()).as_deref()
        );
    }

    /// The MCP options' `Dual` guards reach the kernel through the
    /// operation: without a source membership the kernel denies (a
    /// `TargetOnly` call would sail through) and the operation answers
    /// the 500 before writing anything.
    #[tokio::test]
    async fn live_mcp_dual_guards_deny_without_source_membership() {
        let Some(pool) = live_pool().await else {
            return;
        };
        let scope = seed_move_scope(&pool, "dual").await;
        let err = call_op_move(&pool, &scope, scope.actor_limited)
            .await
            .expect_err("dual denial");
        assert_eq!(err, Error::ServerError);
        let project: (Uuid,) = sqlx::query_as("SELECT project_id FROM issues WHERE id = $1")
            .bind(scope.issue_id)
            .fetch_one(&pool)
            .await
            .expect("unmoved row");
        assert_eq!(project.0, scope.source_project);
    }
}

#[cfg(test)]
mod suppression_tests {
    use super::*;

    #[test]
    fn suppressed_create_builds_no_model_activity_spec() {
        // The MCP options (`emit_model_activity: false`) with no origin:
        // the create pushes no webhook leg, so the chain records no
        // `Origin` stop and the operation answers 201 instead of 500.
        // Suppression wins even when an origin is configured.
        let requested = serde_json::json!({"name": "n"});
        for origin in [None, Some("https://app.example.test".to_owned())] {
            assert!(create_model_activity(
                Uuid::nil(),
                requested.clone(),
                Uuid::nil(),
                "ws".to_owned(),
                origin,
                false,
            )
            .is_none());
        }
    }

    #[test]
    fn emitting_create_forwards_origin_into_spec() {
        // The v1 options (`emit_model_activity: true`) build the spec
        // with the origin forwarded — including a `None` origin, which
        // the chain then stops on (`ChainStop::Origin`), exactly the
        // view's `base_host` raise past the save.
        let requested = serde_json::json!({"name": "n"});
        let model = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let spec = create_model_activity(
            model,
            requested.clone(),
            actor,
            "ws".to_owned(),
            Some("https://app.example.test".to_owned()),
            true,
        )
        .expect("spec");
        assert_eq!(spec.model, model);
        assert_eq!(spec.requested, requested);
        assert_eq!(spec.current, Value::Null);
        assert_eq!(spec.actor, actor);
        assert_eq!(spec.slug, "ws");
        assert_eq!(spec.origin.as_deref(), Some("https://app.example.test"));
        let spec = create_model_activity(model, requested, actor, "ws".to_owned(), None, true)
            .expect("spec");
        assert_eq!(spec.origin, None);
    }

    #[test]
    fn move_write_accepts_none_origin() {
        // The origin-less (`None`) move the MCP layer passes: the type
        // admits it, and the operation forwards it into
        // `MoveInput.origin` — the domain move then enqueues the
        // null-origin webhook, pinned at the leg by
        // `move_model_activity_leg_none_origin_enqueues_null`.
        let target = Value::Null;
        let write = MoveWrite {
            slug: "ws",
            project_id: Uuid::nil(),
            pk: Uuid::nil(),
            target_ref: &target,
            origin: None,
            web_base: None,
        };
        let forwarded: Option<String> = write.origin.clone();
        assert_eq!(forwarded, None);
    }
}
