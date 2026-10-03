#![forbid(unsafe_code)]

//! Scheduler + binding serializer shapes (D-36, stage 5).
//!
//! Ports `apps/api/pi_dash/app/serializers/scheduler.py:1-268` (drift
//! baseline `01a93e17`):
//! - `SchedulerSerializer` (`:74-112`): the 12-field list, the read-only
//!   set, `active_binding_count` (the annotated `_active_binding_count`
//!   when present, else a count query over non-deleted bindings — the
//!   query itself lives in the queries layer; this module resolves
//!   annotated vs counted), and color validation via `_validate_color`
//!   (`:37-43`).
//! - `SchedulerBindingSerializer` (`:114-268`): the 25-field list, the
//!   read-only set (11 Meta entries plus the 3 declared read-only derived
//!   fields = 14 effective), the derived reads (`scheduler_slug/name/color`,
//!   `last_run_status`, `last_run_ended_at`, `pod_name`), and every
//!   validator: `validate_rrule` (`:181-198`), `validate_rdates/exdates`
//!   via `_validate_iso_datetime_list` (`:46-71`), `validate_tzid`
//!   (`:206-216`), `validate_extra_context` (`:218-223`), and cross-field
//!   `validate` (`:225-268`: scheduler/project lock, rrule+dtstart
//!   re-check, pod-must-belong-to-project).
//!
//! Fixture oracles: F36-01
//! (`rust-api/fixtures/app_scheduler/serializers/scheduler_shapes.golden.json`),
//! F36-02 (`.../binding_shapes.golden.json`), F36-03
//! (`.../binding_validation.golden.json`); the unit tests below replay
//! those goldens byte-identically (field order, error strings, bodies).
//!
//! Seams (split-review fixes — `services` cannot depend on `jobs`):
//! - The rrule verdict arrives as an injected
//!   `rrule_validator: &dyn Fn(&str) -> Result<(), String>` closure; only
//!   the message string crosses the seam (all Python uses is `str(e)`).
//!   The bindings handlers pass a closure over
//!   `pidash_jobs::tasks_ticker::rrule::validate_rrule_string`. Python's
//!   `dtstart` anchor parameter is dropped: it only seeds the `dateutil`
//!   parser (`bgtasks/_rrule.py:218-245`) and the verdict checks never
//!   touch it, so field- and cross-field validation call the injected
//!   validator the same way.
//! - The pod-must-belong-to-project check takes the pod's `project_id` as
//!   a parameter (pure function); the queries layer supplies the row.
//!
//! Out of scope (owned by sibling layers): the active-binding count query
//! and every other read/write SQL statement (queries layer, F36-04..06);
//! the pod existence queryset check and the `(scheduler, project)`
//! unique-together check (both need the database; queries + handlers);
//! DRF field-level preemptions (`max_length`, `allow_blank=False`,
//! `allow_null=False`, required, choices, boolean parsing — the handlers
//! replicate those before calling these validators); HTTP envelopes and
//! routes (handlers, F36-10..12).
//!
//! Existing bugs ported as-is (translation, don't redesign):
//! 1. `validate_tzid`'s `(value or "UTC")` fallback and the `None → []`
//!    branch of `_validate_iso_datetime_list` are unreachable via HTTP
//!    (DRF blank/null checks preempt) — ported verbatim anyway.
//! 2. `ZoneInfo('')` (and `'.'`, `'..'`, absolute or non-normalized
//!    keys, and single-segment keys with a NUL byte) raises a bare
//!    `ValueError`, not `ZoneInfoNotFoundError`, so a direct
//!    `validate_tzid('   ')` escapes as a 500 — ported as the distinct
//!    [`TzidError::InvalidKey`] variant.
//! 3. CPython `fromisoformat` drops offset microseconds when HH=MM=SS=0
//!    (e.g. `+00:00:00.5` parses to a zero offset) — the parser below
//!    reproduces it; see [`normalize_iso_datetime`].
//! 4. CPython `fromisoformat` accepts `:` as a fraction separator
//!    (`T00:00:00:04` → `.040000`) and any single Unicode scalar as the
//!    date/time separator — reproduced, not sanitized.

use serde_json::Value;
use uuid::Uuid;

/// `SchedulerSerializer.Meta.fields`, in source order
/// (`app/serializers/scheduler.py:79-92`).
pub const SCHEDULER_SERIALIZER_FIELDS: &[&str] = &[
    "id",
    "workspace",
    "slug",
    "name",
    "description",
    "prompt",
    "color",
    "source",
    "is_enabled",
    "active_binding_count",
    "created_at",
    "updated_at",
];

/// `SchedulerSerializer.Meta.read_only_fields` (`scheduler.py:93-100`).
pub const SCHEDULER_READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "workspace",
    "source",
    "active_binding_count",
    "created_at",
    "updated_at",
];

/// `SchedulerBindingSerializer.Meta.fields`, in source order
/// (`app/serializers/scheduler.py:134-160`).
pub const BINDING_SERIALIZER_FIELDS: &[&str] = &[
    "id",
    "scheduler",
    "scheduler_slug",
    "scheduler_name",
    "scheduler_color",
    "project",
    "workspace",
    "dtstart",
    "tzid",
    "rrule",
    "rdates",
    "exdates",
    "extra_context",
    "enabled",
    "outcome_mode",
    "pod",
    "pod_name",
    "next_run_at",
    "last_run",
    "last_run_status",
    "last_run_ended_at",
    "last_error",
    "actor",
    "created_at",
    "updated_at",
];

/// `SchedulerBindingSerializer.Meta.read_only_fields` (`scheduler.py:161-173`).
pub const BINDING_META_READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "workspace",
    "pod_name",
    "next_run_at",
    "last_run",
    "last_run_status",
    "last_run_ended_at",
    "last_error",
    "actor",
    "created_at",
    "updated_at",
];

/// Derived fields declared `read_only=True` on the field itself
/// (`scheduler.py:115-117`): `scheduler_slug`, `scheduler_name`,
/// `scheduler_color`. Together with [`BINDING_META_READ_ONLY_FIELDS`]
/// these form the effective 14-key read-only set (verified by
/// introspection; see F36-02 `read_only_note`).
pub const BINDING_DECLARED_READ_ONLY_FIELDS: &[&str] =
    &["scheduler_slug", "scheduler_name", "scheduler_color"];

/// The effective binding read-only set (14 keys): the Meta list first,
/// then the 3 declared derived fields. Handlers use this for membership
/// (keys sent by the client are silently ignored).
pub fn binding_read_only_fields() -> Vec<&'static str> {
    let mut fields = BINDING_META_READ_ONLY_FIELDS.to_vec();
    fields.extend_from_slice(BINDING_DECLARED_READ_ONLY_FIELDS);
    fields
}

/// `EXTRA_CONTEXT_MAX_LENGTH = 16 * 1024` (`scheduler.py:27`).
pub const EXTRA_CONTEXT_MAX_LENGTH: usize = 16 * 1024;

/// `RDATE_EXDATE_MAX_LENGTH = 256` (`scheduler.py:32`).
pub const RDATE_EXDATE_MAX_LENGTH: usize = 256;

/// `_validate_color` rejection (`scheduler.py:40-42`).
pub const COLOR_ERROR: &str = "color must be a 7-character hex string like '#3b82f6'";

/// Cross-field pod/project mismatch (`scheduler.py:264-267`).
pub const POD_PROJECT_ERROR: &str = "pod must belong to the same project as this scheduler install";

/// Cross-field scheduler/project lock message (`scheduler.py:232-234`):
/// `"{locked} cannot be changed; uninstall and re-install"`.
pub fn lock_error(field: &str) -> String {
    format!("{field} cannot be changed; uninstall and re-install")
}

/// `validate_extra_context` rejection (`scheduler.py:220-222`).
pub fn extra_context_error() -> String {
    format!("extra_context must be at most {EXTRA_CONTEXT_MAX_LENGTH} characters")
}

/// `validate_tzid` rejection (`scheduler.py:213-215`): the value is the
/// *stripped* input rendered with Python `repr` semantics.
pub fn tzid_unknown_error(stripped: &str) -> String {
    format!(
        "tzid {} is not a recognized IANA timezone",
        py_repr_str(stripped)
    )
}

/// `_validate_iso_datetime_list` non-list rejection (`scheduler.py:51-53`).
pub const ISO_LIST_NOT_ARRAY: &str = "must be a JSON array of ISO 8601 datetime strings";

/// `_validate_iso_datetime_list` over-long rejection (`scheduler.py:54-57`).
pub fn iso_list_too_long() -> String {
    format!("must contain at most {RDATE_EXDATE_MAX_LENGTH} entries")
}

/// `_validate_iso_datetime_list` non-string item (`scheduler.py:60-63`).
pub fn iso_item_type_error(index: usize, type_name: &str) -> String {
    format!("item {index} must be a string, got {type_name}")
}

/// `_validate_iso_datetime_list` unparsable item (`scheduler.py:66-69`).
pub fn iso_item_parse_error(index: usize, detail: &str) -> String {
    format!("item {index} is not a valid ISO 8601 datetime: {detail}")
}

/// Python `str.strip()` with no arguments: strip Unicode whitespace.
/// Exactly `char::is_whitespace` plus U+001C..U+001F (FS/GS/RS/US), which
/// Python strips but Rust's `trim()` does not (verified by an exhaustive
/// BMP probe of `str.strip` against the `White_Space` property: those 4
/// are the only differences; no `White_Space` character exists above the
/// BMP).
fn py_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// `_validate_color` (`scheduler.py:37-43`): strip, lowercase, then
/// `^#[0-9a-fA-F]{6}$`.
///
/// The `$` anchor is exact here (not before-trailing-newline) because the
/// value was stripped first, so a length + ASCII-hex check is equivalent
/// to the regex.
pub fn validate_color(value: &str) -> Result<String, &'static str> {
    let canonical = py_strip(value).to_lowercase();
    let bytes = canonical.as_bytes();
    let valid =
        bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit());
    if valid {
        Ok(canonical)
    } else {
        Err(COLOR_ERROR)
    }
}

/// Python `type(value).__name__` over a JSON value: `None` is `NoneType`,
/// `true` is `bool`, and integer / floating-point numbers are
/// `int` / `float`.
///
/// Edge: integers beyond the `u64` range parse as `f64` under
/// `serde_json` (without `arbitrary_precision`), so they report `float`
/// where Python's unbounded `int` reports `int`. No contract input
/// exercises that.
pub fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// A `_validate_iso_datetime_list` rejection: the field name plus the
/// message. On the wire the field validator raises
/// `ValidationError({field: message})` and DRF's `as_serializer_error`
/// passes the nested dict through, producing
/// `{field: {field: message}}` (see [`nested_field_error_body`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoListError {
    pub field: String,
    pub message: String,
}

/// `_validate_iso_datetime_list` (`scheduler.py:46-71`), shared by
/// `validate_rdates` / `validate_exdates` (`:200-204`).
///
/// `None → []` (`:48-49`) is unreachable via HTTP (`JSONField` with
/// `allow_null=False` rejects null first) and is ported verbatim; the
/// handlers replicate the null check before calling this.
pub fn validate_iso_datetime_list(
    values: &Value,
    field: &str,
) -> Result<Vec<String>, IsoListError> {
    if values.is_null() {
        return Ok(Vec::new());
    }
    let items = match values.as_array() {
        Some(items) => items,
        None => {
            return Err(IsoListError {
                field: field.to_owned(),
                message: ISO_LIST_NOT_ARRAY.to_owned(),
            });
        }
    };
    if items.len() > RDATE_EXDATE_MAX_LENGTH {
        return Err(IsoListError {
            field: field.to_owned(),
            message: iso_list_too_long(),
        });
    }
    let mut out = Vec::with_capacity(items.len());
    for (index, raw) in items.iter().enumerate() {
        let text = match raw.as_str() {
            Some(text) => text,
            None => {
                return Err(IsoListError {
                    field: field.to_owned(),
                    message: iso_item_type_error(index, json_type_name(raw)),
                });
            }
        };
        match normalize_iso_datetime(text) {
            Ok(canonical) => out.push(canonical),
            Err(detail) => {
                return Err(IsoListError {
                    field: field.to_owned(),
                    message: iso_item_parse_error(index, &detail),
                });
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// ISO 8601: `datetime.fromisoformat(raw.replace("Z", "+00:00")).isoformat()`
//
// A statement-by-statement port of CPython 3.12's `_datetimemodule.c`
// (`datetime_fromisoformat`, `_find_isoformat_datetime_separator`,
// `parse_isoformat_date`, `parse_isoformat_time`, `parse_hh_mm_ss_ff`,
// `tzinfo_from_isoformat_results`), verified against the live interpreter
// (probe rounds plus a differential fuzz corpus). Mapping notes:
//
// - C reads bytes with no bounds checks (the NUL terminator stands past
//   the end); `byte_at` returns `0` past the end, which is exactly that.
//   Rust `str` inputs are valid UTF-8, so the lone-surrogate sanitize
//   step (`_sanitize_isoformat_str`) is a no-op here.
// - Every `Z` becomes `+00:00` first (all occurrences, like
//   `str.replace`), so the time parser never sees `Z` in practice; the
//   `Z` branch is still ported for fidelity.
// - The `Invalid isoformat string: {s!r}` echo shows the *replaced* string.
// - Error precedence falls out of the C call order: date syntax, then
//   time syntax, then the offset-range error, then date-range errors
//   (`year`/`month`/`day`), then time-range errors
//   (`hour`/`minute`/`second`).
//
// The surprising-but-true behaviors below are all direct consequences of
// the C loop shapes, not special cases here:
//
// - After each time component the loop consumes one byte; when the part
//   ends there that byte is ignored — any single byte (even a digit or
//   `:`) is tolerated before a timezone sign (`T00:00:00X+05:00`), while
//   two bytes are not (`T00XX+05:00` fails).
// - A fourth colon group after an extended `SS` falls out of the
//   component loop into the fraction parser, so `:DD` reads as fraction
//   digits with no separator (`T00:00:00:25`); `:` directly before a
//   timezone sign takes the one-byte rule instead.
// - The fraction parser requires all its digits when fewer than 6
//   follow, but once 6+ follow it parses 6 and never examines the rest:
//   the leftovers are skipped when digits and simply unexamined
//   otherwise, so they pass iff a timezone follows
//   (`.555555b+05:00` passes, `.555555b` fails, `.5b+05:00` fails).
// - Basic format runs straight into the same fraction parser, which is
//   why 2+ bare digits after `SS` read as a fraction while exactly 1
//   takes the one-byte rule (tolerated only before a timezone).
// - The date/time split for week dates lives in the separator finder:
//   `YYYY-Www-` with a digit at index 10 splits at 8 (the `-`/digit
//   starts separator+time), length 9 is outright rejected, and basic
//   `YYYYWww[D]` splits on digit-run parity.
// - Offset components are *not* individually range-checked (`+05:60`
//   normalizes to `+06:00`); the total must be strictly within ±24h.
// - An all-zero offset HH:MM:SS returns UTC without looking at the
//   (already parsed and validated) microseconds (`+00:00:00.5` is `+00:00`).
// - Year-0 week dates convert through C's `ord_to_ymd` below ordinal 1
//   and always surface `month must be in 1..12` (exhaustively verified);
//   overflowing week dates surface `year 10000 is out of range`.
// - Rendering: `YYYY-MM-DDTHH:MM:SS[.ffffff][±HH:MM[:SS[.ffffff]]]`;
//   seconds render only when nonzero (seconds or microseconds), the
//   fraction only when microseconds are nonzero, and a zero offset
//   renders `+00:00` even when written with `-`.

/// Microseconds per unit (offset arithmetic stays in `i64`; the largest
/// representable offset, ±99:99:99, is ~3.6e11 µs).
const US_PER_SECOND: i64 = 1_000_000;
const US_PER_DAY: i64 = 86_400 * US_PER_SECOND;

/// Proleptic Gregorian ordinal of 9999-12-31 (`datetime.max`).
const MAX_ORDINAL: i64 = 3_652_059;

/// Internal parse failure: syntax vs range. Both render as the `ValueError`
/// text Python's `fromisoformat` raises.
#[derive(Debug, Clone, PartialEq, Eq)]
enum IsoFail {
    /// `Invalid isoformat string: {replaced!r}`.
    Invalid,
    /// A range message verbatim (`month must be in 1..12`, the offset
    /// message, ...).
    Range(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IsoDate {
    year: i32,
    month: u32,
    day: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct IsoTime {
    hour: u32,
    minute: u32,
    second: u32,
    microsecond: u32,
}

/// The `datetime.fromisoformat(raw.replace("Z", "+00:00"))` half:
/// `Ok` carries the normalized `.isoformat()` rendering, `Err` the exact
/// `ValueError` text.
pub fn normalize_iso_datetime(raw: &str) -> Result<String, String> {
    // Like Python's `str.replace`: every `Z` occurrence, not just a suffix.
    let replaced = raw.replace('Z', "+00:00");
    match parse_iso_datetime(&replaced) {
        Ok((date, time, offset_us)) => Ok(render_iso_datetime(date, time, offset_us)),
        Err(IsoFail::Invalid) => Err(format!(
            "Invalid isoformat string: {}",
            py_repr_str(&replaced)
        )),
        Err(IsoFail::Range(message)) => Err(message),
    }
}

/// `datetime_fromisoformat`: split at the separator, parse date, then time,
/// then range-check in construction order (offset, date, time).
fn parse_iso_datetime(text: &str) -> Result<(IsoDate, IsoTime, Option<i64>), IsoFail> {
    let bytes = text.as_bytes();
    let sep = find_isoformat_datetime_separator(bytes);
    let date_raw = parse_isoformat_date(bytes, sep)?;
    // A valid date consumed ASCII bytes `[0, sep)`, so `sep` is a char
    // boundary; `.get` keeps hostile input from panicking regardless.
    let (time, offset_raw) = if sep >= 0 {
        #[allow(clippy::cast_sign_loss)]
        let sep = sep as usize;
        if bytes.len() > sep {
            let rest = text.get(sep..).ok_or(IsoFail::Invalid)?;
            let skip = rest.chars().next().map_or(0, char::len_utf8);
            let time_text = text.get(sep + skip..).ok_or(IsoFail::Invalid)?;
            parse_isoformat_time(time_text)?
        } else {
            (IsoTime::default(), None)
        }
    } else {
        // Unreachable: the date parse rejects `sep < 0` above.
        (IsoTime::default(), None)
    };
    // Range checks in construction order: offset, then date, then time.
    let offset_us = match offset_raw {
        None => None,
        Some(raw) => Some(check_offset_range(raw)?),
    };
    let date = check_date_range(date_raw)?;
    check_time_range(time)?;
    Ok((date, time, offset_us))
}

/// Raw date parse: calendar `(year, month, day)` or ISO week
/// `(iso_year, week, weekday)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawDate {
    Calendar {
        year: i32,
        month: u32,
        day: u32,
    },
    Week {
        iso_year: i32,
        week: u32,
        weekday: u32,
    },
}

/// Byte at `i`, or `0` past the end (the C reads the NUL terminator).
fn byte_at(bytes: &[u8], i: usize) -> u8 {
    bytes.get(i).copied().unwrap_or(0)
}

/// `parse_digits`: exactly `n` ASCII digits at `p` accumulate from 0;
/// anything else (including past-the-end) fails.
fn take_n_digits(bytes: &[u8], mut p: usize, n: usize) -> Option<(u32, usize)> {
    let mut value: u32 = 0;
    for _ in 0..n {
        let digit = bytes.get(p)?;
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(digit - b'0');
        p += 1;
    }
    Some((value, p))
}

/// `_find_isoformat_datetime_separator`: byte offset of the date/time
/// separator, or `-1`. Past-the-end reads yield `0` (the C reads the NUL
/// terminator there; below length 4 it reads past it, but the date parse
/// fails on the year digits first either way, so the outcome matches).
fn find_isoformat_datetime_separator(bytes: &[u8]) -> isize {
    let len = bytes.len();
    if len == 7 {
        return 7;
    }
    if byte_at(bytes, 4) == b'-' {
        // YYYY-???
        if byte_at(bytes, 5) == b'W' {
            // YYYY-W??
            if len < 8 {
                return -1;
            }
            if len > 8 && byte_at(bytes, 8) == b'-' {
                // YYYY-Www-D (10) or YYYY-Www-HH (8).
                if len == 9 {
                    return -1;
                }
                if len > 10 && byte_at(bytes, 10).is_ascii_digit() {
                    return 8;
                }
                return 10;
            }
            // YYYY-Www (8).
            return 8;
        }
        // YYYY-MM-DD (10).
        return 10;
    }
    if byte_at(bytes, 4) == b'W' {
        // YYYYWww (7) or YYYYWwwd (8), split on digit-run parity.
        let mut idx: usize = 7;
        while idx < len && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx < 9 {
            #[allow(clippy::cast_possible_wrap)]
            return idx as isize;
        }
        if idx.is_multiple_of(2) {
            return 7;
        }
        return 8;
    }
    // YYYYMMDD (8).
    8
}

/// `parse_isoformat_date` over `bytes[0..date_len]`, plus the `iso_to_ymd`
/// range verdicts (week 0/53+/day 0/8+ are syntax errors in C too).
fn parse_isoformat_date(bytes: &[u8], date_len: isize) -> Result<RawDate, IsoFail> {
    // A `-1` separator always fails the week/weekday digit reads on the
    // NUL terminator in C; fail directly.
    if date_len < 0 {
        return Err(IsoFail::Invalid);
    }
    #[allow(clippy::cast_sign_loss)]
    let date_len = date_len as usize;
    let (year, mut p) = take_n_digits(bytes, 0, 4).ok_or(IsoFail::Invalid)?;
    #[allow(clippy::cast_possible_truncation)]
    let year = year as i32;
    let uses_separator = byte_at(bytes, p) == b'-';
    if uses_separator {
        p += 1;
    }
    if byte_at(bytes, p) == b'W' {
        p += 1;
        let (week, next) = take_n_digits(bytes, p, 2).ok_or(IsoFail::Invalid)?;
        p = next;
        let weekday = if p < date_len {
            if uses_separator {
                if byte_at(bytes, p) != b'-' {
                    return Err(IsoFail::Invalid);
                }
                p += 1;
            }
            let (day, _) = take_n_digits(bytes, p, 1).ok_or(IsoFail::Invalid)?;
            day
        } else {
            1
        };
        if week == 0 || week > weeks_in_iso_year(year) {
            return Err(IsoFail::Invalid);
        }
        if weekday == 0 || weekday > 7 {
            return Err(IsoFail::Invalid);
        }
        Ok(RawDate::Week {
            iso_year: year,
            week,
            weekday,
        })
    } else {
        let (month, next) = take_n_digits(bytes, p, 2).ok_or(IsoFail::Invalid)?;
        p = next;
        if uses_separator {
            if byte_at(bytes, p) != b'-' {
                return Err(IsoFail::Invalid);
            }
            p += 1;
        }
        let (day, _) = take_n_digits(bytes, p, 2).ok_or(IsoFail::Invalid)?;
        Ok(RawDate::Calendar { year, month, day })
    }
}

/// Raw offset parse: sign plus `HH[:MM[:SS]][.ffffff]` components (the
/// components are *not* individually range-checked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawOffset {
    negative: bool,
    hours: i64,
    minutes: i64,
    seconds: i64,
    micros: i64,
}

/// `parse_hh_mm_ss_ff` over `bytes[start..end]`: the three 2-digit
/// components plus the optional fraction. Returns the components and
/// whether bytes remain (`rv` in C: `false` = clean end). Reads at `end`
/// see the byte there (a timezone sign or the string end, like C).
fn parse_hh_mm_ss_ff(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Result<([u32; 3], u32, bool), IsoFail> {
    let mut vals = [0u32; 3];
    let mut p = start;
    let mut has_separator = true;
    for (i, slot) in vals.iter_mut().enumerate() {
        let (value, next) = take_n_digits(bytes, p, 2).ok_or(IsoFail::Invalid)?;
        *slot = value;
        p = next;
        let c = byte_at(bytes, p);
        p += 1;
        if i == 0 {
            has_separator = c == b':';
        }
        if p >= end {
            return Ok((vals, 0, c != 0));
        } else if has_separator && c == b':' {
            continue;
        } else if c == b'.' || c == b',' {
            break;
        } else if !has_separator {
            p -= 1;
        } else {
            return Err(IsoFail::Invalid);
        }
    }
    // Fraction: `min(remaining, 6)` digits are required, further digits
    // skipped, and anything else left unexamined for the caller. At least
    // one byte always remains here (`p < end` on every path that reaches
    // this code, as in C).
    let to_parse = (end - p).min(6);
    let (mut microsecond, next) = take_n_digits(bytes, p, to_parse).ok_or(IsoFail::Invalid)?;
    p = next;
    static CORRECTION: [u32; 5] = [100_000, 10_000, 1_000, 100, 10];
    if to_parse < 6 {
        debug_assert!(to_parse >= 1);
        if to_parse >= 1 {
            microsecond *= CORRECTION[to_parse - 1];
        }
    }
    while p < end && bytes[p].is_ascii_digit() {
        p += 1;
    }
    Ok((vals, microsecond, byte_at(bytes, p) != 0))
}

/// `parse_isoformat_time`: scan for the first `Z`/`+`/`-`, parse the time
/// before it and the offset after it.
fn parse_isoformat_time(text: &str) -> Result<(IsoTime, Option<RawOffset>), IsoFail> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut tz_at = None;
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'Z' || *byte == b'+' || *byte == b'-' {
            tz_at = Some(i);
            break;
        }
    }
    let part_end = tz_at.unwrap_or(len);
    let ([hour, minute, second], microsecond, more) = parse_hh_mm_ss_ff(bytes, 0, part_end)?;
    let time = IsoTime {
        hour,
        minute,
        second,
        microsecond,
    };
    match tz_at {
        None => {
            if more {
                return Err(IsoFail::Invalid);
            }
            Ok((time, None))
        }
        Some(t) if bytes[t] == b'Z' => {
            if t + 1 != len {
                return Err(IsoFail::Invalid);
            }
            Ok((
                time,
                Some(RawOffset {
                    negative: false,
                    hours: 0,
                    minutes: 0,
                    seconds: 0,
                    micros: 0,
                }),
            ))
        }
        Some(t) => {
            let negative = bytes[t] == b'-';
            let ([hours, minutes, seconds], micros, more) = parse_hh_mm_ss_ff(bytes, t + 1, len)?;
            if more {
                return Err(IsoFail::Invalid);
            }
            Ok((
                time,
                Some(RawOffset {
                    negative,
                    hours: i64::from(hours),
                    minutes: i64::from(minutes),
                    seconds: i64::from(seconds),
                    micros: i64::from(micros),
                }),
            ))
        }
    }
}

/// Offset construction: the ±24h bound (with the `timedelta` repr Python
/// embeds), then the all-zero fast-path quirk that drops parsed
/// microseconds.
fn check_offset_range(raw: RawOffset) -> Result<i64, IsoFail> {
    let RawOffset {
        negative,
        hours,
        minutes,
        seconds,
        micros,
    } = raw;
    let total = (hours * 3600 + minutes * 60 + seconds) * US_PER_SECOND + micros;
    let signed = if negative { -total } else { total };
    if signed.abs() >= US_PER_DAY {
        return Err(IsoFail::Range(format!(
            "offset must be a timedelta strictly between -timedelta(hours=24) \
             and timedelta(hours=24), not {}.",
            format_timedelta(signed)
        )));
    }
    // CPython fast-path quirk: an all-zero HH:MM:SS returns UTC without
    // looking at the (already parsed and validated) microseconds.
    if hours == 0 && minutes == 0 && seconds == 0 {
        return Ok(0);
    }
    Ok(signed)
}

/// `repr(timedelta)` for a microsecond total: days via floored division,
/// then non-negative seconds/microseconds remainders; only nonzero
/// components render.
fn format_timedelta(total_us: i64) -> String {
    let days = total_us.div_euclid(US_PER_DAY);
    let rem = total_us.rem_euclid(US_PER_DAY);
    let seconds = rem / US_PER_SECOND;
    let micros = rem % US_PER_SECOND;
    let mut out = format!("datetime.timedelta(days={days}");
    if seconds != 0 {
        out.push_str(&format!(", seconds={seconds}"));
    }
    if micros != 0 {
        out.push_str(&format!(", microseconds={micros}"));
    }
    out.push(')');
    out
}

/// Date construction: calendar year/month/day order, then the week-date
/// ordinal bounds.
fn check_date_range(raw: RawDate) -> Result<IsoDate, IsoFail> {
    match raw {
        RawDate::Calendar { year, month, day } => {
            if year == 0 {
                return Err(IsoFail::Range("year 0 is out of range".to_owned()));
            }
            if !(1..=12).contains(&month) {
                return Err(IsoFail::Range("month must be in 1..12".to_owned()));
            }
            if day == 0 || day > days_in_month(year, month) {
                return Err(IsoFail::Range("day is out of range for month".to_owned()));
            }
            Ok(IsoDate { year, month, day })
        }
        RawDate::Week {
            iso_year,
            week,
            weekday,
        } => {
            let ordinal = iso_to_ordinal(iso_year, week, weekday);
            // Year 0 resolves to ordinal <= 0 and surfaces CPython's
            // `month must be in 1..12` (the conversion yields a month of 0);
            // past-the-end resolves past `datetime.max`.
            if ordinal < 1 {
                return Err(IsoFail::Range("month must be in 1..12".to_owned()));
            }
            if ordinal > MAX_ORDINAL {
                let (year, _, _) = ordinal_to_ymd(ordinal);
                return Err(IsoFail::Range(format!("year {year} is out of range")));
            }
            let (year, month, day) = ordinal_to_ymd(ordinal);
            Ok(IsoDate { year, month, day })
        }
    }
}

/// Time construction: hour, then minute, then second.
fn check_time_range(time: IsoTime) -> Result<(), IsoFail> {
    if time.hour > 23 {
        return Err(IsoFail::Range("hour must be in 0..23".to_owned()));
    }
    if time.minute > 59 {
        return Err(IsoFail::Range("minute must be in 0..59".to_owned()));
    }
    if time.second > 59 {
        return Err(IsoFail::Range("second must be in 0..59".to_owned()));
    }
    Ok(())
}

/// `datetime.isoformat()`: `YYYY-MM-DDTHH:MM:SS[.ffffff][offset]`.
fn render_iso_datetime(date: IsoDate, time: IsoTime, offset_us: Option<i64>) -> String {
    let mut out = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        date.year, date.month, date.day, time.hour, time.minute, time.second
    );
    if time.microsecond != 0 {
        out.push_str(&format!(".{:06}", time.microsecond));
    }
    if let Some(total) = offset_us {
        let sign = if total < 0 { '-' } else { '+' };
        let mag = total.unsigned_abs();
        let hours = mag / 3_600_000_000;
        let minutes = mag / 60_000_000 % 60;
        let seconds = mag / 1_000_000 % 60;
        let micros = mag % 1_000_000;
        out.push(sign);
        out.push_str(&format!("{hours:02}:{minutes:02}"));
        if seconds != 0 || micros != 0 {
            out.push_str(&format!(":{seconds:02}"));
        }
        if micros != 0 {
            out.push_str(&format!(".{micros:06}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Proleptic Gregorian calendar (hand-rolled: the week-date path needs year 0
// and below-minimum ordinals, which `chrono::NaiveDate` does not model).

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Days before January 1 of `year` (proleptic; 0001-01-01 is ordinal 1).
/// Floored division keeps year 0 exact (`div_euclid`: -1/4 = -1).
fn days_before_year(year: i32) -> i64 {
    let y = i64::from(year) - 1;
    365 * y + y.div_euclid(4) - y.div_euclid(100) + y.div_euclid(400)
}

fn ordinal_from_ymd(year: i32, month: u32, day: u32) -> i64 {
    let mut ordinal = days_before_year(year) + i64::from(day);
    for m in 1..month {
        ordinal += i64::from(days_in_month(year, m));
    }
    ordinal
}

/// Weekday of an ordinal, Monday = 0 (always valid via `rem_euclid`,
/// even for ordinals <= 0 from year-0 week dates).
fn weekday_of_ordinal(ordinal: i64) -> i64 {
    (ordinal - 1).rem_euclid(7)
}

/// ISO weeks in `iso_year`: 53 when January 1 is a Thursday, or (leap
/// year and) a Wednesday.
fn weeks_in_iso_year(iso_year: i32) -> u32 {
    let jan1 = weekday_of_ordinal(ordinal_from_ymd(iso_year, 1, 1));
    if jan1 == 3 || (is_leap_year(iso_year) && jan1 == 2) {
        53
    } else {
        52
    }
}

/// ISO week date → proleptic ordinal (may be <= 0 for year 0, or past
/// `MAX_ORDINAL` at the top end; the caller bounds-checks).
fn iso_to_ordinal(iso_year: i32, week: u32, weekday: u32) -> i64 {
    let jan4 = ordinal_from_ymd(iso_year, 1, 4);
    let week1_monday = jan4 - weekday_of_ordinal(jan4);
    week1_monday + i64::from(week - 1) * 7 + i64::from(weekday - 1)
}

/// Proleptic ordinal (≥ 1) → `(year, month, day)` (Hinnant's
/// `civil_from_days`; days are counted from 0001-01-01 = 1).
fn ordinal_to_ymd(ordinal: i64) -> (i32, u32, u32) {
    let z = ordinal - 719_163 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    #[allow(clippy::cast_possible_truncation)]
    let year = (if m <= 2 { y + 1 } else { y }) as i32;
    #[allow(clippy::cast_possible_truncation)]
    let (month, day) = (m as u32, d as u32);
    (year, month, day)
}

// ---------------------------------------------------------------------------
// Binding field validators.

/// `validate_rrule` (`scheduler.py:181-198`): strip; empty means
/// single-shot (the validator is a no-op for it); otherwise strip one
/// leading `RRULE:` (case-insensitive test, first 6 characters removed)
/// and take the verdict from the injected `rrule_validator` closure.
///
/// Only the message string crosses the seam (all Python uses is `str(e)`);
/// field-level failures render as `{"rrule": [message]}`.
///
/// The byte-prefix test is exactly Python's
/// `value.upper().startswith("RRULE:")`: no non-ASCII character anywhere
/// in Unicode uppercases to a string starting with `R`, `U`, `L` or `E`
/// (exhaustively verified), so a match implies 6 ASCII bytes and the
/// 6-byte removal is exactly `value[len("RRULE:"):]`.
///
/// Note the `"RRULE:"`-only input strips to `""` and Python still calls
/// `validate_rrule_string("")` (a no-op) — this calls the closure too.
pub fn validate_rrule(
    value: &str,
    rrule_validator: &dyn Fn(&str) -> Result<(), String>,
) -> Result<String, String> {
    let stripped = py_strip(value);
    if stripped.is_empty() {
        return Ok(String::new());
    }
    let canonical = match stripped.get(.."RRULE:".len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case("RRULE:") => &stripped["RRULE:".len()..],
        _ => stripped,
    };
    rrule_validator(canonical)?;
    Ok(canonical.to_owned())
}

/// `validate_tzid` failure: `Unknown` carries the 400 message
/// (`tzid {value!r} is not a recognized IANA timezone`); `InvalidKey`
/// carries the bare-`ValueError` text CPython raises for empty, absolute,
/// non-normalized or escaping `ZoneInfo` keys — uncaught in Python, i.e.
/// a 500. Both are unreachable-via-HTTP refinements the handlers map to
/// their respective statuses (`""` / whitespace / null never reach field
/// validation through DRF's `CharField` checks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TzidError {
    Unknown(String),
    InvalidKey(String),
}

/// `validate_tzid` (`scheduler.py:206-216`): the `(value or "UTC")`
/// fallback, strip, exact-`UTC` short-circuit, else a `ZoneInfo`-semantics
/// membership check.
///
/// Membership is `chrono_tz::Tz` parsing (case-sensitive IANA names, like
/// `ZoneInfo`): the deterministic approximation of "present in the system
/// tzdata". Known boundary: exotic keys that exist in a tzdata install but
/// are not `chrono-tz` variants (`localtime`, `Factory`, `posix/...`,
/// `right/...`), non-zone root files that make `ZoneInfo` raise
/// `ValueError` (`zone.tab`, ...), and NUL bytes in multi-segment keys
/// (whose verdict depends on which parent directories exist) diverge —
/// the former 400 here where the system copy may accept, the rest 400
/// here where Python 500s. No contract input exercises those.
pub fn validate_tzid(value: &str) -> Result<String, TzidError> {
    if value.is_empty() {
        return Ok("UTC".to_owned());
    }
    let stripped = py_strip(value);
    if stripped == "UTC" {
        return Ok("UTC".to_owned());
    }
    if let Some(key_error) = zoneinfo_key_error(stripped) {
        return Err(TzidError::InvalidKey(key_error));
    }
    match stripped.parse::<chrono_tz::Tz>() {
        Ok(_) => Ok(stripped.to_owned()),
        Err(_) => Err(TzidError::Unknown(tzid_unknown_error(stripped))),
    }
}

/// The `ZoneInfo(key)` key-shape gate (CPython `Modules/_zoneinfo.c`):
/// absolute keys, keys that are not normalized relative paths, and keys
/// escaping `TZPATH` raise bare `ValueError`. Returns the exact CPython
/// message when the key is malformed, `None` when it may be looked up
/// (found or not).
fn zoneinfo_key_error(key: &str) -> Option<String> {
    if key.starts_with('/') {
        return Some(format!(
            "ZoneInfo keys may not be absolute paths, got: {key}"
        ));
    }
    // POSIX `normpath`: drop `.` / empty segments, resolve `..` against a
    // stack, keep leading `..` when the stack is empty.
    let mut stack: Vec<&str> = Vec::new();
    for segment in key.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                // `..` pops a real segment only; an empty stack or a
                // leading `..` keeps it (POSIX `normpath` semantics —
                // `normpath('../..')` is `'../..'`, not `'.'`).
                if stack.last().is_some_and(|top| *top != "..") {
                    stack.pop();
                } else {
                    stack.push("..");
                }
            }
            name => stack.push(name),
        }
    }
    let mut normalized = stack.join("/");
    if normalized.is_empty() {
        normalized.push('.');
    }
    if normalized != key {
        return Some(format!(
            "ZoneInfo keys must be normalized relative paths, got: {key}"
        ));
    }
    if normalized == "." || normalized == ".." || normalized.starts_with("../") {
        return Some(format!(
            "ZoneInfo keys must refer to subdirectories of TZPATH, got: {key}"
        ));
    }
    // A NUL byte in a single-segment key always fails `open()` argument
    // parsing (`ValueError: embedded null byte`) before any filesystem
    // access. In multi-segment keys the outcome depends on whether the
    // parent directories exist (a missing parent surfaces NZNF first —
    // e.g. `a/\x00` misses where `America/\x00` NUL-errors), so only the
    // single-segment shape is deterministic enough to model; the rest
    // falls through to the membership check below.
    if !key.contains('/') && key.contains('\x00') {
        return Some("embedded null byte".to_owned());
    }
    None
}

/// `validate_extra_context` (`scheduler.py:218-223`): the 16 KiB cap.
/// Python's `len()` counts code points, so this counts `chars`, not bytes;
/// the value passes through unchanged.
pub fn validate_extra_context(value: &str) -> Result<(), String> {
    if !value.is_empty() && value.chars().count() > EXTRA_CONTEXT_MAX_LENGTH {
        return Err(extra_context_error());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Binding cross-field `validate` (`scheduler.py:225-268`).

/// The scheduler/project update lock (`scheduler.py:229-234`): on update
/// (`instance is not None`, i.e. the handlers call this only for PATCH),
/// a supplied value differing from the instance value is rejected.
/// `incoming = None` means the key was absent (DRF partial semantics).
pub fn validate_locked_field(
    field: &str,
    incoming: Option<Uuid>,
    current: Uuid,
) -> Result<(), String> {
    if incoming.is_none_or(|id| id == current) {
        Ok(())
    } else {
        Err(lock_error(field))
    }
}

/// The cross-field rrule+dtstart re-check (`scheduler.py:235-249`):
/// resolve attrs-first, instance-second (default `""`), and re-validate a
/// non-empty rrule. The `dtstart` resolution exists in Python only as the
/// validator's anchor argument, which cannot change the verdict
/// (`bgtasks/_rrule.py:218-245`: the anchor only seeds the `dateutil`
/// parser), so the injected validator is called exactly as in
/// field-level validation — no `dtstart` parameter.
///
/// Failures render as `{"rrule": [message]}`.
pub fn validate_cross_rrule(
    attrs_rrule: Option<&str>,
    instance_rrule: Option<&str>,
    rrule_validator: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let resolved = attrs_rrule.or(instance_rrule).unwrap_or("");
    if resolved.is_empty() {
        return Ok(());
    }
    rrule_validator(resolved)
}

/// The pod-must-belong-to-project check (`scheduler.py:250-267`).
/// `pod_project_id = None` means the pod key was absent or null (skipped).
/// Otherwise the authoritative project resolves instance → view context →
/// body (first non-null wins); a known project that differs from the
/// pod's project is rejected. The pod's own existence check (the
/// `deleted_at IS NULL` queryset) lives in the queries layer, which
/// supplies the row this reads.
pub fn validate_pod_project(
    pod_project_id: Option<Uuid>,
    instance_project_id: Option<Uuid>,
    context_project_id: Option<Uuid>,
    body_project_id: Option<Uuid>,
) -> Result<(), &'static str> {
    let pod_project = match pod_project_id {
        None => return Ok(()),
        Some(id) => id,
    };
    let project_id = instance_project_id
        .or(context_project_id)
        .or(body_project_id);
    match project_id {
        None => Ok(()),
        Some(id) if id == pod_project => Ok(()),
        Some(_) => Err(POD_PROJECT_ERROR),
    }
}

// ---------------------------------------------------------------------------
// Read-path derivations.

/// `get_active_binding_count` (`scheduler.py:102-108`): the annotated
/// `_active_binding_count` when present (including 0 — `is not None`),
/// else the queries layer's count over non-deleted bindings, passed in.
pub fn resolve_active_binding_count(annotated: Option<i64>, fallback_count: i64) -> i64 {
    annotated.unwrap_or(fallback_count)
}

/// `get_last_run_status` (`scheduler.py:175-176`):
/// `obj.last_run.status if obj.last_run_id else None`.
pub fn last_run_status(last_run_id: Option<Uuid>, status: &str) -> Option<&str> {
    last_run_id.map(|_| status)
}

/// `get_last_run_ended_at` (`scheduler.py:178-179`):
/// `obj.last_run.ended_at if obj.last_run_id else None`. The caller
/// passes the F-07-rendered datetime (or the raw value — the gate is
/// identical either way).
pub fn last_run_ended_at(last_run_id: Option<Uuid>, ended_at: &str) -> Option<&str> {
    last_run_id.map(|_| ended_at)
}

/// `pod_name` (`scheduler.py:130`): `CharField(source="pod.name",
/// read_only=True, default=None)` — the joined pod name, or `None` when
/// the binding uses the project default (NULL pod). Quirk (F36-02): the
/// list join does not filter `pod.deleted_at`, so a soft-deleted pod
/// still renders its stale name — the queries layer owns that join.
pub fn pod_display_name(pod_id: Option<Uuid>, name: &str) -> Option<&str> {
    pod_id.map(|_| name)
}

// ---------------------------------------------------------------------------
// Error bodies (DRF 3.15.2 `as_serializer_error` + `exception_handler`).

/// One DRF field-`ValidationError` body, byte-identical:
/// `{"<field>": ["<message>"]}`.
pub fn field_error_body(field: &str, message: &str) -> String {
    format!(
        "{{\"{}\":[\"{}\"]}}",
        escape_json_string(field),
        escape_json_string(message)
    )
}

/// The nested-dict body a field validator produces by raising
/// `ValidationError({field: message})` (`rdates` / `exdates`):
/// `{"<field>": {"<field>": "<message>"}}` — bare inner string, no list
/// wrap (`as_serializer_error` passes nested dicts through).
pub fn nested_field_error_body(field: &str, message: &str) -> String {
    format!(
        "{{\"{}\":{{\"{}\":\"{}\"}}}}",
        escape_json_string(field),
        escape_json_string(field),
        escape_json_string(message)
    )
}

/// Django-`json.dumps` string escaping for the hand-assembled bodies
/// above (DRF renders with `ensure_ascii=False`): short escapes for `"`,
/// `\`, `\b`, `\f`, `\n`, `\r`, `\t`; `\u00XX` for the other C0
/// controls; everything else — including DEL and all non-ASCII —
/// verbatim. (This coincides with `serde_json` on every input, which the
/// `body_escaping_matches_serde_json` test pins.)
///
/// Handlers must use these builders (not ad-hoc `serde_json` assembly)
/// for serializer error bodies so the escaping stays Django-identical.
fn escape_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Python `repr()` over a string, for the `{value!r}` / `{s!r}` echoes
/// (`tzid` rejection, `Invalid isoformat string`). Quote choice, the
/// `\'` / `\\` / `\n` / `\r` / `\t` short escapes, and the
/// `\xXX` / `\uXXXX` / `\UXXXXXXXX` ladder for the rest match CPython.
/// Boundary: unassigned (Cn) code points other than the permanent
/// noncharacters pass through where CPython would escape them — no
/// contract input exercises those.
fn py_repr_str(value: &str) -> String {
    // Quote choice: double quotes iff the value contains `'` but no `"`.
    // Every other escape applies identically under either quote — only
    // the quote character itself differs (`repr("a'b\n")` is `"a'b\n"`,
    // backslash-n escaped inside double quotes).
    let double_quoted = value.contains('\'') && !value.contains('"');
    let mut out = String::with_capacity(value.len() + 2);
    out.push(if double_quoted { '"' } else { '\'' });
    for c in value.chars() {
        match c {
            '\'' if !double_quoted => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if py_repr_needs_escape(c) => {
                let code = c as u32;
                if code < 0x100 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code < 0x1_0000 {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(if double_quoted { '"' } else { '\'' });
    out
}

/// CPython `str.isprintable() == False`: Cc (via `is_control`), Cf, Co,
/// Zl, Zp, Zs other than U+0020 itself, and the permanent noncharacters
/// (U+FDD0..U+FDEF and U+nFFFE/F — guaranteed never assigned, hence
/// permanently Cn). Category ranges dumped from CPython 3.12's own
/// `unicodedata` (Unicode 15.0, the production version); surrogates (Cs)
/// are unrepresentable in a Rust `char`. Remaining boundary: other Cn
/// (unassigned-but-assignable) code points, which no Unicode table here
/// can enumerate.
fn py_repr_needs_escape(c: char) -> bool {
    if c.is_control() {
        return true;
    }
    let cp = c as u32;
    // Permanent noncharacters: U+FDD0..U+FDEF and the last two scalars of
    // every plane.
    if (0xFDD0..=0xFDEF).contains(&cp) || (cp & 0xFFFF >= 0xFFFE && cp <= 0x10_FFFF) {
        return true;
    }
    matches!(
        cp,
        // Zl + Zp (exactly one scalar each).
        0x2028 | 0x2029
        // Zs (space separators) other than U+0020 itself.
        | 0x00A0 | 0x1680 | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000
        // Cf (format).
        | 0x00AD
        | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F
        | 0x0890..=0x0891 | 0x08E2 | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E
        | 0x2060..=0x2064 | 0x2066..=0x206F
        | 0xFEFF | 0xFFF9..=0xFFFB
        | 0x110BD | 0x110CD | 0x13430..=0x1343F
        | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A
        | 0xE0001 | 0xE0020..=0xE007F
        // Co (private use).
        | 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const F36_01: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/serializers/scheduler_shapes.golden.json"
    );
    const F36_02: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/serializers/binding_shapes.golden.json"
    );
    const F36_03: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_scheduler/serializers/binding_validation.golden.json"
    );

    fn golden(path: &str) -> serde_json::Value {
        let raw = std::fs::read_to_string(path).expect("fixture golden exists");
        serde_json::from_str(&raw).expect("fixture golden is valid JSON")
    }

    fn str_list(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_array()
            .expect("golden carries a string list")
            .iter()
            .map(|v| v.as_str().expect("field names are strings"))
            .collect()
    }

    /// A stub `rrule_validator`: canned verdicts plus a call log.
    struct StubValidator {
        calls: RefCell<Vec<String>>,
        verdict: RefCell<Result<(), String>>,
    }

    impl StubValidator {
        fn ok() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                verdict: RefCell::new(Ok(())),
            }
        }

        fn err(message: &str) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                verdict: RefCell::new(Err(message.to_owned())),
            }
        }

        fn closure(&self) -> impl Fn(&str) -> Result<(), String> + '_ {
            |input| {
                self.calls.borrow_mut().push(input.to_owned());
                self.verdict.borrow().clone()
            }
        }
    }

    #[test]
    fn scheduler_fields_match_f36_01_in_order() {
        let parsed = golden(F36_01);
        let expected = str_list(&parsed["fields"]);
        assert_eq!(SCHEDULER_SERIALIZER_FIELDS.len(), 12);
        assert_eq!(SCHEDULER_SERIALIZER_FIELDS, expected.as_slice());
    }

    #[test]
    fn scheduler_read_only_and_render_keys_match_f36_01() {
        let parsed = golden(F36_01);
        let guards = str_list(&parsed["read_only_fields"]);
        assert_eq!(SCHEDULER_READ_ONLY_FIELDS, guards.as_slice());
        // The render example carries exactly the 12 keys (order is pinned
        // by the `fields` array above — `serde_json` maps do not preserve
        // document order).
        let mut rendered: Vec<&str> = parsed["render_example"]
            .as_object()
            .expect("golden carries render_example")
            .keys()
            .map(String::as_str)
            .collect();
        rendered.sort_unstable();
        let mut fields = SCHEDULER_SERIALIZER_FIELDS.to_vec();
        fields.sort_unstable();
        assert_eq!(rendered, fields);
    }

    #[test]
    fn binding_fields_match_f36_02_in_order() {
        let parsed = golden(F36_02);
        let expected = str_list(&parsed["fields"]);
        assert_eq!(BINDING_SERIALIZER_FIELDS.len(), 25);
        assert_eq!(BINDING_SERIALIZER_FIELDS, expected.as_slice());
    }

    #[test]
    fn binding_effective_read_only_is_14() {
        let parsed = golden(F36_02);
        // The golden's `read_only_fields` is the *effective* 14-key set
        // (fields order); the port keeps the two sources separate (the 11
        // Meta entries plus the 3 declared derived fields) and unions them.
        let golden_effective = str_list(&parsed["read_only_fields"]);
        assert_eq!(golden_effective.len(), 14);
        assert_eq!(BINDING_META_READ_ONLY_FIELDS.len(), 11);
        assert_eq!(BINDING_DECLARED_READ_ONLY_FIELDS.len(), 3);
        let mut effective = binding_read_only_fields();
        assert_eq!(effective.len(), 14);
        effective.sort_unstable();
        let mut golden_sorted = golden_effective;
        golden_sorted.sort_unstable();
        assert_eq!(effective, golden_sorted);
        // Every effective key is a real field.
        for key in &effective {
            assert!(BINDING_SERIALIZER_FIELDS.contains(key));
        }
        // The render example carries exactly the 25 keys.
        let mut rendered: Vec<&str> = parsed["render_example"]
            .as_object()
            .expect("golden carries render_example")
            .keys()
            .map(String::as_str)
            .collect();
        rendered.sort_unstable();
        let mut fields = BINDING_SERIALIZER_FIELDS.to_vec();
        fields.sort_unstable();
        assert_eq!(rendered, fields);
    }

    #[test]
    fn color_vectors_match_f36_01() {
        let parsed = golden(F36_01);
        let cases = parsed["color_validation"]["cases"]
            .as_array()
            .expect("golden carries color cases");
        assert_eq!(
            validate_color(cases[0]["in"].as_str().unwrap()),
            Ok("#10b981".to_owned())
        );
        assert_eq!(
            validate_color(cases[1]["in"].as_str().unwrap()),
            Ok("#abcdef".to_owned())
        );
        for case in &cases[2..4] {
            let input = case["in"].as_str().unwrap();
            assert_eq!(validate_color(input), Err(COLOR_ERROR));
            // The golden pins the message list; the wire body wraps it
            // under the field (`{"color": [...]}`).
            let message = case["error"][0].as_str().unwrap();
            assert_eq!(message, COLOR_ERROR);
            assert_eq!(
                field_error_body("color", COLOR_ERROR),
                format!(
                    "{{\"color\":{}}}",
                    serde_json::to_string(&case["error"]).unwrap()
                )
            );
        }
    }

    #[test]
    fn color_drf_preempts_are_documented() {
        // DRF `max_length=7` / `allow_blank=False` reject these before the
        // custom validator runs (F36-01 cases 4-5); the handlers replicate
        // those checks. The raw function below still answers with the
        // custom message — exactly what `_validate_color` itself returns
        // when called directly.
        assert_eq!(validate_color("#12345678"), Err(COLOR_ERROR));
        assert_eq!(validate_color(""), Err(COLOR_ERROR));
    }

    #[test]
    fn color_strip_and_case_edges() {
        // Python-strip parity: FS/GS/RS/US strip like whitespace.
        assert_eq!(
            validate_color("\u{1c}#ABCDEF\u{1f}"),
            Ok("#abcdef".to_owned())
        );
        // Non-breaking space strips too (Unicode whitespace).
        assert_eq!(
            validate_color("\u{a0}#ABCDEF\u{a0}"),
            Ok("#abcdef".to_owned())
        );
        // Full-width digits are not ASCII hex.
        assert_eq!(validate_color("#ABCDE１"), Err(COLOR_ERROR));
        // A trailing newline would satisfy `$` in the regex, but the strip
        // removes it first — equivalent to the exact-length check.
        assert_eq!(validate_color("#ABCDEF\n"), Ok("#abcdef".to_owned()));
        // Interior whitespace survives the strip and fails.
        assert_eq!(validate_color("#AB CDE"), Err(COLOR_ERROR));
    }

    #[test]
    fn active_binding_count_resolution() {
        // Annotated wins — including an annotated 0 (`is not None`).
        assert_eq!(resolve_active_binding_count(Some(3), 99), 3);
        assert_eq!(resolve_active_binding_count(Some(0), 99), 0);
        // Missing annotation → the queries layer's count.
        assert_eq!(resolve_active_binding_count(None, 7), 7);
    }

    #[test]
    fn last_run_and_pod_gates() {
        let id = Uuid::parse_str("68ad4deb-fc7c-4531-b5ce-376263af21e3").unwrap();
        assert_eq!(last_run_status(Some(id), "completed"), Some("completed"));
        assert_eq!(last_run_status(None, "completed"), None);
        assert_eq!(
            last_run_ended_at(Some(id), "2024-05-06T07:08:09Z"),
            Some("2024-05-06T07:08:09Z")
        );
        assert_eq!(last_run_ended_at(None, "2024-05-06T07:08:09Z"), None);
        assert_eq!(pod_display_name(Some(id), "pod-a"), Some("pod-a"));
        assert_eq!(pod_display_name(None, "pod-a"), None);
    }

    #[test]
    fn rrule_canonicalization() {
        let stub = StubValidator::ok();
        let check = |input: &str, expected: &str| {
            assert_eq!(
                validate_rrule(input, &stub.closure()),
                Ok(expected.to_owned())
            );
        };
        check("", "");
        check("RRULE:FREQ=DAILY", "FREQ=DAILY");
        // Prefix test is case-insensitive; the remainder keeps its case.
        check("rrule:freq=daily", "freq=daily");
        check("RrUlE:FREQ=DAILY", "FREQ=DAILY");
        check("  FREQ=HOURLY  ", "FREQ=HOURLY");
        check("RRULE:", "");
        // `RRULE:`-only strips to `""` and Python still calls the
        // validator (a no-op) — the closure sees the empty string.
        assert!(stub.calls.borrow().iter().any(String::is_empty));
        // A bare empty input never reaches the validator.
        let calls_before = stub.calls.borrow().len();
        assert_eq!(validate_rrule("   ", &stub.closure()), Ok(String::new()));
        assert_eq!(stub.calls.borrow().len(), calls_before);
        // Non-ASCII never matches the prefix (no Unicode char uppercases
        // to an `R`-start), so it validates verbatim.
        check("ｒrule:FREQ=DAILY", "ｒrule:FREQ=DAILY");
    }

    #[test]
    fn rrule_message_passthrough_matches_f36_03() {
        let parsed = golden(F36_03);
        let cases = parsed["rrule"]["cases"]
            .as_array()
            .expect("golden carries rrule cases");
        // The verdict messages come from the jobs validator; the seam
        // passes the string through into `{"rrule": [message]}`.
        for case in &cases[5..8] {
            let input = case["in"].as_str().expect("case input").trim_matches('\'');
            let message = case["out_400"]["rrule"][0].as_str().unwrap().to_owned();
            let stub = StubValidator::err(&message);
            assert_eq!(validate_rrule(input, &stub.closure()), Err(message.clone()));
            assert_eq!(stub.calls.borrow().as_slice(), [input.to_owned()]);
            assert_eq!(
                field_error_body("rrule", &message),
                serde_json::to_string(&case["out_400"]).unwrap()
            );
        }
    }

    #[test]
    fn iso_list_vectors_match_f36_03() {
        let parsed = golden(F36_03);
        let cases = parsed["iso_datetime_list"]["cases"]
            .as_array()
            .expect("golden carries iso cases");
        // `[]` → `[]`.
        assert_eq!(
            validate_iso_datetime_list(&serde_json::json!([]), "rdates"),
            Ok(Vec::new())
        );
        // Normalization trio: `Z` becomes `+00:00`, offsets and
        // naive-ness preserved (no UTC conversion).
        let trio = serde_json::json!([
            "2024-01-02T03:04:05Z",
            "2024-06-01T12:00:00+05:00",
            "2024-01-01T00:00:00"
        ]);
        assert_eq!(
            validate_iso_datetime_list(&trio, "rdates"),
            Ok(vec![
                "2024-01-02T03:04:05+00:00".to_owned(),
                "2024-06-01T12:00:00+05:00".to_owned(),
                "2024-01-01T00:00:00".to_owned(),
            ])
        );
        // 256 items OK; 257 rejected.
        let many: Vec<serde_json::Value> = (0..257)
            .map(|_| serde_json::Value::String("2024-01-01T00:00:00".to_owned()))
            .collect();
        assert!(validate_iso_datetime_list(
            &serde_json::Value::Array(many[..256].to_vec()),
            "rdates"
        )
        .is_ok());
        let too_long =
            validate_iso_datetime_list(&serde_json::Value::Array(many), "rdates").unwrap_err();
        assert_eq!(too_long.field, "rdates");
        assert_eq!(
            nested_field_error_body(&too_long.field, &too_long.message),
            serde_json::to_string(&cases[4]["out_400"]).unwrap()
        );
        // Non-list → nested dict with bare inner string.
        let not_list =
            validate_iso_datetime_list(&serde_json::json!("tomorrow"), "rdates").unwrap_err();
        assert_eq!(
            nested_field_error_body(&not_list.field, &not_list.message),
            serde_json::to_string(&cases[3]["out_400"]).unwrap()
        );
        // Non-string item → `type().__name__`.
        let mixed = serde_json::json!(["2024-01-01T00:00:00Z", 5]);
        let type_err = validate_iso_datetime_list(&mixed, "rdates").unwrap_err();
        assert_eq!(
            nested_field_error_body(&type_err.field, &type_err.message),
            serde_json::to_string(&cases[5]["out_400"]).unwrap()
        );
        // Unparsable item → the `ValueError` text verbatim.
        let bad = serde_json::json!(["2024-01-01T00:00:00Z", "nope"]);
        let parse_err = validate_iso_datetime_list(&bad, "exdates").unwrap_err();
        assert_eq!(parse_err.field, "exdates");
        assert_eq!(
            nested_field_error_body(&parse_err.field, &parse_err.message),
            serde_json::to_string(&cases[6]["out_400"]).unwrap()
        );
        // `null → []` is ported verbatim although DRF's `allow_null=False`
        // rejects null first (F36-03 case 7 — the handlers own that check).
        assert_eq!(
            validate_iso_datetime_list(&serde_json::Value::Null, "rdates"),
            Ok(Vec::new())
        );
    }

    const OFFSET_ERR: &str = "offset must be a timedelta strictly between -timedelta(hours=24) and timedelta(hours=24), not ";

    /// `Invalid isoformat string` with a plain single-quote echo. Every
    /// vector below echoes printable ASCII without quotes, so this is
    /// exact; quoting/escaping itself is pinned by
    /// `invalid_echo_escaping` + `py_repr_vectors`.
    fn inv(echo: &str) -> String {
        format!("Invalid isoformat string: '{echo}'")
    }

    fn off(td: &str) -> String {
        format!("{OFFSET_ERR}{td}.")
    }

    #[test]
    fn iso_grammar_vectors_match_cpython() {
        // (`input`, `Ok(rendered)` / `Err(ValueError text)`), transcribed
        // from live CPython 3.12 probes. `D` = `2024-01-01T00:00:00`.
        let vectors: &[(&str, Result<&str, String>)] = &[
            // Basics, Z handling, date-only, truncation.
            ("2024-01-02T03:04:05Z", Ok("2024-01-02T03:04:05+00:00")),
            ("2024-06-01T12:00:00+05:00", Ok("2024-06-01T12:00:00+05:00")),
            ("2024-01-01T00:00:00", Ok("2024-01-01T00:00:00")),
            ("nope", Err(inv("nope"))),
            ("", Err(inv(""))),
            ("2024-01-01", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00:00.123456789Z",
                Ok("2024-01-01T00:00:00.123456+00:00"),
            ),
            ("2024-01-01 00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00:00:00+0000", Ok("2024-01-01T00:00:00+00:00")),
            ("2024-01-01T00:00:00+00", Ok("2024-01-01T00:00:00+00:00")),
            ("2024-13-01", Err("month must be in 1..12".to_owned())),
            (
                "2024-01-32T00:00:00",
                Err("day is out of range for month".to_owned()),
            ),
            (
                "2024-01-01T24:00:00",
                Err("hour must be in 0..23".to_owned()),
            ),
            ("2024-01-01T00:00:00.", Err(inv("2024-01-01T00:00:00."))),
            ("2024-W01-1", Ok("2024-01-01T00:00:00")),
            ("2024-001", Err(inv("2024-001"))),
            ("20240101", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00:00:00,123", Ok("2024-01-01T00:00:00.123000")),
            // The echo shows the *replaced* string.
            (
                "2024-01-01T00:00:00Z ",
                Err(inv("2024-01-01T00:00:00+00:00 ")),
            ),
            (
                " 2024-01-01T00:00:00Z",
                Err(inv(" 2024-01-01T00:00:00+00:00")),
            ),
            (
                "2024-01-01T00:00:00+00:00:00",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            ("2024-01-01T00:00:00+05", Ok("2024-01-01T00:00:00+05:00")),
            ("2024-01-01T00:00:00.5", Ok("2024-01-01T00:00:00.500000")),
            ("0001-01-01T00:00:00", Ok("0001-01-01T00:00:00")),
            ("2024-01-01T00:00:00+14:00", Ok("2024-01-01T00:00:00+14:00")),
            ("2024-01-01T00:00:00-23:59", Ok("2024-01-01T00:00:00-23:59")),
            (
                "2024-01-01T00:00:00+24:00",
                Err(off("datetime.timedelta(days=1)")),
            ),
            ("12:00:00", Err(inv("12:00:00"))),
            ("2024-01", Err(inv("2024-01"))),
            ("2024", Err(inv("2024"))),
            (
                "2024-01-01T00:00:00+00:0",
                Err(inv("2024-01-01T00:00:00+00:0")),
            ),
            ("2024-1-1", Err(inv("2024-1-1"))),
            ("2024-01-01T0:00:00", Err(inv("2024-01-01T0:00:00"))),
            ("2024-01-01t00:00:00z", Err(inv("2024-01-01t00:00:00z"))),
            (
                "2024-01-01T00:00:00++00:00",
                Err(inv("2024-01-01T00:00:00++00:00")),
            ),
            (
                "2024-01-01T00:00:00+00:00Z",
                Err(inv("2024-01-01T00:00:00+00:00+00:00")),
            ),
            (
                "9999-12-31T23:59:59.999999",
                Ok("9999-12-31T23:59:59.999999"),
            ),
            (
                "2024-02-30T00:00:00",
                Err("day is out of range for month".to_owned()),
            ),
            (
                "2024-01-01T00:00:00.1234567890123",
                Ok("2024-01-01T00:00:00.123456"),
            ),
            (
                "2023-02-29T00:00:00",
                Err("day is out of range for month".to_owned()),
            ),
            (
                "2024-04-31",
                Err("day is out of range for month".to_owned()),
            ),
            (
                "2024-01-01T00:00:61",
                Err("second must be in 0..59".to_owned()),
            ),
            // Offset seconds, overflow normalization, offset errors.
            (
                "2024-01-01T00:00:00+05:30:45",
                Ok("2024-01-01T00:00:00+05:30:45"),
            ),
            (
                "2024-01-01T00:00:00+00:00:01",
                Ok("2024-01-01T00:00:00+00:00:01"),
            ),
            (
                "2024-01-01T00:00:00+23:59:59",
                Ok("2024-01-01T00:00:00+23:59:59"),
            ),
            (
                "2024-01-01T00:00:00-24:00",
                Err(off("datetime.timedelta(days=-1)")),
            ),
            (
                "2024-01-01T00:00:00+25:00",
                Err(off("datetime.timedelta(days=1, seconds=3600)")),
            ),
            (
                "2024-01-01T00:00:00+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            ("2024-01-01T00:00:00+05:60", Ok("2024-01-01T00:00:00+06:00")),
            (
                "2024-01-01T00:00:00+05:30:99",
                Ok("2024-01-01T00:00:00+05:31:39"),
            ),
            (
                "2024-01-01T00:60:00",
                Err("minute must be in 0..59".to_owned()),
            ),
            (
                "2024-01-01T00:00:60",
                Err("second must be in 0..59".to_owned()),
            ),
            (
                "2024-01-01T00:00:00.abcdef",
                Err(inv("2024-01-01T00:00:00.abcdef")),
            ),
            ("2024-W54-1", Err(inv("2024-W54-1"))),
            ("2024-W01-8", Err(inv("2024-W01-8"))),
            ("2024-W01-0", Err(inv("2024-W01-0"))),
            ("2024-W01", Ok("2024-01-01T00:00:00")),
            ("20240101T000000", Ok("2024-01-01T00:00:00")),
            ("20240101T000000+0000", Ok("2024-01-01T00:00:00+00:00")),
            ("2024-01-01t00:00:00", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00:00+5:00",
                Err(inv("2024-01-01T00:00:00+5:00")),
            ),
            ("2024-01-01T00:00.5", Ok("2024-01-01T00:00:00.500000")),
            ("2024-01-01T00:00,5", Ok("2024-01-01T00:00:00.500000")),
            ("2024-01-01X00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00:00:00+00:60", Ok("2024-01-01T00:00:00+01:00")),
            ("2024-01-01T00:00:00+0", Err(inv("2024-01-01T00:00:00+0"))),
            (
                "2024-01-01T00:00:00+000",
                Err(inv("2024-01-01T00:00:00+000")),
            ),
            (
                "2024-01-01T00:00:00+00000",
                Err(inv("2024-01-01T00:00:00+00000")),
            ),
            // `+00:00` after a bare date is separator `+` + time `00:00`.
            ("2024-01-01+00:00", Ok("2024-01-01T00:00:00")),
            ("-0001-01-01", Err(inv("-0001-01-01"))),
            ("10000-01-01", Err(inv("10000-01-01"))),
            (
                "2024-01-01T00:00:00,123456789",
                Ok("2024-01-01T00:00:00.123456"),
            ),
            ("２０２４-０１-０１", Err(inv("２０２４-０１-０１"))),
            (
                "2024-01-01T00:00:00１２",
                Err(inv("2024-01-01T00:00:00１２")),
            ),
            ("2024-02-29T00:00:00", Ok("2024-02-29T00:00:00")),
            ("2000-02-29T00:00:00", Ok("2000-02-29T00:00:00")),
            (
                "1900-02-29",
                Err("day is out of range for month".to_owned()),
            ),
            // Empty fraction is fine before a timezone ...
            (
                "2024-01-01T00:00:00.+05:00",
                Ok("2024-01-01T00:00:00+05:00"),
            ),
            ("2024-01-01T00:00:00.+05", Ok("2024-01-01T00:00:00+05:00")),
            (
                "2024-01-01T00:00:00.123+05:30",
                Ok("2024-01-01T00:00:00.123000+05:30"),
            ),
            (
                "2024-01-01T00:00:00,123+05:30",
                Ok("2024-01-01T00:00:00.123000+05:30"),
            ),
            ("2024-01-01T00:00:00+0530", Ok("2024-01-01T00:00:00+05:30")),
            (
                "2024-01-01T00:00:00-053045",
                Ok("2024-01-01T00:00:00-05:30:45"),
            ),
            (
                "2024-01-01T00:00:00+05:30:45.123",
                Ok("2024-01-01T00:00:00+05:30:45.123000"),
            ),
            ("20240101T000000,5", Ok("2024-01-01T00:00:00.500000")),
            ("2024-W01-1T00:00:00+05:00", Ok("2024-01-01T00:00:00+05:00")),
            ("2024-W01-1 00:00:00", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00:00+00:00 ",
                Err(inv("2024-01-01T00:00:00+00:00 ")),
            ),
            ("2024-01-01T00:00:0", Err(inv("2024-01-01T00:00:0"))),
            ("2024-01-01T00:00:000", Err(inv("2024-01-01T00:00:000"))),
            (
                "2024-01-01T00:00:00.1.2",
                Err(inv("2024-01-01T00:00:00.1.2")),
            ),
            ("2024-01-01T00:00:00+", Err(inv("2024-01-01T00:00:00+"))),
            ("2024-01-01T00:00:00-", Err(inv("2024-01-01T00:00:00-"))),
            (
                "2024-01-01T00:00:00Z+05:00",
                Err(inv("2024-01-01T00:00:00+00:00+05:00")),
            ),
            ("ZZ", Err(inv("+00:00+00:00"))),
            ("2024-01-01T00:00:00.  ", Err(inv("2024-01-01T00:00:00.  "))),
            ("2024-01-01T00:00:00,", Err(inv("2024-01-01T00:00:00,"))),
            // Fraction after HH / HH:MM is still sub-second.
            ("2024-01-01T00.5", Ok("2024-01-01T00:00:00.500000")),
            ("2024-01-01+05:00", Ok("2024-01-01T05:00:00")),
            ("2024-01-01Z", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00:00.+05:00", Ok("2024-01-01T00:00:00+05:00")),
            ("2024-01-01T00.+05:00", Ok("2024-01-01T00:00:00+05:00")),
            (
                "2024-01-01T00:00:00,1234567890123456789012345678901234567890",
                Ok("2024-01-01T00:00:00.123456"),
            ),
            // Date/time formats are independent; time mixing is rejected.
            ("20240101T00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T000000", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T", Err(inv("2024-01-01T"))),
            (
                "2024-01-01T00:00:00.123456+00:00:00.000000",
                Ok("2024-01-01T00:00:00.123456+00:00"),
            ),
            ("20240101T00", Ok("2024-01-01T00:00:00")),
            ("2024W01", Ok("2024-01-01T00:00:00")),
            ("2024W011", Ok("2024-01-01T00:00:00")),
            ("2024-W011", Err(inv("2024-W011"))),
            ("2020-W53-7", Ok("2021-01-03T00:00:00")),
            ("2021-W53-1", Err(inv("2021-W53-1"))),
            ("2024-01-01T00_00_00", Err(inv("2024-01-01T00_00_00"))),
            ("2024-01-01T00:0000", Err(inv("2024-01-01T00:0000"))),
            ("0000-01-01", Err("year 0 is out of range".to_owned())),
            // Offset fractions are sub-second too.
            (
                "2024-01-01T00:00:00+05:30.5",
                Ok("2024-01-01T00:00:00+05:30:00.500000"),
            ),
            (
                "2024-01-01T00:00:00+05.5",
                Ok("2024-01-01T00:00:00+05:00:00.500000"),
            ),
            (
                "2024-01-01T00:00:00+053045.5",
                Ok("2024-01-01T00:00:00+05:30:45.500000"),
            ),
            (
                "2024-01-01T00:00:00+23:59:60",
                Err(off("datetime.timedelta(days=1)")),
            ),
            (
                "2024-01-01T00:00:00-25:00",
                Err(off("datetime.timedelta(days=-2, seconds=82800)")),
            ),
            (
                "2024-01-01T00:00:00-99:99",
                Err(off("datetime.timedelta(days=-5, seconds=69660)")),
            ),
            // The all-zero offset fast-path quirk drops parsed microseconds.
            (
                "2024-01-01T00:00:00+00:00:00.0000001",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            ("2024-01-01T00:00:00.0000001", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00:00:00.0000000", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00:00+00:00:00.5",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00-00:00:00.5",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            ("2024-01-01T00:00:00+00:60", Ok("2024-01-01T00:00:00+01:00")),
            ("2024-01-01T00:00:00-00:60", Ok("2024-01-01T00:00:00-01:00")),
            ("T00:00:00", Err(inv("T00:00:00"))),
            (
                "2024-01-01T00:00:00+00:00:00,5",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            ("02024-01-01", Err(inv("02024-01-01"))),
            ("+2024-01-01", Err(inv("+2024-01-01"))),
            (
                "2024-13-01T00:00:00",
                Err("month must be in 1..12".to_owned()),
            ),
            (
                "2024-13-40T99:99:99",
                Err("month must be in 1..12".to_owned()),
            ),
            (
                "2024-01-32T99:99:99",
                Err("day is out of range for month".to_owned()),
            ),
            // Offset-range beats time-range.
            (
                "2024-01-01T99:00:00+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            (
                "2024-01-01T00:00:00+24:00extra",
                Err(inv("2024-01-01T00:00:00+24:00extra")),
            ),
            ("2024-13-01+99:99", Err("month must be in 1..12".to_owned())),
            ("abcd-01-01T00:00:00", Err(inv("abcd-01-01T00:00:00"))),
            ("2024-01-01\n00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01\t00:00:00", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00:00.123000",
                Ok("2024-01-01T00:00:00.123000"),
            ),
            (
                "2024-01-01T00:00:00+05:30:45.123000",
                Ok("2024-01-01T00:00:00+05:30:45.123000"),
            ),
            (
                "2024-01-01T00:00:00.000001",
                Ok("2024-01-01T00:00:00.000001"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.000001",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            ("2019-W52-7", Ok("2019-12-29T00:00:00")),
            ("2019-12-29", Ok("2019-12-29T00:00:00")),
            ("0001-01-01", Ok("0001-01-01T00:00:00")),
            ("2024-01-01T00:00:00+0059", Ok("2024-01-01T00:00:00+00:59")),
            (
                "2024-01-01T00:00:00+2400",
                Err(off("datetime.timedelta(days=1)")),
            ),
            (
                "2024-01-01T00:00:00+24",
                Err(off("datetime.timedelta(days=1)")),
            ),
            (
                "2024-01-01T00:00:00-24",
                Err(off("datetime.timedelta(days=-1)")),
            ),
            (
                "2024-01-01T00:00:00+00:00:60",
                Ok("2024-01-01T00:00:00+00:01"),
            ),
            (
                "2024-01-01T00:00:00+00:00:60.5",
                Ok("2024-01-01T00:00:00+00:01:00.500000"),
            ),
            (
                "2024-01-01T00:00:00+15:00:00.9999999",
                Ok("2024-01-01T00:00:00+15:00:00.999999"),
            ),
            (
                "2024-06-30T23:59:60",
                Err("second must be in 0..59".to_owned()),
            ),
            (
                "2024-01-01T00:00:00,5+05:00",
                Ok("2024-01-01T00:00:00.500000+05:00"),
            ),
            ("2024-01-01T00:00:0000", Err(inv("2024-01-01T00:00:0000"))),
            ("2024-01-01T0000", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T000000.5", Ok("2024-01-01T00:00:00.500000")),
            (
                "2024-01-01T00:00:00+0:00",
                Err(inv("2024-01-01T00:00:00+0:00")),
            ),
            (
                "2024-01-01T00:00:00+00:0",
                Err(inv("2024-01-01T00:00:00+00:0")),
            ),
            ("2024-01-01T00:00:00.00000000", Ok("2024-01-01T00:00:00")),
            ("9999-W52-7", Err("year 10000 is out of range".to_owned())),
            ("0001-W01-1", Ok("0001-01-01T00:00:00")),
            // Sub-minute offsets render; the drop needs all-zero HH:MM:SS.
            (
                "2024-01-01T00:00:00+00:00:30",
                Ok("2024-01-01T00:00:00+00:00:30"),
            ),
            (
                "2024-01-01T00:00:00+00:00:01.5",
                Ok("2024-01-01T00:00:00+00:00:01.500000"),
            ),
            (
                "2024-01-01T00:00:00+00:01:00.5",
                Ok("2024-01-01T00:00:00+00:01:00.500000"),
            ),
            (
                "2024-01-01T00:00:00+01:00:00.5",
                Ok("2024-01-01T00:00:00+01:00:00.500000"),
            ),
            (
                "2024-01-01T00:00:00-00:00:01.5",
                Ok("2024-01-01T00:00:00-00:00:01.500000"),
            ),
            (
                "2024-01-01T00:00:00+00:00:59.9999999",
                Ok("2024-01-01T00:00:00+00:00:59.999999"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.999999",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00-00:00:00.000001",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.0000005",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+00:00:01.0000005",
                Ok("2024-01-01T00:00:00+00:00:01"),
            ),
            ("2024-01-01T00:00:00+00.5", Ok("2024-01-01T00:00:00+00:00")),
            (
                "2024-01-01T00:00:00+00:00.5",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+00:30.5",
                Ok("2024-01-01T00:00:00+00:30:00.500000"),
            ),
            ("2024-01-01T00:00:00-00.5", Ok("2024-01-01T00:00:00+00:00")),
            (
                "2024-01-01T00:00:00-00:00.5",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.0000000",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+01:00:00.0000001",
                Ok("2024-01-01T00:00:00+01:00"),
            ),
            // The dropped fraction region is still syntax-checked.
            (
                "2024-01-01T00:00:00+00:00:00.5x",
                Err(inv("2024-01-01T00:00:00+00:00:00.5x")),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.",
                Err(inv("2024-01-01T00:00:00+00:00:00.")),
            ),
            (
                "2024-01-01T00:00:00+05:00:00.",
                Err(inv("2024-01-01T00:00:00+05:00:00.")),
            ),
            (
                "2024-01-01T00:00:00+000000.5",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+0030.5",
                Ok("2024-01-01T00:00:00+00:30:00.500000"),
            ),
            (
                "2024-01-01T00:00:00+0000.5",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+000000.000001",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.5+05:00",
                Err(inv("2024-01-01T00:00:00+00:00:00.5+05:00")),
            ),
            (
                "2024-01-01T00:00:00+00:00.0",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            ("2024-01-01T00:00:00+00.0", Ok("2024-01-01T00:00:00+00:00")),
            (
                "2024-01-01T00:00:00+00:00:00.0",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00-00:00:30",
                Ok("2024-01-01T00:00:00-00:00:30"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.5000000",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00,999999",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            // ... and still counts in the ±24h error's timedelta.
            (
                "2024-01-01T00:00:00+24:00:00.5",
                Err(off("datetime.timedelta(days=1, microseconds=500000)")),
            ),
            (
                "2024-01-01T00:00:00+23:59:59.9999999",
                Ok("2024-01-01T00:00:00+23:59:59.999999"),
            ),
            (
                "2024-01-01T00:00:00+23:59:59.99999999",
                Ok("2024-01-01T00:00:00+23:59:59.999999"),
            ),
            (
                "2024-01-01T00:00:00-23:59:59.9999999",
                Ok("2024-01-01T00:00:00-23:59:59.999999"),
            ),
            (
                "2024-01-01T00:00:00+23:59:60",
                Err(off("datetime.timedelta(days=1)")),
            ),
            (
                "2024-01-01T00:00:00-23:59:60",
                Err(off("datetime.timedelta(days=-1)")),
            ),
            // Syntax beats range; offset-range beats date- and time-range.
            (
                "2024-01-01T99:00:00+ZZ",
                Err(inv("2024-01-01T99:00:00++00:00+00:00")),
            ),
            (
                "2024-01-32T00:00:00+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            (
                "2024-01-01T00:61:00+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            (
                "2024-01-01T00:00:61+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            (
                "2024-13-01T99:99:99+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            ("2024-01-01é00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01𝄞00:00:00", Ok("2024-01-01T00:00:00")),
            ("0000-W01-1", Err("month must be in 1..12".to_owned())),
            (
                "2024-01-01T00:00:00+053",
                Err(inv("2024-01-01T00:00:00+053")),
            ),
            (
                "2024-01-01T00:00:00+05304",
                Err(inv("2024-01-01T00:00:00+05304")),
            ),
            (
                "2024-01-01T00:00:00+0530456",
                Err(inv("2024-01-01T00:00:00+0530456")),
            ),
            (
                "2024-01-01T00:00:00+000:00",
                Err(inv("2024-01-01T00:00:00+000:00")),
            ),
            (
                "2024-01-01T00:00:00+00:000",
                Err(inv("2024-01-01T00:00:00+00:000")),
            ),
            (
                "2024-01-01T00:00:00+00:00:000",
                Err(inv("2024-01-01T00:00:00+00:00:000")),
            ),
            (
                "2024-01-01T00:00:00+0000:00",
                Err(inv("2024-01-01T00:00:00+0000:00")),
            ),
            (
                "2024-01-01T00:00:00+00:0000",
                Err(inv("2024-01-01T00:00:00+00:0000")),
            ),
            (
                "2024-01-01T00:00:00+0000000",
                Err(inv("2024-01-01T00:00:00+0000000")),
            ),
            // `:` is a fraction separator in extended format (time + offset).
            (
                "2024-01-01T00:00:00+00:00:00:00",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            ("2024-01-01T00:00:00:00", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00:00+05:30:",
                Err(inv("2024-01-01T00:00:00+05:30:")),
            ),
            (
                "2024-01-01T00:00:00+05:",
                Err(inv("2024-01-01T00:00:00+05:")),
            ),
            (
                "2024-01-01T00:00:00+05:30:4",
                Err(inv("2024-01-01T00:00:00+05:30:4")),
            ),
            (
                "2024-01-01T00:00:00+05:30:456",
                Err(inv("2024-01-01T00:00:00+05:30:456")),
            ),
            ("2024-01-01T00.25", Ok("2024-01-01T00:00:00.250000")),
            ("2024-01-01T00:00.25", Ok("2024-01-01T00:00:00.250000")),
            ("2024-01-01.5", Err(inv("2024-01-01.5"))),
            ("20240101+0000", Ok("2024-01-01T00:00:00")),
            ("20240101+00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01-05:00", Ok("2024-01-01T05:00:00")),
            ("2024-01-0105:00", Err(inv("2024-01-0105:00"))),
            ("2024-01-01+00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01+0", Err(inv("2024-01-01+0"))),
            ("2024-01-01 ", Err(inv("2024-01-01 "))),
            ("2024-01-01000000", Err(inv("2024-01-01000000"))),
            ("2024-01-01z00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00:00:00z", Err(inv("2024-01-01T00:00:00z"))),
            (
                "2024-01-01T00:00:00.5Z",
                Ok("2024-01-01T00:00:00.500000+00:00"),
            ),
            ("2024-01-01T000000.+00:00", Ok("2024-01-01T00:00:00+00:00")),
            ("2024-01-01T00:00:00.0000005", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00:00.9999999",
                Ok("2024-01-01T00:00:00.999999"),
            ),
            (
                "2024-01-01T00:00:00.5.6",
                Err(inv("2024-01-01T00:00:00.5.6")),
            ),
            ("20241301", Err("month must be in 1..12".to_owned())),
            ("20240001", Err("month must be in 1..12".to_owned())),
            ("20240230", Err("day is out of range for month".to_owned())),
            ("2024-W00-1", Err(inv("2024-W00-1"))),
            ("0001-W52-7", Ok("0001-12-30T00:00:00")),
            (
                "2024-01-01T00:00:00+00:00:00,",
                Err(inv("2024-01-01T00:00:00+00:00:00,")),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.5,6",
                Err(inv("2024-01-01T00:00:00+00:00:00.5,6")),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.+05:00",
                Err(inv("2024-01-01T00:00:00+00:00:00.+05:00")),
            ),
            (
                "2024-01-01T00:00:00,5,6",
                Err(inv("2024-01-01T00:00:00,5,6")),
            ),
            ("2024-01-01T01:02:03:04", Ok("2024-01-01T01:02:03.040000")),
            ("2024-01-01T00:00:05:07", Ok("2024-01-01T00:00:05.070000")),
            (
                "2024-01-01T00:00:05:07.5",
                Err(inv("2024-01-01T00:00:05:07.5")),
            ),
            (
                "2024-01-01T00:00:00:00:00",
                Err(inv("2024-01-01T00:00:00:00:00")),
            ),
            ("2024-01-01T00:00:00:", Err(inv("2024-01-01T00:00:00:"))),
            (
                "2024-01-01T00:00:00.5:06",
                Err(inv("2024-01-01T00:00:00.5:06")),
            ),
            (
                "2024-01-01T00:00:00+00:00:05:07",
                Ok("2024-01-01T00:00:00+00:00:05.070000"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00:05.5",
                Err(inv("2024-01-01T00:00:00+00:00:00:05.5")),
            ),
            // ... but not after a basic time.
            ("2024-01-01T000000:00", Err(inv("2024-01-01T000000:00"))),
            // Empty colon-fraction before a timezone is accepted, like `.`.
            (
                "2024-01-01T00:00:00:+05:00",
                Ok("2024-01-01T00:00:00+05:00"),
            ),
            (
                "2024-01-01T00:00:00:00+05:00",
                Ok("2024-01-01T00:00:00+05:00"),
            ),
            ("2024-01-01T00:00:00:1", Ok("2024-01-01T00:00:00.100000")),
            ("2024-01-01T00:00:00:123", Ok("2024-01-01T00:00:00.123000")),
            ("2024-01-01T00:00:00:000", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00:00::00",
                Err(inv("2024-01-01T00:00:00::00")),
            ),
            (
                "2024-01-01T12:34:56:78.9",
                Err(inv("2024-01-01T12:34:56:78.9")),
            ),
            ("2024-W01-1T00:00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01:00:00", Ok("2024-01-01T00:00:00")),
            (
                "9999-W52-7T00:00:00+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            (
                "9999-W52-7T99:00:00",
                Err("year 10000 is out of range".to_owned()),
            ),
            (
                "2024-13-01T00:00:00+ZZ",
                Err(inv("2024-13-01T00:00:00++00:00+00:00")),
            ),
            (
                "0000-01-01T00:00:00+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            (
                "abcd-01-01T00:00:00+99:99",
                Err(inv("abcd-01-01T00:00:00+99:99")),
            ),
            (
                "2021-W53-1T00:00:00+99:99",
                Err(inv("2021-W53-1T00:00:00+99:99")),
            ),
            (
                "2024-01-01T00:00:00+05:30:45:06",
                Ok("2024-01-01T00:00:00+05:30:45.060000"),
            ),
            (
                "2024-01-01T00:00:00:00 ",
                Err(inv("2024-01-01T00:00:00:00 ")),
            ),
            (
                "2024-01-01T00:00:00+05:00 ",
                Err(inv("2024-01-01T00:00:00+05:00 ")),
            ),
            (
                "2024-01-01T00:00:000:00",
                Err(inv("2024-01-01T00:00:000:00")),
            ),
            ("2024-01-01T00:000:00", Err(inv("2024-01-01T00:000:00"))),
            (
                "2024-01-01T00:00:00:00:00:00",
                Err(inv("2024-01-01T00:00:00:00:00:00")),
            ),
            ("0000W011", Err("month must be in 1..12".to_owned())),
            ("0000-W01", Err("month must be in 1..12".to_owned())),
            ("0001-W00-1", Err(inv("0001-W00-1"))),
            // Odd basic widths, one-digit components, tz after short times.
            ("2024-01-01T000", Err(inv("2024-01-01T000"))),
            ("2024-01-01T00000", Err(inv("2024-01-01T00000"))),
            ("2024-01-01T0000000", Err(inv("2024-01-01T0000000"))),
            ("2024-01-01T0", Err(inv("2024-01-01T0"))),
            ("2024-01-01T00:", Err(inv("2024-01-01T00:"))),
            ("2024-01-01T00:0", Err(inv("2024-01-01T00:0"))),
            ("2024-01-01T00:00:", Err(inv("2024-01-01T00:00:"))),
            ("2024-01-01T00+05:00", Ok("2024-01-01T00:00:00+05:00")),
            ("2024-01-01T00:00+05:00", Ok("2024-01-01T00:00:00+05:00")),
            (
                "2024-01-01T000000.5+05:00",
                Ok("2024-01-01T00:00:00.500000+05:00"),
            ),
            (
                "2024-01-01T000000,5+05:00",
                Ok("2024-01-01T00:00:00.500000+05:00"),
            ),
            ("2024-01-01T00,5", Ok("2024-01-01T00:00:00.500000")),
            ("2024-01-01T00:00,5", Ok("2024-01-01T00:00:00.500000")),
            (
                "2024-01-01T99:99:99",
                Err("hour must be in 0..23".to_owned()),
            ),
            (
                "2024-01-01T00:99:99",
                Err("minute must be in 0..59".to_owned()),
            ),
            (
                "2024-01-01T00:00:00+00:00:00:05",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            ("2024-01-01000:00:00", Ok("2024-01-01T00:00:00")),
            ("0000-13-01", Err("year 0 is out of range".to_owned())),
            ("0000-00-01", Err("year 0 is out of range".to_owned())),
            ("2024-00-01", Err("month must be in 1..12".to_owned())),
            (
                "2024-01-00",
                Err("day is out of range for month".to_owned()),
            ),
            (
                "2024-01-01T00:00:00+00:00:00:00+05:00",
                Err(inv("2024-01-01T00:00:00+00:00:00:00+05:00")),
            ),
            (
                "2024-01-01T00:00:00,5:06",
                Err(inv("2024-01-01T00:00:00,5:06")),
            ),
            ("2024-01-01\u{0}00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-W01-", Err(inv("2024-W01-"))),
            ("2024-W1", Err(inv("2024-W1"))),
            ("2024W1", Err(inv("2024W1"))),
            ("2024W0112", Err(inv("2024W0112"))),
            ("2024W011T00:00:00", Ok("2024-01-01T00:00:00")),
            ("2024-01-01T00:00:00+00", Ok("2024-01-01T00:00:00+00:00")),
            ("2024-01-01T000000+05", Ok("2024-01-01T00:00:00+05:00")),
            ("2024-02-29", Ok("2024-02-29T00:00:00")),
            (
                "2023-02-29",
                Err("day is out of range for month".to_owned()),
            ),
            ("2000-02-29", Ok("2000-02-29T00:00:00")),
            ("9999-12-31", Ok("9999-12-31T00:00:00")),
            (
                "9999-12-31T23:59:59.999999+00:00",
                Ok("9999-12-31T23:59:59.999999+00:00"),
            ),
            (
                "0001-01-01T00:00:00-00:00:01",
                Ok("0001-01-01T00:00:00-00:00:01"),
            ),
            (
                "2024-01-01T00:00:00−05:00",
                Err(inv("2024-01-01T00:00:00−05:00")),
            ),
            (
                "2024-01-01T00:00:00+05:30:45,6",
                Ok("2024-01-01T00:00:00+05:30:45.600000"),
            ),
            (
                "2024-01-01T00:00:00+00:00:00.0000009",
                Ok("2024-01-01T00:00:00+00:00"),
            ),
            // C-loop mechanics, read off `_datetimemodule.c`: one byte
            // after a component is ignored before a timezone; two fail.
            ("2024-01-01T00X+05:00", Ok("2024-01-01T00:00:00+05:00")),
            ("2024-01-01T009+05:00", Ok("2024-01-01T00:00:00+05:00")),
            ("2024-01-01T00XX+05:00", Err(inv("2024-01-01T00XX+05:00"))),
            ("2024-01-01T00:00X+05:00", Ok("2024-01-01T00:00:00+05:00")),
            (
                "2024-01-01T00:00XX+05:00",
                Err(inv("2024-01-01T00:00XX+05:00")),
            ),
            (
                "2024-01-01T00:00:00X+05:00",
                Ok("2024-01-01T00:00:00+05:00"),
            ),
            (
                "2024-01-01T00:00:00XY+05:00",
                Err(inv("2024-01-01T00:00:00XY+05:00")),
            ),
            (
                "2024-01-01T00:00:009+05:00",
                Ok("2024-01-01T00:00:00+05:00"),
            ),
            (
                "2024-01-01T00:00:0009+05:00",
                Err(inv("2024-01-01T00:00:0009+05:00")),
            ),
            // Basic bare fractions feed the same fraction parser (2+
            // digits); exactly 1 takes the one-byte rule instead.
            (
                "2024-01-01T00000099+05:00",
                Ok("2024-01-01T00:00:00.990000+05:00"),
            ),
            ("2024-01-01T0000009+05:00", Ok("2024-01-01T00:00:00+05:00")),
            ("2024-01-01T0000009", Err(inv("2024-01-01T0000009"))),
            // Past 6 fraction digits the rest passes iff a timezone
            // follows; below 6 every digit is required.
            (
                "2024-01-01T00:00:00.555555b+05:00",
                Ok("2024-01-01T00:00:00.555555+05:00"),
            ),
            (
                "2024-01-01T00:00:00.555555b",
                Err(inv("2024-01-01T00:00:00.555555b")),
            ),
            (
                "2024-01-01T00:00:00.5b+05:00",
                Err(inv("2024-01-01T00:00:00.5b+05:00")),
            ),
            (
                "2024-01-01T00:00:00.8130754150.84+05:00",
                Ok("2024-01-01T00:00:00.813075+05:00"),
            ),
            // Fourth colon group: digits become the fraction.
            (
                "2024-01-01T00:00:00:00:00+05:00",
                Err(inv("2024-01-01T00:00:00:00:00+05:00")),
            ),
            (
                "2024-01-01T00:00:00:0000000+05:00",
                Ok("2024-01-01T00:00:00+05:00"),
            ),
            // Week-date split: a digit at index 10 splits at 8.
            ("2024-W01-300:00", Err(inv("2024-W01-300:00"))),
            ("2024-W01-30X", Err(inv("2024-W01-30X"))),
            ("2024-W01-300", Err(inv("2024-W01-300"))),
            ("2024-W01-12:00", Ok("2024-01-01T12:00:00")),
            ("2024-W01-1T12:00", Ok("2024-01-01T12:00:00")),
            ("2024-W01-3X00:00", Ok("2024-01-03T00:00:00")),
            ("2024W0112:00", Err(inv("2024W0112:00"))),
            ("2024W01120", Ok("2024-01-01T20:00:00")),
            // Basic offsets take bare fractions; the ±24h error carries
            // the parsed microseconds.
            (
                "2024-01-01T00:00:00+09090099",
                Ok("2024-01-01T00:00:00+09:09:00.990000"),
            ),
            (
                "2024-01-01T00:00:00+437018023",
                Err(off(
                    "datetime.timedelta(days=1, seconds=72618, microseconds=23000)",
                )),
            ),
            (
                "2024-01-01T00:00:00+05:30:45.123456b",
                Err(inv("2024-01-01T00:00:00+05:30:45.123456b")),
            ),
            (
                "2024-01-01T00:00:00+05:30:45.1234567",
                Ok("2024-01-01T00:00:00+05:30:45.123456"),
            ),
            // Embedded NUL obeys the same one-byte/clean-end rules.
            (
                "2024-01-01T00:00:00\u{0}+05:00",
                Ok("2024-01-01T00:00:00+05:00"),
            ),
            ("2024-01-01T00:00:00\u{0}", Ok("2024-01-01T00:00:00")),
            (
                "2024-01-01T00:00\u{0}:00",
                Err("Invalid isoformat string: '2024-01-01T00:00\\x00:00'".to_owned()),
            ),
            // Date-range beats time-range; offset-range beats both.
            (
                "0000-01-01T99:00:00",
                Err("year 0 is out of range".to_owned()),
            ),
            (
                "2024-13-01T99:00:00",
                Err("month must be in 1..12".to_owned()),
            ),
            (
                "2024-02-30T99:99:99",
                Err("day is out of range for month".to_owned()),
            ),
            (
                "2024-13-40T99:99:99+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            (
                "0000-W01-1T00:00:00+99:99",
                Err(off("datetime.timedelta(days=4, seconds=16740)")),
            ),
            ("2024W011 beginning", Err(inv("2024W011 beginning"))),
        ];
        assert!(vectors.len() > 250, "table carries the full probe corpus");
        for (input, expected) in vectors {
            let expected: Result<String, String> = match expected {
                Ok(rendered) => Ok((*rendered).to_owned()),
                Err(detail) => Err(detail.clone()),
            };
            assert_eq!(normalize_iso_datetime(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn iso_type_names() {
        for (value, name) in [
            (serde_json::json!(null), "NoneType"),
            (serde_json::json!(true), "bool"),
            (serde_json::json!(false), "bool"),
            (serde_json::json!(5), "int"),
            (serde_json::json!(-5), "int"),
            (serde_json::json!(5.5), "float"),
            (serde_json::json!("x"), "str"),
            (serde_json::json!([1]), "list"),
            (serde_json::json!({"a": 1}), "dict"),
        ] {
            assert_eq!(json_type_name(&value), name);
            // Every non-string item is rejected with its type name (a
            // string item proceeds to ISO parsing instead).
            if name != "str" {
                let err =
                    validate_iso_datetime_list(&serde_json::json!([value]), "rdates").unwrap_err();
                assert_eq!(err.message, format!("item 0 must be a string, got {name}"));
            }
        }
    }

    #[test]
    fn invalid_echo_escaping() {
        // The `Invalid isoformat string: {s!r}` echo uses Python repr over
        // the *replaced* string (full literals — no shared helper with the
        // implementation under test).
        for (input, expected) in [
            ("a\x00b", "Invalid isoformat string: 'a\\x00b'"),
            ("a'b", "Invalid isoformat string: \"a'b\""),
            // Double-quoted echoes still escape everything else.
            ("a'b\nc", "Invalid isoformat string: \"a'b\\nc\""),
            ("a'b\\c", "Invalid isoformat string: \"a'b\\\\c\""),
            ("a\"b", "Invalid isoformat string: 'a\"b'"),
            ("a\\b", "Invalid isoformat string: 'a\\\\b'"),
            ("a\nb", "Invalid isoformat string: 'a\\nb'"),
            ("'\"", "Invalid isoformat string: '\\'\"'"),
            ("éx", "Invalid isoformat string: 'éx'"),
            ("\u{a0}", "Invalid isoformat string: '\\xa0'"),
            ("\u{2028}x", "Invalid isoformat string: '\\u2028x'"),
            ("\u{ad}x", "Invalid isoformat string: '\\xadx'"),
            ("\x7fx", "Invalid isoformat string: '\\x7fx'"),
            ("\u{80}x", "Invalid isoformat string: '\\x80x'"),
            ("😀x", "Invalid isoformat string: '😀x'"),
        ] {
            assert_eq!(normalize_iso_datetime(input), Err(expected.to_owned()));
        }
    }

    #[test]
    fn calendar_boundaries() {
        // Ordinal round-trips at both ends of the datetime range.
        assert_eq!(ordinal_to_ymd(1), (1, 1, 1));
        assert_eq!(ordinal_from_ymd(1, 1, 1), 1);
        assert_eq!(ordinal_to_ymd(MAX_ORDINAL), (9999, 12, 31));
        assert_eq!(ordinal_from_ymd(9999, 12, 31), MAX_ORDINAL);
        assert_eq!(ordinal_to_ymd(ordinal_from_ymd(2024, 2, 29)), (2024, 2, 29));
        // Leap rules incl. century exceptions.
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(0));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        // ISO week counts incl. the year-0 proleptic value (52).
        assert_eq!(weeks_in_iso_year(2020), 53);
        assert_eq!(weeks_in_iso_year(2021), 52);
        assert_eq!(weeks_in_iso_year(2024), 52);
        assert_eq!(weeks_in_iso_year(0), 52);
        assert_eq!(weeks_in_iso_year(1), 52);
        // 0001-01-01 was a Monday.
        assert_eq!(weekday_of_ordinal(1), 0);
        assert_eq!(iso_to_ordinal(1, 1, 1), 1);
        assert_eq!(iso_to_ordinal(2020, 53, 7), ordinal_from_ymd(2021, 1, 3));
    }

    #[test]
    fn tzid_vectors_match_f36_03() {
        let parsed = golden(F36_03);
        let cases = parsed["tzid"]["cases"]
            .as_array()
            .expect("golden carries tzid cases");
        // `'  UTC  '` strips to `UTC`; `America/New_York` passes through.
        assert_eq!(validate_tzid("  UTC  "), Ok("UTC".to_owned()));
        assert_eq!(validate_tzid("UTC"), Ok("UTC".to_owned()));
        assert_eq!(
            validate_tzid("America/New_York"),
            Ok("America/New_York".to_owned())
        );
        // Unknown zone → the exact 400 body.
        let unknown = validate_tzid("Mars/Olympus").unwrap_err();
        let TzidError::Unknown(message) = unknown else {
            panic!("Mars/Olympus must be Unknown, not InvalidKey");
        };
        assert_eq!(
            field_error_body("tzid", &message),
            serde_json::to_string(&cases[6]["out_400"]).unwrap()
        );
        // Direct-call refinements, unreachable via HTTP (DRF blank/null /
        // max_length preempts — the handlers own those): `""` takes the
        // `(value or "UTC")` fallback ...
        assert_eq!(validate_tzid(""), Ok("UTC".to_owned()));
        // ... whitespace strips to `""` and `ZoneInfo('')` raises bare
        // `ValueError` (a 500 in Python) ...
        assert!(matches!(
            validate_tzid("   "),
            Err(TzidError::InvalidKey(_))
        ));
        // ... and a 67-char input fails the membership check here (DRF's
        // `max_length=64` preempts it on the wire).
        assert!(matches!(
            validate_tzid(&"x".repeat(67)),
            Err(TzidError::Unknown(_))
        ));
        // Case-sensitive, like `ZoneInfo`.
        assert!(matches!(validate_tzid("utc"), Err(TzidError::Unknown(_))));
    }

    #[test]
    fn tzid_key_shapes() {
        // Bare-`ValueError` shapes (500 in Python → `InvalidKey` here),
        // with CPython's exact texts.
        for (key, message) in [
            (
                "/etc/passwd",
                "ZoneInfo keys may not be absolute paths, got: /etc/passwd",
            ),
            ("/", "ZoneInfo keys may not be absolute paths, got: /"),
            ("", "ZoneInfo keys must be normalized relative paths, got: "),
            (
                "   ",
                "ZoneInfo keys must be normalized relative paths, got: ",
            ),
            (
                "a/../b",
                "ZoneInfo keys must be normalized relative paths, got: a/../b",
            ),
            (
                "a/./b",
                "ZoneInfo keys must be normalized relative paths, got: a/./b",
            ),
            (
                "America//New_York",
                "ZoneInfo keys must be normalized relative paths, got: America//New_York",
            ),
            (
                "a/..",
                "ZoneInfo keys must be normalized relative paths, got: a/..",
            ),
            (
                "x/..",
                "ZoneInfo keys must be normalized relative paths, got: x/..",
            ),
            (
                "UTC/",
                "ZoneInfo keys must be normalized relative paths, got: UTC/",
            ),
            (
                "./x",
                "ZoneInfo keys must be normalized relative paths, got: ./x",
            ),
            (
                "x/../..",
                "ZoneInfo keys must be normalized relative paths, got: x/../..",
            ),
            (
                ".",
                "ZoneInfo keys must refer to subdirectories of TZPATH, got: .",
            ),
            (
                "..",
                "ZoneInfo keys must refer to subdirectories of TZPATH, got: ..",
            ),
            (
                "../x",
                "ZoneInfo keys must refer to subdirectories of TZPATH, got: ../x",
            ),
            (
                "../..",
                "ZoneInfo keys must refer to subdirectories of TZPATH, got: ../..",
            ),
            // Single-segment NUL always fails `open()` argument parsing.
            ("a\x00b", "embedded null byte"),
            ("\x00", "embedded null byte"),
            ("UTC\x00", "embedded null byte"),
        ] {
            // `""` takes the `UTC` fallback before any key check.
            if key.is_empty() {
                assert_eq!(validate_tzid(key), Ok("UTC".to_owned()));
                continue;
            }
            assert_eq!(
                validate_tzid(key),
                Err(TzidError::InvalidKey(message.to_owned())),
                "key {key:?}"
            );
        }
        // Well-shaped but unknown → the 400 message (shape check passes,
        // membership fails). Multi-segment NUL lands here too: the
        // verdict depends on parent-directory existence, so it is
        // deliberately unmodeled (this parent exists nowhere).
        for key in [
            "a\\b",
            "a:b",
            "~",
            "...",
            "Mars/Olympus",
            "Factory ",
            "zz-no-such-dir-xyz/\x00",
        ] {
            assert!(
                matches!(validate_tzid(key), Err(TzidError::Unknown(_))),
                "key {key:?}"
            );
        }
    }

    #[test]
    fn tzid_membership_samples() {
        // Common zones pass; unknown names fail with the exact message.
        for zone in [
            "UTC",
            "America/New_York",
            "America/Argentina/Buenos_Aires",
            "Europe/London",
            "Asia/Tokyo",
            "Pacific/Auckland",
            "Etc/UTC",
            "CET",
            "EST5EDT",
        ] {
            assert_eq!(validate_tzid(zone), Ok(zone.to_owned()), "zone {zone}");
        }
        assert_eq!(
            validate_tzid("Mars/Olympus"),
            Err(TzidError::Unknown(
                "tzid 'Mars/Olympus' is not a recognized IANA timezone".to_owned()
            ))
        );
        // The `!r` echo renders through `py_repr_str`.
        assert_eq!(
            validate_tzid("o'clock"),
            Err(TzidError::Unknown(
                "tzid \"o'clock\" is not a recognized IANA timezone".to_owned()
            ))
        );
    }

    #[test]
    fn extra_context_boundaries() {
        let parsed = golden(F36_03);
        let cases = parsed["extra_context"]["cases"]
            .as_array()
            .expect("golden carries extra_context cases");
        assert!(validate_extra_context("").is_ok());
        assert!(validate_extra_context(&"x".repeat(EXTRA_CONTEXT_MAX_LENGTH)).is_ok());
        let too_long =
            validate_extra_context(&"x".repeat(EXTRA_CONTEXT_MAX_LENGTH + 1)).unwrap_err();
        assert_eq!(
            field_error_body("extra_context", &too_long),
            serde_json::to_string(&cases[2]["out_400"]).unwrap()
        );
        // `len()` counts code points, not bytes: 16384 `é` (32768 bytes)
        // pass; 16385 fail.
        assert!(validate_extra_context(&"é".repeat(EXTRA_CONTEXT_MAX_LENGTH)).is_ok());
        assert!(validate_extra_context(&"é".repeat(EXTRA_CONTEXT_MAX_LENGTH + 1)).is_err());
    }

    #[test]
    fn lock_vectors_match_f36_03() {
        let parsed = golden(F36_03);
        let cases = parsed["cross_field_validate"]["scheduler_project_lock"]["cases"]
            .as_array()
            .expect("golden carries lock cases");
        let current = Uuid::parse_str("0d065023-aa70-4d7d-85da-029d7169843e").unwrap();
        let other = Uuid::parse_str("68ad4deb-fc7c-4531-b5ce-376263af21e3").unwrap();
        for (field, case) in [("scheduler", &cases[0]), ("project", &cases[1])] {
            let err = validate_locked_field(field, Some(other), current).unwrap_err();
            assert_eq!(
                field_error_body(field, &err),
                serde_json::to_string(&case["out_400"]).unwrap()
            );
        }
        // Same id, absent keys, and create (handlers skip the lock when
        // there is no instance) are all OK.
        assert!(validate_locked_field("scheduler", Some(current), current).is_ok());
        assert!(validate_locked_field("project", Some(current), current).is_ok());
        assert!(validate_locked_field("scheduler", None, current).is_ok());
        assert!(validate_locked_field("project", None, current).is_ok());
    }

    #[test]
    fn cross_rrule_resolution() {
        // Attrs win over the instance; the resolved value validates as-is
        // (no re-strip — attrs carry the canonicalized field value).
        let stub = StubValidator::ok();
        assert!(
            validate_cross_rrule(Some("FREQ=DAILY"), Some("FREQ=HOURLY"), &stub.closure()).is_ok()
        );
        assert_eq!(stub.calls.borrow().as_slice(), ["FREQ=DAILY".to_owned()]);
        // Instance fallback.
        let stub = StubValidator::ok();
        assert!(validate_cross_rrule(None, Some("FREQ=HOURLY"), &stub.closure()).is_ok());
        assert_eq!(stub.calls.borrow().as_slice(), ["FREQ=HOURLY".to_owned()]);
        // Empty everywhere skips the check.
        let stub = StubValidator::ok();
        assert!(validate_cross_rrule(None, None, &stub.closure()).is_ok());
        assert!(validate_cross_rrule(Some(""), Some(""), &stub.closure()).is_ok());
        assert!(stub.calls.borrow().is_empty());
        // Message passthrough into `{"rrule": [message]}` (F36-03
        // cross-field case 0 pins the envelope).
        let parsed = golden(F36_03);
        let case = &parsed["cross_field_validate"]["rrule_dtstart"]["cases"][0];
        let message = case["out_400"]["rrule"][0].as_str().unwrap().to_owned();
        let stub = StubValidator::err(&message);
        assert_eq!(
            validate_cross_rrule(Some("FREQ=NEVER"), None, &stub.closure()),
            Err(message.clone())
        );
        assert_eq!(
            field_error_body("rrule", &message),
            serde_json::to_string(&case["out_400"]).unwrap()
        );
    }

    #[test]
    fn pod_trust_order_matches_f36_03() {
        let parsed = golden(F36_03);
        let cases = parsed["cross_field_validate"]["pod_trust_order"]["cases"]
            .as_array()
            .expect("golden carries pod cases");
        let p1 = Uuid::parse_str("7e93ee4e-e0b5-4f32-933e-4a5a003d997b").unwrap();
        let p2 = Uuid::parse_str("de1cd5d2-f080-465a-af38-ec3556587d8e").unwrap();
        // 0: update, pod in same project → OK.
        assert!(validate_pod_project(Some(p1), Some(p1), None, None).is_ok());
        // 1: update, pod in foreign project → 400.
        let err = validate_pod_project(Some(p2), Some(p1), None, None).unwrap_err();
        assert_eq!(
            field_error_body("pod", err),
            serde_json::to_string(&cases[1]["out_400"]).unwrap()
        );
        // 2: crafted payload — context wins over body → 400.
        let err = validate_pod_project(Some(p2), None, Some(p1), Some(p2)).unwrap_err();
        assert_eq!(
            field_error_body("pod", err),
            serde_json::to_string(&cases[2]["out_400"]).unwrap()
        );
        // 3: context project + its pod → OK.
        assert!(validate_pod_project(Some(p1), None, Some(p1), Some(p2)).is_ok());
        // 4: no context → body fallback → OK.
        assert!(validate_pod_project(Some(p2), None, None, Some(p2)).is_ok());
        // 5: no project known → no check → OK.
        assert!(validate_pod_project(Some(p2), None, None, None).is_ok());
        // 6: pod None / absent → skipped → OK.
        assert!(validate_pod_project(None, Some(p1), Some(p1), Some(p2)).is_ok());
        // Instance wins over context and body alike.
        assert!(validate_pod_project(Some(p1), Some(p1), Some(p2), Some(p2)).is_ok());
        assert!(validate_pod_project(Some(p2), Some(p1), Some(p2), Some(p2)).is_err());
    }

    #[test]
    fn error_bodies_match_fixtures() {
        // Flat bodies across both serializers.
        assert_eq!(
            field_error_body("color", COLOR_ERROR),
            "{\"color\":[\"color must be a 7-character hex string like '#3b82f6'\"]}"
        );
        assert_eq!(
            field_error_body("rrule", "invalid RRULE: invalid 'FREQ': NEVER"),
            "{\"rrule\":[\"invalid RRULE: invalid 'FREQ': NEVER\"]}"
        );
        assert_eq!(
            field_error_body("scheduler", &lock_error("scheduler")),
            "{\"scheduler\":[\"scheduler cannot be changed; uninstall and re-install\"]}"
        );
        // Nested bodies for the ISO-list fields.
        assert_eq!(
            nested_field_error_body("rdates", ISO_LIST_NOT_ARRAY),
            "{\"rdates\":{\"rdates\":\"must be a JSON array of ISO 8601 datetime strings\"}}"
        );
        assert_eq!(
            nested_field_error_body("exdates", &iso_list_too_long()),
            "{\"exdates\":{\"exdates\":\"must contain at most 256 entries\"}}"
        );
    }

    #[test]
    fn body_escaping_matches_serde_json() {
        // Every body the helpers emit must equal `serde_json` rendering of
        // the same logical value.
        for message in [
            "plain",
            "quote\"back\\slash",
            "new\nline\ttab\rcr",
            "c0\x00\x07\x0b\x0c\x1fcontrols",
            "del\x7fkept",
            "unicodeé😀\u{a0}",
            "o'clock",
        ] {
            let flat = field_error_body("f", message);
            let expected = serde_json::to_string(&serde_json::json!({"f": [message]})).unwrap();
            assert_eq!(flat, expected, "message {message:?}");
            let nested = nested_field_error_body("f", message);
            let expected =
                serde_json::to_string(&serde_json::json!({"f": {"f": message}})).unwrap();
            assert_eq!(nested, expected, "message {message:?}");
        }
    }

    #[test]
    fn py_repr_vectors() {
        // Quote choice + short escapes + the control ladder, pinned
        // against live CPython `repr` outputs.
        for (input, expected) in [
            ("abc", "'abc'"),
            ("", "''"),
            ("a'b", "\"a'b\""),
            // Under double quotes only the quote char differs: backslash,
            // controls and nonprintables still escape.
            ("a'b\nc", "\"a'b\\nc\""),
            ("a'b\\c", "\"a'b\\\\c\""),
            ("a'b\x00c", "\"a'b\\x00c\""),
            ("'\u{a0}", "\"'\\xa0\""),
            ("a\"b", "'a\"b'"),
            ("'\"", "'\\'\"'"),
            ("a\\b", "'a\\\\b'"),
            ("a\nb", "'a\\nb'"),
            ("a\rb", "'a\\rb'"),
            ("a\tb", "'a\\tb'"),
            ("a\x00b", "'a\\x00b'"),
            ("a\x07b", "'a\\x07b'"),
            ("a\x0bb", "'a\\x0bb'"),
            ("a\x0cb", "'a\\x0cb'"),
            ("a\x1fb", "'a\\x1fb'"),
            ("\x7fx", "'\\x7fx'"),
            ("\u{a0}x", "'\\xa0x'"),
            ("\u{ad}x", "'\\xadx'"),
            ("\u{202f}x", "'\\u202fx'"),
            ("\u{2028}x", "'\\u2028x'"),
            ("\u{2029}x", "'\\u2029x'"),
            ("\u{3000}x", "'\\u3000x'"),
            ("\u{ad}", "'\\xad'"),
            ("\u{200b}x", "'\\u200bx'"),
            ("\u{feff}x", "'\\ufeffx'"),
            ("\u{e000}x", "'\\ue000x'"),
            ("éx", "'éx'"),
            ("😀x", "'😀x'"),
            ("中", "'中'"),
            ("€", "'€'"),
            ("́x", "'́x'"),
            ("\u{fdd0}x", "'\\ufdd0x'"),
            ("\u{fffe}x", "'\\ufffex'"),
            ("\u{ffff}x", "'\\uffffx'"),
            ("\u{10000}x", "'\u{10000}x'"),
            // Astral Cf (shorthand / musical / tag controls).
            ("\u{1bca0}x", "'\\U0001bca0x'"),
            ("\u{1d173}x", "'\\U0001d173x'"),
            ("\u{e0001}", "'\\U000e0001'"),
        ] {
            assert_eq!(py_repr_str(input), expected, "input {input:?}");
        }
        // `\\U` ladder above the BMP (private-use astral).
        assert_eq!(py_repr_str("\u{f0000}"), "'\\U000f0000'");
    }
}
