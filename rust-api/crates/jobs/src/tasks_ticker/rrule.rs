//! RRULE helpers for the project scheduler.
//!
//! Port of `pi_dash/bgtasks/_rrule.py` (all 406 lines; Django-free by
//! design). Three responsibilities, one topological closure:
//!
//! 1. [`cron_to_rrule`] — convert a 5-field cron string into the RRULE
//!    string used by the migration. Only the cron flavors production
//!    actually has are handled.
//! 2. [`validate_rrule_string`] — our own constraints on an RRULE string
//!    before persisting it. `FREQ=SECONDLY` is rejected (DoS vector against
//!    the Beat tick); the effective fire rate must be >= 1 min.
//! 3. [`next_fire_from_rrule`] / [`occurrences_between`] — expand an RRULE
//!    bundle (`dtstart`, `tzid`, `rrule`, `rdates`, `exdates`) into fire
//!    times. Return `None` on parse error (the caller treats that as a
//!    configuration error and disables the binding).
//!
//! Translation notes (verified against live `dateutil` 2.9.0 while porting):
//!
//! - Expansion runs on the [`rrule`](https://docs.rs/rrule) crate, a port
//!   of the same `dateutil` recurrence engine, with the `exrule` and
//!   `by-easter` features enabled so embedded `EXRULE`/`BYEASTER` behave as
//!   in Python. The wrapper keeps the Python boundary semantics the engine
//!   does not own: strictly-after-`now` (the engine's `after` filter is
//!   boundary-inclusive), lazy window iteration with `cap` + `has_more`,
//!   single-shot-at-`dtstart`, and `tzid`-is-informational.
//! - Validation is a hand port of `dateutil`'s `_parse_rfc`/`_parse_rfc_rrule`
//!   (uppercase-first, whitespace-split lines, single/multi branch, exact
//!   `invalid RRULE: …` / `unknown parameter …` / `unsupported …` texts) so
//!   every rejection message matches byte for byte.
//! - Ported bug BUG-TICKER-1 (`_rrule.py:323,395,400`): on the single-shot
//!   paths a *naive* exdate never excludes, because tuple membership
//!   compares an aware `dtstart` against a naive exdate. That shape is
//!   unrepresentable here — every instant is a `DateTime<Utc>` — so a naive
//!   exdate cannot arrive and every exdate on these paths is exact. The
//!   aware-exdate behavior (exact-instant exclusion) is preserved and pinned
//!   by the `*-aware-exdate-*` golden vectors.
//! - `validate_rrule_string` drops Python's `dtstart` anchor parameter:
//!   Django's anchor is always aware (`dtstart or now(utc)`), and only its
//!   awareness feeds the verdict (a naive `UNTIL` under an aware anchor is
//!   rejected), so the port treats the anchor as aware unless the rule text
//!   carries its own naive `DTSTART` line, which `rrulestr` substitutes.
//! - Naive datetimes are normalized to UTC at the caller boundary (Python
//!   does `dtstart.replace(tzinfo=utc)` inline); this module takes
//!   `DateTime<Utc>` throughout.

use std::fmt;

use chrono::{DateTime, Utc};

// ---------------------------------------------------------------------------
// errors

/// Raised when a cron expression can't be losslessly converted to RRULE.
///
/// Mirrors `CronConversionError(ValueError)`; the message text matches the
/// Python raise sites byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronConversionError(String);

impl CronConversionError {
    /// The exact message the Python raise site produces.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CronConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CronConversionError {}

/// Raised when an RRULE string violates Pi Dash's constraints.
///
/// Mirrors `RRuleValidationError(ValueError)`; the message text matches the
/// Python raise sites byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RRuleValidationError(String);

impl RRuleValidationError {
    /// The exact message the Python raise site produces.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RRuleValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RRuleValidationError {}

// ---------------------------------------------------------------------------
// Python string repr

/// Python `repr` of a string: single-quoted unless it contains a single
/// quote and no double quote (then double-quoted), with backslash escapes.
/// Only used for the `{cron_expr!r}` / `{piece!r}` fragments of the cron
/// error messages.
fn py_repr_str(s: &str) -> String {
    fn escape_into(out: &mut String, s: &str, quote: char) {
        for c in s.chars() {
            match c {
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
    }

    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    escape_into(&mut out, s, quote);
    out.push(quote);
    out
}

// ---------------------------------------------------------------------------
// cron parsing

/// RFC 5545 day-of-week codes for cron DOW (0=Sunday in cron, mapped to SU).
const DOW_TO_BYDAY: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

/// One parsed cron field: `None` (= `*`) or a sorted list of ints.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CronField {
    values: Option<Vec<i64>>,
    /// True when the field came in as `*/N` (interval semantics), so the
    /// FREQ picker can emit `INTERVAL=N` for `FREQ=MINUTELY` instead of a
    /// huge `BYMINUTE` list.
    is_interval: bool,
    interval: Option<i64>,
}

/// Error shapes of `_parse_cron_field`, rendered by the caller into the
/// exact Python message.
enum FieldError {
    /// `empty cron field`
    Empty,
    /// `bad interval: {spec!r}` / `interval must be positive: {spec!r}`
    Interval(String),
    /// `bad step: {step!r}` / `step must be positive: {step!r}`
    Step(String),
    /// `bad range: {piece!r}`
    Range(String),
    /// `bad value: {piece!r}`
    Value(String),
    /// `value out of range [{lo}..{hi}]: {piece!r}` (piece is post-`/`).
    OutOfRange { lo: i64, hi: i64, piece: String },
}

impl FieldError {
    fn message(self) -> String {
        match self {
            FieldError::Empty => "empty cron field".to_owned(),
            FieldError::Interval(spec) => spec,
            FieldError::Step(spec) => spec,
            FieldError::Range(spec) => spec,
            FieldError::Value(spec) => spec,
            FieldError::OutOfRange { lo, hi, piece } => {
                format!("value out of range [{lo}..{hi}]: {}", py_repr_str(&piece))
            }
        }
    }
}

/// Parse one cron integer the way Python's `int()` does for these inputs:
/// surrounding whitespace and a leading `+`/`-` are accepted. Values that
/// overflow `i64` are reported as out-of-range (Python's unbounded `int`
/// would proceed to the range check and fail there too).
fn parse_cron_int(piece: &str) -> Option<i64> {
    // Underscore digit separators (`int("1_0") == 10` in Python) are not
    // honored: no stored rule contains them.
    piece.trim().parse::<i64>().ok()
}

fn parse_cron_field(spec: &str, lo: i64, hi: i64) -> Result<CronField, FieldError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(FieldError::Empty);
    }
    if spec == "*" {
        return Ok(CronField {
            values: None,
            is_interval: false,
            interval: None,
        });
    }

    // `*/N` — interval form. Keep the interval info distinct so the FREQ
    // picker below can emit `INTERVAL=N` instead of a long value list.
    if let Some(rest) = spec.strip_prefix("*/") {
        let interval = parse_cron_int(rest)
            .ok_or_else(|| FieldError::Interval(format!("bad interval: {}", py_repr_str(spec))))?;
        if interval <= 0 {
            return Err(FieldError::Interval(format!(
                "interval must be positive: {}",
                py_repr_str(spec)
            )));
        }
        let values: Vec<i64> = (lo..=hi).step_by(interval as usize).collect();
        return Ok(CronField {
            values: Some(values),
            is_interval: true,
            interval: Some(interval),
        });
    }

    let mut values: Vec<i64> = Vec::new();
    for raw_piece in spec.split(',') {
        let raw_piece = raw_piece.trim();
        let (piece, step) = match raw_piece.split_once('/') {
            Some((base, step_s)) => {
                let step = parse_cron_int(step_s).ok_or_else(|| {
                    FieldError::Step(format!("bad step: {}", py_repr_str(step_s.trim())))
                })?;
                if step <= 0 {
                    return Err(FieldError::Step(format!(
                        "step must be positive: {}",
                        py_repr_str(step_s.trim())
                    )));
                }
                (base.trim(), step)
            }
            None => (raw_piece, 1),
        };
        let (start, end) = if piece == "*" {
            (lo, hi)
        } else if let Some((start_s, end_s)) = piece.split_once('-') {
            let start = parse_cron_int(start_s)
                .ok_or_else(|| FieldError::Range(format!("bad range: {}", py_repr_str(piece))))?;
            let end = parse_cron_int(end_s)
                .ok_or_else(|| FieldError::Range(format!("bad range: {}", py_repr_str(piece))))?;
            (start, end)
        } else {
            match parse_cron_int(piece) {
                Some(n) => (n, n),
                None => {
                    return Err(FieldError::Value(format!(
                        "bad value: {}",
                        py_repr_str(piece)
                    )));
                }
            }
        };
        if start < lo || end > hi || start > end {
            return Err(FieldError::OutOfRange {
                lo,
                hi,
                piece: piece.to_owned(),
            });
        }
        let mut v = start;
        while v <= end {
            values.push(v);
            v += step;
        }
    }
    // Dedup + sort. cron is set-valued; "1,1,2" is just "1,2".
    values.sort_unstable();
    values.dedup();
    Ok(CronField {
        values: Some(values),
        is_interval: false,
        interval: None,
    })
}

/// Convert a 5-field cron expression to an RFC 5545 RRULE string.
///
/// Supports the cron grammar production actually has: `*`, `N`, `N-M`,
/// `*/N`, `N-M/K`, and comma-separated lists.
///
/// Raises [`CronConversionError`] for a wrong field count, for DOM and DOW
/// both constrained (Vixie OR semantics can't be expressed as a single
/// rule), and for unsupported grammar (day names like `MON`, `@yearly`
/// shortcuts).
pub fn cron_to_rrule(cron_expr: &str) -> Result<String, CronConversionError> {
    let parts: Vec<&str> = cron_expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(CronConversionError(format!(
            "cron must have exactly 5 fields, got {}: {}",
            parts.len(),
            py_repr_str(cron_expr)
        )));
    }
    let minute = parse_cron_field(parts[0], 0, 59).map_err(|e| CronConversionError(e.message()))?;
    let hour = parse_cron_field(parts[1], 0, 23).map_err(|e| CronConversionError(e.message()))?;
    let dom = parse_cron_field(parts[2], 1, 31).map_err(|e| CronConversionError(e.message()))?;
    let month = parse_cron_field(parts[3], 1, 12).map_err(|e| CronConversionError(e.message()))?;
    // cron DOW: 0 or 7 = Sunday. Normalize 7 → 0 before parsing. This is a
    // whole-string replace (so `17` becomes `10` and fails the range check
    // as `'10'`), ported literally.
    let dow_spec;
    let dow_raw = if parts[4] != "*" {
        dow_spec = parts[4].replace('7', "0");
        dow_spec.as_str()
    } else {
        parts[4]
    };
    let dow = parse_cron_field(dow_raw, 0, 6).map_err(|e| CronConversionError(e.message()))?;

    let dom_constrained = dom.values.is_some();
    let dow_constrained = dow.values.is_some();
    if dom_constrained && dow_constrained {
        return Err(CronConversionError(format!(
            "cron has both day-of-month and day-of-week set; Vixie OR semantics \
             can't be expressed in a single RRULE: {}",
            py_repr_str(cron_expr)
        )));
    }

    // Determine FREQ.
    let freq = if dow_constrained {
        "WEEKLY"
    } else if dom_constrained {
        "MONTHLY"
    } else if month.values.is_some() {
        // Month set but DOM unset = "every day in this month every year" —
        // ambiguous and uncommon, emit YEARLY and let the BY* clauses define it.
        "YEARLY"
    } else if hour.values.is_some() || (minute.values.is_some() && !minute.is_interval) {
        // Specific hour(s) and/or specific minute(s) → DAILY or HOURLY.
        if hour.values.is_none() {
            "HOURLY"
        } else {
            "DAILY"
        }
    } else {
        "MINUTELY"
    };

    let mut parts_out = vec![format!("FREQ={freq}")];
    let join = |values: &[i64]| {
        values
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };

    // INTERVAL — for MINUTELY/HOURLY we honor the */N form on the driving
    // field. For other FREQs the driving cadence comes from the BY* clauses.
    if freq == "MINUTELY" {
        if minute.is_interval {
            if let Some(interval) = minute.interval {
                parts_out.push(format!("INTERVAL={interval}"));
            }
        } else if let Some(values) = &minute.values {
            if values.len() > 1 {
                parts_out.push(format!("BYMINUTE={}", join(values)));
            }
        }
        // else: minute=*, fires every minute, INTERVAL defaults to 1
    } else if freq == "HOURLY" {
        if hour.is_interval {
            if let Some(interval) = hour.interval {
                parts_out.push(format!("INTERVAL={interval}"));
            }
        }
        if let Some(values) = &minute.values {
            parts_out.push(format!("BYMINUTE={}", join(values)));
        }
    } else {
        // DAILY/WEEKLY/MONTHLY/YEARLY all use BYHOUR/BYMINUTE for time.
        if let Some(values) = &hour.values {
            parts_out.push(format!("BYHOUR={}", join(values)));
        }
        if let Some(values) = &minute.values {
            parts_out.push(format!("BYMINUTE={}", join(values)));
        }
    }

    if freq == "WEEKLY" {
        if let Some(values) = &dow.values {
            let days = values
                .iter()
                .map(|d| DOW_TO_BYDAY[*d as usize])
                .collect::<Vec<_>>()
                .join(",");
            parts_out.push(format!("BYDAY={days}"));
        }
    }
    if freq == "MONTHLY" || freq == "YEARLY" {
        if let Some(values) = &dom.values {
            parts_out.push(format!("BYMONTHDAY={}", join(values)));
        }
    }
    if freq != "MONTHLY" {
        if let Some(values) = &month.values {
            parts_out.push(format!("BYMONTH={}", join(values)));
        }
    }

    Ok(parts_out.join(";"))
}

// ---------------------------------------------------------------------- validation

/// FREQ values we allow. SECONDLY is rejected.
const ALLOWED_FREQS: [&str; 6] = ["MINUTELY", "HOURLY", "DAILY", "WEEKLY", "MONTHLY", "YEARLY"];

/// The allowlist as Python's `sorted(_ALLOWED_FREQS)` renders it; part of
/// the SECONDLY rejection message byte for byte.
const ALLOWED_FREQS_SORTED_REPR: &str =
    "['DAILY', 'HOURLY', 'MINUTELY', 'MONTHLY', 'WEEKLY', 'YEARLY']";

/// FREQ values `dateutil` parses (its `_freq_map` keys). Anything else is
/// `invalid 'FREQ': …`; `SECONDLY` parses and is then rejected by us.
const DATEUTIL_FREQS: [&str; 7] = [
    "YEARLY", "MONTHLY", "WEEKLY", "DAILY", "HOURLY", "MINUTELY", "SECONDLY",
];

/// `dateutil` weekday codes (`_weekday_map` keys).
const WEEKDAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

/// Properties `dateutil` parses as a single int (`_handle_int`).
fn is_int_prop(name: &str) -> bool {
    matches!(name, "INTERVAL" | "COUNT")
}

/// Properties `dateutil` parses as an int list (`_handle_int_list`).
fn is_int_list_prop(name: &str) -> bool {
    matches!(
        name,
        "BYSETPOS"
            | "BYMONTH"
            | "BYMONTHDAY"
            | "BYYEARDAY"
            | "BYEASTER"
            | "BYWEEKNO"
            | "BYHOUR"
            | "BYMINUTE"
            | "BYSECOND"
    )
}

/// A parsed single RRULE value. FREQ and INTERVAL drive our own checks;
/// the `until`/`bysetpos`/`by*` values are kept so the `rrule.__init__`
/// semantic checks (`check_rrule_init_semantics`) can run exactly as in
/// `dateutil`. Properties that only affect validity (COUNT, BYDAY, …) are
/// validated during parsing and not retained.
#[derive(Debug, Default)]
struct ParsedRule {
    freq: Option<String>,
    interval: Option<i64>,
    /// `Some(true)` when an `UNTIL` value carries a zone (`Z`/offset),
    /// `Some(false)` when it is naive, `None` when absent.
    until: Option<bool>,
    bysetpos: Option<Vec<i64>>,
    byhour: Option<Vec<i64>>,
    byminute: Option<Vec<i64>>,
    bysecond: Option<Vec<i64>>,
}

/// Python-`int()`-shaped integer parse for RRULE values: surrounding
/// whitespace and a leading `+`/`-` are accepted (the values reaching here
/// never contain whitespace anyway — `dateutil` splits lines first).
fn parse_rrule_int(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok()
}

/// Lenient UNTIL/date-value parse. `dateutil` runs these through its fuzzy
/// date parser; the stored rules only ever carry ISO / iCal forms, so the
/// formats below accept every such value. Anything else is invalid,
/// matching the verdict (if not the internal wording) of `dateutil`.
fn parse_loose_datetime(value: &str) -> bool {
    parse_loose_datetime_aware(value).is_some()
}

/// [`parse_loose_datetime`] plus the parsed value's timezone-awareness:
/// `Some(true)` when it carries a zone (`Z`/offset), `Some(false)` when it
/// is naive, `None` when unparseable. `rrule.__init__` compares the
/// awareness of `DTSTART` and `UNTIL` (`rrule.py:465-472`), so the
/// distinction the boolean form discards is needed here.
fn parse_loose_datetime_aware(value: &str) -> Option<bool> {
    if chrono::DateTime::parse_from_rfc3339(value).is_ok() {
        return Some(true);
    }
    const FORMATS: [(&str, bool); 7] = [
        ("%Y%m%dT%H%M%SZ", true),
        ("%Y%m%dT%H%M%S", false),
        ("%Y%m%d", false),
        ("%Y-%m-%dT%H:%M:%S%.f", false),
        ("%Y-%m-%dT%H:%M:%S", false),
        ("%Y-%m-%d %H:%M:%S", false),
        ("%Y-%m-%d", false),
    ];
    FORMATS.iter().find_map(|(fmt, aware)| {
        if chrono::NaiveDateTime::parse_from_str(value, fmt).is_ok()
            || chrono::NaiveDate::parse_from_str(value, fmt).is_ok()
        {
            Some(*aware)
        } else {
            None
        }
    })
}

/// Parse one paren-form `BYDAY` item (`TH(+1)`) the way `dateutil`'s
/// `_handle_BYWEEKDAY` does: `w` is the text before the first `(`,
/// `n` is `int` of the rest minus its last char (dropped unconditionally).
/// Returns the uppercased weekday code and the ordinal, or `None` when
/// `dateutil` would raise (the `int` fails or the code is unknown).
///
/// Shared by validation (`check_byday`) and expansion canonicalization so
/// the two cannot drift: every item the validator accepts must rewrite to
/// something the crate parses, and every item it rejects must stay a
/// parse error.
fn parse_paren_byday(wday: &str) -> Option<(String, i64)> {
    // `splt = wday.split('(')`; `w = splt[0]`; `n = int(splt[1][:-1])`.
    let mut parts = wday.split('(');
    let w = parts.next().unwrap_or("");
    let rest = parts.next()?;
    let n_str: String = {
        let mut chars: Vec<char> = rest.chars().collect();
        chars.pop();
        chars.into_iter().collect()
    };
    let n = parse_rrule_int(&n_str)?;
    // Uppercase like validation does (`dateutil` uppercases the whole
    // input before parsing), so raw-case expansion matches it.
    let w = w.to_uppercase();
    if !WEEKDAYS.contains(&w.as_str()) {
        return None;
    }
    Some((w, n))
}

/// Check one `BYDAY`/`BYWEEKDAY` value the way `dateutil`'s
/// `_handle_BYWEEKDAY` does: comma-separated items of either `+1MO` /
/// `1MO` / `MO` form or `TH(+1)` paren form.
fn check_byday(value: &str) -> bool {
    value.split(',').all(|wday| {
        if wday.contains('(') {
            // `TH(+1)` form — shared parse with expansion canonicalization
            // (the input is already uppercased, so the verdict is unchanged).
            parse_paren_byday(wday).is_some()
        } else {
            // `+1MO` form: leading `[+-0-9]*` is the ordinal, the rest the code.
            let idx = wday
                .char_indices()
                .find(|(_, c)| !"+-0123456789".contains(*c))
                .map(|(i, _)| i)
                .unwrap_or(wday.len());
            let (n_str, w) = wday.split_at(idx);
            if !n_str.is_empty() && parse_rrule_int(n_str).is_none() {
                return false;
            }
            WEEKDAYS.contains(&w)
        }
    })
}

/// Parse one bare or `RRULE:`-prefixed rule line (`_parse_rfc_rrule`).
/// `line` is already uppercased. Returns the message fragment for
/// `invalid RRULE: …` on failure.
fn parse_single_rule(line: &str) -> Result<ParsedRule, String> {
    let value = if line.contains(':') {
        // `name, value = line.split(':')` — no maxsplit, so a second colon
        // is "too many values to unpack (expected 2)".
        let mut colon_parts = line.split(':');
        let name = colon_parts.next().unwrap_or("");
        let value = match colon_parts.next() {
            Some(v) => v,
            None => return Err("unknown parameter name".to_owned()),
        };
        if colon_parts.next().is_some() {
            return Err("too many values to unpack (expected 2)".to_owned());
        }
        if name != "RRULE" {
            return Err("unknown parameter name".to_owned());
        }
        value
    } else {
        line
    };

    let mut parsed = ParsedRule::default();
    for pair in value.split(';') {
        // `name, value = pair.split('=')` — same unpack strictness.
        let mut eq_parts = pair.split('=');
        let name = eq_parts.next().unwrap_or("");
        let prop_value = match eq_parts.next() {
            Some(v) => v,
            None => {
                return Err("not enough values to unpack (expected 2, got 1)".to_owned());
            }
        };
        if eq_parts.next().is_some() {
            return Err("too many values to unpack (expected 2)".to_owned());
        }
        if name == "FREQ" {
            if !DATEUTIL_FREQS.contains(&prop_value) {
                return Err(format!("invalid 'FREQ': {prop_value}"));
            }
            parsed.freq = Some(prop_value.to_owned());
        } else if is_int_prop(name) {
            match parse_rrule_int(prop_value) {
                Some(n) => {
                    if name == "INTERVAL" {
                        parsed.interval = Some(n);
                    }
                }
                None => return Err(format!("invalid '{name}': {prop_value}")),
            }
        } else if is_int_list_prop(name) {
            match prop_value
                .split(',')
                .map(parse_rrule_int)
                .collect::<Option<Vec<i64>>>()
            {
                Some(values) => match name {
                    "BYSETPOS" => parsed.bysetpos = Some(values),
                    "BYHOUR" => parsed.byhour = Some(values),
                    "BYMINUTE" => parsed.byminute = Some(values),
                    "BYSECOND" => parsed.bysecond = Some(values),
                    _ => {}
                },
                None => return Err(format!("invalid '{name}': {prop_value}")),
            }
        } else if name == "UNTIL" {
            match parse_loose_datetime_aware(prop_value) {
                Some(aware) => parsed.until = Some(aware),
                None => return Err(format!("invalid 'UNTIL': {prop_value}")),
            }
        } else if name == "WKST" {
            if !WEEKDAYS.contains(&prop_value) {
                return Err(format!("invalid 'WKST': {prop_value}"));
            }
        } else if name == "BYDAY" || name == "BYWEEKDAY" {
            if !check_byday(prop_value) {
                return Err(format!("invalid '{name}': {prop_value}"));
            }
        } else {
            return Err(format!("unknown parameter '{name}'"));
        }
    }
    Ok(parsed)
}

/// Apply the FREQ allowlist + INTERVAL floor to a parsed rule.
fn check_freq_interval(parsed: &ParsedRule) -> Result<(), RRuleValidationError> {
    // Without FREQ the `rrule(dtstart, **kwargs)` constructor itself fails.
    // The qualifier is `rrule.__init__` (confirmed against live dateutil:
    // `TypeError: rrule.__init__() missing 1 required positional argument:
    // 'freq'`); the message below keeps it byte for byte.
    let freq = parsed.freq.as_deref().ok_or_else(|| {
        RRuleValidationError(
            "invalid RRULE: rrule.__init__() missing 1 required positional argument: 'freq'"
                .to_owned(),
        )
    })?;
    if !ALLOWED_FREQS.contains(&freq) {
        return Err(RRuleValidationError(format!(
            "FREQ={freq} is not allowed (allowed: {ALLOWED_FREQS_SORTED_REPR})"
        )));
    }
    let interval = match parsed.interval.unwrap_or(1) {
        0 => 1,
        n => n,
    };
    if interval < 1 {
        return Err(RRuleValidationError(format!(
            "INTERVAL must be >= 1, got {interval}"
        )));
    }
    Ok(())
}

/// The `rrule.__init__` semantic checks `rrulestr` runs for us on the Django
/// side (`dateutil/rrule.py:465-499,838-848`), returning the bare
/// `dateutil` message so the caller wraps it as `invalid RRULE: …`. Only the
/// properties the stored subset can carry are checked: `UNTIL` awareness
/// against the implicit aware anchor (or an explicit `DTSTART`), the
/// `BYSETPOS` range, and the
/// `BYHOUR`/`BYMINUTE`/`BYSECOND` ranges (which `dateutil` enforces by
/// building a `datetime.time` per `(hour, minute, second)` combination,
/// lowest FREQ below `HOURLY`). `BYMONTH`, `BYMONTHDAY`, `BYYEARDAY` and
/// `BYWEEKNO` have no range check in `rrule.__init__` — confirmed against a
/// live `dateutil` — so they are deliberately not checked here.
fn check_rrule_init_semantics(parsed: &ParsedRule, anchor_aware: bool) -> Result<(), String> {
    // `if self._dtstart and self._until:` — the awareness must match
    // (`rrule.py:462-472`); either mismatch direction raises the same
    // message. The anchor is the aware `datetime.now(tz=utc)` unless the
    // rule text carries its own DTSTART, which `rrulestr` substitutes.
    if let Some(until_aware) = parsed.until {
        if anchor_aware != until_aware {
            return Err(
                "RRULE UNTIL values must be specified in UTC when DTSTART is timezone-aware"
                    .to_owned(),
            );
        }
    }

    // bysetpos must be in [-366, -1] or [1, 366].
    if let Some(positions) = &parsed.bysetpos {
        for pos in positions {
            if *pos == 0 || !(-366..=366).contains(pos) {
                return Err("bysetpos must be between 1 and 366, or between -366 and -1".to_owned());
            }
        }
    }

    // Below HOURLY, `dateutil` materializes `datetime.time(hour, minute,
    // second)` for every combination; CPython raises the range errors.
    // `dateutil` sorts and de-duplicates each set, and an absent list falls
    // back to the anchor's (always in-range) components — a single in-range
    // sentinel models that without changing which invalid value is hit
    // first in the h→m→s iteration order.
    if matches!(
        parsed.freq.as_deref(),
        Some("YEARLY" | "MONTHLY" | "WEEKLY" | "DAILY")
    ) {
        let hours = sorted_set(parsed.byhour.as_deref().unwrap_or(&[0]));
        let minutes = sorted_set(parsed.byminute.as_deref().unwrap_or(&[0]));
        let seconds = sorted_set(parsed.bysecond.as_deref().unwrap_or(&[0]));
        for hour in &hours {
            if !(0..=23).contains(hour) {
                return Err("hour must be in 0..23".to_owned());
            }
            for minute in &minutes {
                if !(0..=59).contains(minute) {
                    return Err("minute must be in 0..59".to_owned());
                }
                for second in &seconds {
                    if !(0..=59).contains(second) {
                        return Err("second must be in 0..59".to_owned());
                    }
                }
            }
        }
    }
    Ok(())
}

/// `dateutil` stores each `BYxxx` list as a sorted set.
fn sorted_set(values: &[i64]) -> Vec<i64> {
    let mut out = values.to_vec();
    out.sort_unstable();
    out.dedup();
    out
}

/// `rrulestr` runs `rrule.__init__` before Django applies its own FREQ/
/// INTERVAL constraints, so the constructor's semantic checks come first; a
/// missing FREQ is the constructor's `TypeError` and wins over both.
fn check_parsed_rule(parsed: &ParsedRule, anchor_aware: bool) -> Result<(), RRuleValidationError> {
    if parsed.freq.is_none() {
        return check_freq_interval(parsed);
    }
    check_rrule_init_semantics(parsed, anchor_aware)
        .map_err(|msg| RRuleValidationError(format!("invalid RRULE: {msg}")))?;
    check_freq_interval(parsed)
}

/// Whether `rrulestr` would build an aware anchor from a `DTSTART` line.
/// A `Z`/offset value is already aware; a `TZID=` parameter supplies the
/// zone for an otherwise naive value. `check_date_line` has already
/// validated the shape.
fn dtstart_aware(parms: &[&str], value: &str) -> bool {
    parms.iter().any(|parm| parm.starts_with("TZID="))
        || parse_loose_datetime_aware(value) == Some(true)
}

/// Check the date values of an `RDATE`/`EXDATE`/`DTSTART` line
/// (`_parse_date_value`): `TZID=` parms are looked up (accepted here),
/// `VALUE=DATE[-TIME]` may appear at most once, anything else is unsupported.
fn check_date_line(prop: &str, parms: &[&str], value: &str) -> Result<bool, String> {
    let mut value_seen = false;
    for parm in parms {
        if parm.starts_with("TZID=") {
            continue;
        }
        if *parm == "VALUE=DATE-TIME" || *parm == "VALUE=DATE" {
            if value_seen {
                return Err(format!("Duplicate value parameter found in: {parm}"));
            }
            value_seen = true;
            continue;
        }
        if prop == "RDATE" {
            return Err(format!("unsupported RDATE parm: {parm}"));
        }
        return Err(format!("unsupported parm: {parm}"));
    }
    let dates: Vec<&str> = value.split(',').collect();
    if prop == "DTSTART" && dates.len() != 1 {
        return Err(format!("Multiple DTSTART values specified:{value}"));
    }
    for datestr in dates {
        if !parse_loose_datetime(datestr) {
            // `dateutil`'s fuzzy parser wording lives here; only reachable
            // for non-ISO date values, which no stored rule carries.
            return Err(format!("unparseable date value: {datestr}"));
        }
    }
    Ok(true)
}

/// Raise [`RRuleValidationError`] if the RRULE violates our constraints.
///
/// Constraints: parses cleanly the way `dateutil.rrule.rrulestr` parses it
/// (including `rrule.__init__`'s semantic checks: `UNTIL` awareness against
/// the aware anchor, the `BYSETPOS` range, the `BYHOUR`/`BYMINUTE`/`BYSECOND`
/// ranges;
/// FREQ is one of MINUTELY/HOURLY/DAILY/WEEKLY/MONTHLY/YEARLY; the effective
/// fire rate is >= 1 minute (`INTERVAL >= 1`, with `0` meaning the default
/// `1` as in `dateutil`).
///
/// The empty string is valid: `rrule_str` is optional in the model and
/// callers pass `""` for single-shot bindings (which fire once at `dtstart`).
pub fn validate_rrule_string(rrule_str: &str) -> Result<(), RRuleValidationError> {
    if rrule_str.is_empty() {
        return Ok(()); // empty = single-shot, fine.
    }
    // `dateutil` uppercases the whole input before parsing.
    let upper = rrule_str.to_uppercase();
    if upper.trim().is_empty() {
        return Err(RRuleValidationError(
            "invalid RRULE: empty string".to_owned(),
        ));
    }
    // Without `unfold`, `dateutil` splits lines on any whitespace run.
    let lines: Vec<&str> = upper.split_whitespace().collect();
    let invalid = |msg: String| RRuleValidationError(format!("invalid RRULE: {msg}"));

    if lines.len() == 1 && (!lines[0].contains(':') || lines[0].starts_with("RRULE:")) {
        let parsed = parse_single_rule(lines[0]).map_err(invalid)?;
        return check_parsed_rule(&parsed, true);
    }

    // Multi-property mode. `dateutil` assembles a set (which never carries
    // `_freq`, so the verdict is "missing FREQ") when there are several
    // `RRULE` values or any `RDATE`/`EXDATE`/`EXRULE` lines — a lone
    // `DTSTART` line does *not* force a set. Each member parses first, so a
    // malformed member fails as invalid before assembly.
    let mut rrule_count = 0usize;
    let mut forces_set = false;
    let mut only_rule: Option<ParsedRule> = None;
    let mut anchor_aware = true;
    for line in &lines {
        if !line.contains(':') {
            let parsed = parse_single_rule(line).map_err(invalid)?;
            rrule_count += 1;
            only_rule = Some(parsed);
            continue;
        }
        let (head, value) = line.split_once(':').unwrap_or((line, ""));
        let mut head_parts = head.split(';');
        let prop = head_parts.next().unwrap_or("");
        let parms: Vec<&str> = head_parts.collect();
        match prop {
            "RRULE" => {
                if let Some(parm) = parms.first() {
                    return Err(invalid(format!("unsupported RRULE parm: {parm}")));
                }
                let parsed = parse_single_rule(value).map_err(invalid)?;
                rrule_count += 1;
                only_rule = Some(parsed);
            }
            "EXRULE" => {
                if let Some(parm) = parms.first() {
                    return Err(invalid(format!("unsupported EXRULE parm: {parm}")));
                }
                parse_single_rule(value).map_err(invalid)?;
                forces_set = true;
            }
            "RDATE" | "EXDATE" => {
                check_date_line(prop, &parms, value).map_err(invalid)?;
                forces_set = true;
            }
            "DTSTART" => {
                check_date_line(prop, &parms, value).map_err(invalid)?;
                anchor_aware = dtstart_aware(&parms, value);
            }
            _ => {
                return Err(invalid(format!("unsupported property: {prop}")));
            }
        }
    }
    if rrule_count == 1 && !forces_set {
        if let Some(parsed) = &only_rule {
            return check_parsed_rule(parsed, anchor_aware);
        }
    }
    // `rrule_count == 0` (e.g. a lone `DTSTART:` line) hits `rrulevals[0]`
    // in `dateutil`, raising a bare `IndexError` that is *not* an
    // `RRuleValidationError`. A typed `Result` cannot propagate that, so it
    // reports the same verdict as its sibling shapes (lone `RDATE:` line).
    Err(RRuleValidationError("RRULE is missing FREQ".to_owned()))
}

// ---------------------------------------------------------------------- expansion

use rrule::{RRuleSet, Tz};

/// Engine timezone: every instant here is UTC. Callers normalize naive
/// datetimes to UTC before calling (Python does
/// `dt.replace(tzinfo=timezone.utc)` inline at each site).
fn to_engine(dt: DateTime<Utc>) -> chrono::DateTime<Tz> {
    dt.with_timezone(&Tz::UTC)
}

fn from_engine(dt: chrono::DateTime<Tz>) -> DateTime<Utc> {
    dt.with_timezone(&Utc)
}

/// Render `dtstart` as the `DTSTART` line the engine requires
/// (`FromStr for RRuleSet` rejects input without one; `dateutil` falls back
/// to its `dtstart=` argument instead).
fn dtstart_line(dtstart: DateTime<Utc>) -> String {
    format!("DTSTART:{}", dtstart.format("%Y%m%dT%H%M%SZ"))
}

/// Rewrite one `BYDAY`/`BYWEEKDAY` value's paren-form items (`TH(+1)`)
/// into the `[+-]N + weekday` spelling (`1TH`) the `rrule` crate parses.
///
/// `dateutil` accepts both spellings; the crate only understands `[+-]NNDD`
/// and byte-slices the last two bytes, which panics on non-ASCII tails
/// (`MO(1€`) and mis-parses ASCII paren items (`TH(+1)` reads weekday
/// `"1)"`). Non-paren items pass through byte-identical. Returns `None`
/// when an item fails the `dateutil` parse — including `n == 0`, which
/// `dateutil`'s `weekday` rejects ("Can't create weekday with n==0") — so
/// the caller maps it to the parse-error result, exactly as `dateutil`
/// raising does on the Python side.
fn canonicalize_byday_value(value: &str) -> Option<String> {
    let mut items = Vec::new();
    for item in value.split(',') {
        if !item.contains('(') {
            items.push(item.to_owned());
            continue;
        }
        let (w, n) = parse_paren_byday(item)?;
        if n == 0 {
            return None;
        }
        items.push(format!("{n}{w}"));
    }
    Some(items.join(","))
}

/// Canonicalize one whitespace-separated rule token for the crate: rewrite
/// paren-form `BYDAY`/`BYWEEKDAY` items in bare, `RRULE:` and `EXRULE:`
/// lines; every other line passes through untouched. Returns `None` when a
/// `BYDAY` value fails the `dateutil` parse (the crate input would be a
/// parse error there too).
fn canonicalize_token(token: &str) -> Option<String> {
    let (head, value) = match token.split_once(':') {
        Some((head, value)) => (Some(head), value),
        None => (None, token),
    };
    let is_rule = match head {
        None => true,
        Some(head) => {
            let prop = head.split(';').next().unwrap_or("");
            prop.eq_ignore_ascii_case("RRULE") || prop.eq_ignore_ascii_case("EXRULE")
        }
    };
    if !is_rule {
        return Some(token.to_owned());
    }
    let mut pairs = Vec::new();
    for pair in value.split(';') {
        let Some((name, pair_value)) = pair.split_once('=') else {
            pairs.push(pair.to_owned());
            continue;
        };
        if name.eq_ignore_ascii_case("BYDAY") || name.eq_ignore_ascii_case("BYWEEKDAY") {
            pairs.push(format!("{name}={}", canonicalize_byday_value(pair_value)?));
        } else {
            pairs.push(pair.to_owned());
        }
    }
    let body = pairs.join(";");
    match head {
        Some(head) => Some(format!("{head}:{body}")),
        None => Some(body),
    }
}

/// Build the engine set for an RRULE bundle: the rule string plus the
/// extra one-off `rdates` and the `exdates` to skip. Returns `None` on
/// parse error, like Python returning `None` from the `except` path.
///
/// The rule string is whitespace-normalized the way `dateutil` splits lines
/// (so a stray space breaks a rule in both engines); its case is preserved
/// (the engine parses names case-insensitively and `TZID`s are
/// case-sensitive), except paren-form `BYDAY` items, which are rewritten to
/// the crate's `[+-]N + weekday` spelling first. An embedded `DTSTART` line
/// wins over `dtstart`, as in `dateutil`; otherwise `dtstart` anchors the
/// series. The set is only built with members when `rdates`/`exdates` are
/// present — mirroring the `if rdates or exdates` branches — which keeps
/// single-rule input on the plain-rule path in both engines.
fn expansion_set(
    dtstart: DateTime<Utc>,
    rrule_str: &str,
    rdates: &[DateTime<Utc>],
    exdates: &[DateTime<Utc>],
) -> Option<RRuleSet> {
    let tokens: Vec<&str> = rrule_str.split_whitespace().collect();
    let has_dtstart = tokens.iter().any(|token| {
        token
            .split([':', ';'])
            .next()
            .is_some_and(|head| head.eq_ignore_ascii_case("DTSTART"))
    });
    let mut src = String::with_capacity(rrule_str.len() + 32);
    if !has_dtstart {
        src.push_str(&dtstart_line(dtstart));
        src.push('\n');
    }
    let mut rewritten = Vec::with_capacity(tokens.len());
    for token in &tokens {
        rewritten.push(canonicalize_token(token)?);
    }
    src.push_str(&rewritten.join("\n"));
    // The crate's `BYDAY` parser byte-slices without an ASCII guard, so a
    // hostile or half-validated rule could still panic inside the parse; a
    // panic here must degrade to the parse-error result, never crash the
    // caller (request handler, job, or occurrences view).
    let mut set: RRuleSet = std::panic::catch_unwind(|| src.parse()).ok()?.ok()?;
    // `rruleset` assembly only when extras are present, like Python.
    if !rdates.is_empty() || !exdates.is_empty() {
        for rdate in rdates {
            set = set.rdate(to_engine(*rdate));
        }
        for exdate in exdates {
            set = set.exdate(to_engine(*exdate));
        }
    }
    Some(set)
}

/// Return the next datetime the RRULE bundle is due strictly after `now`.
///
/// Returns `None` both on parse error and when the series is legitimately
/// exhausted (a finite COUNT/UNTIL series with nothing left, or a
/// single-shot whose `dtstart` has passed). Callers that need to tell those
/// two cases apart use [`next_fire_with_error`].
pub fn next_fire_from_rrule(
    dtstart: DateTime<Utc>,
    rrule_str: &str,
    tzid: &str,
    rdates: &[DateTime<Utc>],
    exdates: &[DateTime<Utc>],
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    next_fire_with_error(dtstart, rrule_str, tzid, rdates, exdates, now)
        .ok()
        .flatten()
}

/// `next_fire_with_error` (`_rrule.py`): the next fire for the RRULE
/// bundle, telling a parse error apart from an exhausted series.
///
/// - `Ok(Some(next))` — the bundle is valid and fires again after `now`.
/// - `Ok(None)` — the bundle is valid but the series has no occurrence
///   after `now` (finite series exhausted / single-shot already past).
/// - `Err(text)` — the bundle failed to parse. Python returns `str(e)`;
///   every caller only branches on its presence, so the text is the
///   engine's, not dateutil's.
///
/// Semantics:
/// - `dtstart` is the series anchor. If `rrule_str` is empty, the rule
///   fires once at `dtstart` (and only if it's after `now`).
/// - `rdates` are additional one-off firings appended to the series.
/// - `exdates` are firings to skip.
/// - Returned datetime is timezone-aware UTC.
///
/// The `tzid` is informational at this layer — `dateutil` expands `dtstart`
/// as it is (tz-aware UTC). Wall-clock-aware DST semantics are deferred;
/// `tzid` is stored and surfaced in the API but doesn't drive expansion.
pub fn next_fire_with_error(
    dtstart: DateTime<Utc>,
    rrule_str: &str,
    tzid: &str,
    rdates: &[DateTime<Utc>],
    exdates: &[DateTime<Utc>],
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, String> {
    let _ = tzid;
    if rrule_str.is_empty() {
        // Single-shot: fire at dtstart if it's still ahead.
        if dtstart > now && !exdates.contains(&dtstart) {
            return Ok(Some(dtstart));
        }
        // Otherwise check rdates for an explicit one-off after now.
        // (Single-shot rdates are not exdate-filtered, like Python.)
        return Ok(rdates.iter().copied().filter(|d| *d > now).min());
    }
    let Some(set) = expansion_set(dtstart, rrule_str, rdates, exdates) else {
        // The crate refuses rules dateutil expands (PIDASHCONV-1300); the
        // dateutil-faithful fallback expands those, and agrees with the
        // crate on rules dateutil rejects — reported as the parse error.
        return fallback::next_fire_with_error(dtstart, rrule_str, rdates, exdates, now)
            .inspect_err(|error| {
                tracing::warn!(rrule = rrule_str, err = %error, "scheduler.rrule_parse: bad rrule");
            });
    };
    // Strictly after `now`: the engine's `after` filter is
    // boundary-inclusive, so the bound is applied on the lazy iterator.
    let base = to_engine(now);
    Ok(set.into_iter().find(|occ| *occ > base).map(from_engine))
}

/// Expand the RRULE bundle into all occurrences in
/// `[window_start, window_end]`.
///
/// Returns `(occurrences, has_more)`. `has_more` is true if the cap was
/// hit; callers should surface "narrow the date range" to the user.
///
/// The engine iterator is lazy — iteration stops as soon as the window is
/// left or the cap is hit — mirroring the `xafter` loop (Python deliberately
/// avoids `between`, which materialises the full expansion before the cap
/// applies).
#[allow(clippy::too_many_arguments)]
pub fn occurrences_between(
    dtstart: DateTime<Utc>,
    rrule_str: &str,
    tzid: &str,
    rdates: &[DateTime<Utc>],
    exdates: &[DateTime<Utc>],
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
    cap: usize,
) -> (Vec<DateTime<Utc>>, bool) {
    let _ = tzid;
    if rrule_str.is_empty() {
        let mut out = Vec::new();
        if window_start <= dtstart && dtstart <= window_end && !exdates.contains(&dtstart) {
            out.push(dtstart);
        }
        for rdate in rdates {
            if window_start <= *rdate && *rdate <= window_end && !exdates.contains(rdate) {
                out.push(*rdate);
            }
        }
        out.sort();
        return (out, false);
    }
    let Some(set) = expansion_set(dtstart, rrule_str, rdates, exdates) else {
        return fallback::occurrences(
            dtstart,
            rrule_str,
            rdates,
            exdates,
            window_start,
            window_end,
            cap,
        );
    };
    let start = to_engine(window_start);
    let end = to_engine(window_end);
    let mut out = Vec::new();
    for occ in set.into_iter() {
        if occ < start {
            continue;
        }
        if occ > end {
            break;
        }
        out.push(from_engine(occ));
        if out.len() >= cap {
            return (out, true);
        }
    }
    (out, false)
}

// ---------------------------------------------------------------------------
// dateutil fallback expansion (PIDASHCONV-1300)

/// dateutil-faithful expansion for rules the `rrule` crate refuses.
///
/// The crate enforces RFC 5545 restrictions and integer widths that
/// `dateutil` 2.8.2 does not have (`BYMONTHDAY`+`WEEKLY`,
/// `BYYEARDAY`+`DAILY`/`WEEKLY`/`MONTHLY`, `BYWEEKNO` outside `YEARLY`,
/// `BYEASTER` frequency/companions, `BYDAY`-ordinal and `BYSETPOS` ranges,
/// `INTERVAL: u16`, `COUNT: u32`, `UNTIL` before `dtstart`, duplicate
/// properties). There is no public API to bypass the crate's validator, so
/// this module ports the dateutil 2.8.2 engine pieces that matter for
/// expansion — `_parse_rfc` + `_parse_rfc_rrule`, `rrule.__init__`,
/// `rrule._iter`, `_iterinfo`, `rruleset._iter`, `easter.easter` — and is
/// used only when [`expansion_set`] returns `None`.
///
/// Every failure maps to "no expansion" (`None`), mirroring
/// `bgtasks/_rrule.py`'s `except (ValueError, TypeError)`. Three dateutil
/// behaviors are deliberately not reproduced: shapes whose expansion
/// raises `IndexError` (positive out-of-range `BYDAY` ordinals under
/// `MONTHLY`/`YEARLY`, `BYEASTER` past the mask) 500 on Django; giant
/// `INTERVAL`s hang Django in the `fixday` month loop (this port jumps
/// arithmetically); `INTERVAL < 1` hangs or yields one instant forever on
/// Django. Here all three yield `None`. `UNTIL`/`RDATE`/`EXDATE`/`DTSTART`
/// values accept the ISO/iCal subset [`parse_loose_datetime`] accepts;
/// dateutil's fuzzy parser accepts more. Non-UTC `TZID`s are unsupported
/// (the crate path owns them whenever it accepts the rule).
mod fallback {
    use std::collections::VecDeque;

    use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};

    /// Frequency, ordered as dateutil's `YEARLY..SECONDLY` (`0..6`) so
    /// `freq < HOURLY` / `freq > MONTHLY` comparisons port directly
    /// (`dateutil/rrule.py:53-59`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Freq {
        Yearly,
        Monthly,
        Weekly,
        Daily,
        Hourly,
        Minutely,
        Secondly,
    }

    /// Any failure of the fallback: a dateutil `ValueError`/`TypeError`
    /// (which `_rrule.py` maps to "no expansion"), or one of the
    /// documented dateutil-500/hang classes (also "no expansion" here).
    /// No message crosses this boundary — the caller only needs `None`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FallbackError;

    type Result<T> = std::result::Result<T, FallbackError>;

    /// Parse an integer the way Python's `int()` does for RRULE values:
    /// optional sign, digits with `_` separators placed between digits,
    /// unbounded magnitude. Magnitudes beyond `i64` saturate: a count or
    /// interval that large never exhausts within any real window, and
    /// every yielded instant matches dateutil.
    fn parse_py_int(value: &str) -> Result<i64> {
        let value = value.trim();
        let (negative, digits) = match value.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => match value.strip_prefix('+') {
                Some(rest) => (false, rest),
                None => (false, value),
            },
        };
        if digits.is_empty() {
            return Err(FallbackError);
        }
        let mut magnitude: i128 = 0;
        let mut any_digit = false;
        let mut prev_underscore = false;
        for c in digits.chars() {
            if c == '_' {
                if !any_digit || prev_underscore {
                    return Err(FallbackError);
                }
                prev_underscore = true;
                continue;
            }
            let digit = c.to_digit(10).ok_or(FallbackError)?;
            prev_underscore = false;
            any_digit = true;
            // Clamp while accumulating so absurd inputs cannot overflow `i128`.
            magnitude = (magnitude * 10 + i128::from(digit)).min(i128::from(i64::MAX) + 1);
        }
        if prev_underscore || !any_digit {
            return Err(FallbackError);
        }
        let signed = if negative { -magnitude } else { magnitude };
        Ok(signed.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
    }

    /// One parsed `BYDAY` item: dateutil weekday (`0=MO..6=SU`) plus the
    /// optional ordinal (`+10MO` → `Some(10)`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ParsedByday {
        weekday: u8,
        ordinal: Option<i64>,
    }

    /// `MO`..`SU` to dateutil weekday (`_weekday_map`, `rrule.py:1471-1472`).
    fn weekday_code(code: &str) -> Result<u8> {
        match code {
            "MO" => Ok(0),
            "TU" => Ok(1),
            "WE" => Ok(2),
            "TH" => Ok(3),
            "FR" => Ok(4),
            "SA" => Ok(5),
            "SU" => Ok(6),
            _ => Err(FallbackError),
        }
    }

    /// Parse a `BYDAY`/`BYWEEKDAY` value the way `_handle_BYWEEKDAY` does
    /// (`rrule.py:1509-1533`), returning the items. `n == 0` is invalid
    /// (`weekday.__init__` raises).
    fn parse_byday(value: &str) -> Result<Vec<ParsedByday>> {
        value
            .split(',')
            .map(|wday| {
                if wday.contains('(') {
                    // `TH(+1)` form: `splt = wday.split('(')` (no maxsplit —
                    // extras ignored); `n = int(splt[1][:-1])` (last char
                    // dropped unconditionally).
                    let mut parts = wday.split('(');
                    let w = parts.next().unwrap_or("");
                    let rest = parts.next().ok_or(FallbackError)?;
                    let n_str: String = {
                        let mut chars: Vec<char> = rest.chars().collect();
                        chars.pop();
                        chars.into_iter().collect()
                    };
                    let n = parse_py_int(&n_str)?;
                    if n == 0 {
                        return Err(FallbackError);
                    }
                    Ok(ParsedByday {
                        weekday: weekday_code(w)?,
                        ordinal: Some(n),
                    })
                } else {
                    // `+1MO` form: leading `[+-0-9]*` is the ordinal.
                    // (For an all-digit item dateutil's loop variable ends
                    // at the last index rather than the length; both shapes
                    // fail the weekday lookup, so the verdict matches.)
                    let idx = wday
                        .char_indices()
                        .find(|(_, c)| !"+-0123456789".contains(*c))
                        .map(|(i, _)| i)
                        .unwrap_or(wday.len());
                    let (n_str, w) = wday.split_at(idx);
                    let ordinal = if n_str.is_empty() {
                        None
                    } else {
                        let n = parse_py_int(n_str)?;
                        if n == 0 {
                            return Err(FallbackError);
                        }
                        Some(n)
                    };
                    Ok(ParsedByday {
                        weekday: weekday_code(w)?,
                        ordinal,
                    })
                }
            })
            .collect()
    }

    /// A parsed bundle date: the UTC wall clock plus whether the source
    /// was tz-aware. dateutil rejects naive `UNTIL` against an aware
    /// `dtstart`, and `rruleset` merges raise on naive-vs-aware `<`
    /// (while `!=` is just unequal).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct BundleDate {
        dt: NaiveDateTime,
        aware: bool,
    }

    /// Parse one date value. Accepts exactly the forms
    /// [`super::parse_loose_datetime`] accepts (the ISO/iCal shapes stored
    /// rules carry), returning the instant; aware values normalize to UTC.
    /// Anything else is invalid, as when dateutil's `parser.parse` fails
    /// inside `_handle_UNTIL`.
    fn parse_bundle_date(value: &str) -> Result<BundleDate> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
            return Ok(BundleDate {
                dt: dt.with_timezone(&Utc).naive_utc(),
                aware: true,
            });
        }
        if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ") {
            return Ok(BundleDate { dt, aware: true });
        }
        const NAIVE_FORMATS: [&str; 6] = [
            "%Y%m%dT%H%M%S",
            "%Y%m%d",
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d",
        ];
        for fmt in NAIVE_FORMATS {
            if let Ok(dt) = NaiveDateTime::parse_from_str(value, fmt) {
                return Ok(BundleDate { dt, aware: false });
            }
            if let Ok(d) = NaiveDate::parse_from_str(value, fmt) {
                if let Some(dt) = d.and_hms_opt(0, 0, 0) {
                    return Ok(BundleDate { dt, aware: false });
                }
            }
        }
        Err(FallbackError)
    }

    /// Raw parsed rule kwargs: each field `None` when the property was
    /// absent. Duplicates overwrite — last wins, as `_parse_rfc_rrule`
    /// assigns `rrkwargs[name]` per pair (`rrule.py:1548-1560`).
    #[derive(Default)]
    struct RawRule {
        freq: Option<Freq>,
        interval: Option<i64>,
        wkst: Option<u8>,
        count: Option<i64>,
        until: Option<BundleDate>,
        bysetpos: Option<Vec<i64>>,
        bymonth: Option<Vec<i64>>,
        bymonthday: Option<Vec<i64>>,
        byyearday: Option<Vec<i64>>,
        byeaster: Option<Vec<i64>>,
        byweekno: Option<Vec<i64>>,
        byweekday: Option<Vec<ParsedByday>>,
        byhour: Option<Vec<i64>>,
        byminute: Option<Vec<i64>>,
        bysecond: Option<Vec<i64>>,
    }

    /// A parsed RRULE in post-constructor state: every default applied,
    /// every `ValueError` raised, exactly as `rrule.__init__`
    /// (`rrule.py:428-698`) leaves them. All instants are UTC wall clocks.
    /// Empty `Vec`s stand for dateutil's `None`/`()`: every use is a
    /// truthiness check, membership, or iteration, where they agree.
    struct Rule {
        freq: Freq,
        dtstart: NaiveDateTime,
        aware: bool,
        interval: i64,
        wkst: u8,
        count: Option<i64>,
        until: Option<NaiveDateTime>,
        bysetpos: Vec<i32>,
        bymonth: Vec<i64>,
        byyearday: Vec<i64>,
        byeaster: Vec<i64>,
        bymonthday_pos: Vec<i64>,
        bymonthday_neg: Vec<i64>,
        byweekno: Vec<i64>,
        byweekday: Vec<u8>,
        bynweekday: Vec<(u8, i64)>,
        byhour: Vec<i64>,
        byminute: Vec<i64>,
        bysecond: Vec<i64>,
        /// Cartesian times for `freq < HOURLY` (`rrule.py:687-698`); empty
        /// otherwise (per-iteration `gettimeset` is used instead).
        timeset: Vec<NaiveTime>,
    }

    /// `math.gcd`: non-negative, `gcd(0, b) == b`. Total over `i64`
    /// (unsigned internals, so `i64::MIN` cannot overflow).
    fn gcd(a: i64, b: i64) -> i64 {
        let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        a.min(i64::MAX as u64) as i64
    }

    /// `__construct_byset` (`rrule.py:1032-1077`): keep the `BYXXX` values
    /// reachable from `start` stepping `interval` modulo `base`; an empty
    /// set is invalid. (`divmod(x, g)[1]` with `g > 0` is `rem_euclid`.)
    fn construct_byset(start: i64, values: Vec<i64>, interval: i64, base: i64) -> Result<Vec<i64>> {
        let mut kept: Vec<i64> = values
            .into_iter()
            .filter(|num| {
                let g = gcd(interval, base);
                // `(num - start) % g == 0` without the overflowing
                // subtraction: congruence is exact for any magnitudes.
                g == 1 || num.rem_euclid(g) == start.rem_euclid(g)
            })
            .collect();
        if kept.is_empty() {
            return Err(FallbackError);
        }
        kept.sort_unstable();
        kept.dedup();
        Ok(kept)
    }

    fn sorted_unique(mut values: Vec<i64>) -> Vec<i64> {
        values.sort_unstable();
        values.dedup();
        values
    }

    impl Rule {
        /// `rrule.__init__` (`rrule.py:428-698`): defaults plus validation.
        /// Only expansion-relevant state is kept (`_original_rule` feeds
        /// `__str__`, which nothing calls).
        fn build(raw: RawRule, dtstart: NaiveDateTime, aware: bool) -> Result<Self> {
            // `rrule.__init__` truncates the anchor to whole seconds
            // (`dtstart.replace(microsecond=0)`, `rrule.py:443`); without
            // this a sub-second anchor wrongly excludes its own
            // truncated fire (`res >= dtstart`). `0` is always a valid
            // nanosecond, so the fallback never runs.
            let dtstart = dtstart.with_nanosecond(0).unwrap_or(dtstart);
            // A missing FREQ fails in the constructor call itself.
            let freq = raw.freq.ok_or(FallbackError)?;
            let interval = raw.interval.unwrap_or(1);
            // dateutil never validates the interval, but `0` yields one
            // instant forever and negatives step backwards forever —
            // Django hangs or serves garbage. Both validators promise
            // `INTERVAL >= 1`, and the crate path already yields `None`
            // here, so the fallback does too.
            if interval < 1 {
                return Err(FallbackError);
            }
            let until = match raw.until {
                Some(u) => {
                    // `rrule.py:461-473`: aware and naive must not mix.
                    if aware != u.aware {
                        return Err(FallbackError);
                    }
                    Some(u.dt)
                }
                None => None,
            };
            // `count` + `until` together only warn (`:475-478`); both apply.
            // `wkst` defaults to `calendar.firstweekday()` (Monday = 0).
            let wkst = raw.wkst.unwrap_or(0);
            let bysetpos = match raw.bysetpos {
                // Kept in input order with duplicates (`tuple(bysetpos)`,
                // `:495`); each must be non-zero within ±366.
                Some(values) => {
                    for pos in &values {
                        if *pos == 0 || !(-366..=366).contains(pos) {
                            return Err(FallbackError);
                        }
                    }
                    values.into_iter().map(|pos| pos as i32).collect()
                }
                None => Vec::new(),
            };

            // Day-selector defaults (`:504-517`): only when NO day selector
            // (`byweekno`/`byyearday`/`bymonthday`/`byweekday`/`byeaster`)
            // was given at all.
            let mut bymonth = raw.bymonth;
            let mut bymonthday = raw.bymonthday;
            let mut byweekday = raw.byweekday;
            if raw.byweekno.is_none()
                && raw.byyearday.is_none()
                && bymonthday.is_none()
                && byweekday.is_none()
                && raw.byeaster.is_none()
            {
                match freq {
                    Freq::Yearly => {
                        if bymonth.is_none() {
                            bymonth = Some(vec![i64::from(dtstart.month())]);
                        }
                        bymonthday = Some(vec![i64::from(dtstart.day())]);
                    }
                    Freq::Monthly => {
                        bymonthday = Some(vec![i64::from(dtstart.day())]);
                    }
                    Freq::Weekly => {
                        byweekday = Some(vec![ParsedByday {
                            weekday: dtstart.weekday().num_days_from_monday() as u8,
                            ordinal: None,
                        }]);
                    }
                    _ => {}
                }
            }

            let bymonth = sorted_unique(bymonth.unwrap_or_default());
            let byyearday = sorted_unique(raw.byyearday.unwrap_or_default());
            // `tuple(sorted(byeaster))` (`:548`): sorted, not deduped.
            let mut byeaster = raw.byeaster.unwrap_or_default();
            byeaster.sort_unstable();
            let (bymonthday_pos, bymonthday_neg) = match bymonthday {
                Some(values) => {
                    // Deduped, then split by sign (`:562-565`).
                    let values = sorted_unique(values);
                    (
                        values.iter().copied().filter(|v| *v > 0).collect(),
                        values.iter().copied().filter(|v| *v < 0).collect(),
                    )
                }
                None => (Vec::new(), Vec::new()),
            };
            let byweekno = sorted_unique(raw.byweekno.unwrap_or_default());

            // `byweekday` split (`:594-607`): the ordinal is kept only for
            // `YEARLY`/`MONTHLY`; finer frequencies use the plain weekday.
            // (`n == 0` already failed in `parse_byday`.)
            let mut plain: Vec<u8> = Vec::new();
            let mut nth: Vec<(u8, i64)> = Vec::new();
            for item in byweekday.unwrap_or_default() {
                match item.ordinal {
                    None => plain.push(item.weekday),
                    Some(_) if freq > Freq::Monthly => plain.push(item.weekday),
                    Some(n) => nth.push((item.weekday, n)),
                }
            }
            plain.sort_unstable();
            plain.dedup();
            nth.sort_unstable();
            nth.dedup();

            // `byhour`/`byminute`/`bysecond` (`:625-685`): same-level
            // `BYXXX` goes through `__construct_byset`; otherwise the set
            // is sorted as given. Defaults come from `dtstart` below the
            // matching frequency.
            let byhour = match raw.byhour {
                None => {
                    if freq < Freq::Hourly {
                        vec![i64::from(dtstart.hour())]
                    } else {
                        Vec::new()
                    }
                }
                Some(values) => {
                    if freq == Freq::Hourly {
                        construct_byset(i64::from(dtstart.hour()), values, interval, 24)?
                    } else {
                        sorted_unique(values)
                    }
                }
            };
            let byminute = match raw.byminute {
                None => {
                    if freq < Freq::Minutely {
                        vec![i64::from(dtstart.minute())]
                    } else {
                        Vec::new()
                    }
                }
                Some(values) => {
                    if freq == Freq::Minutely {
                        construct_byset(i64::from(dtstart.minute()), values, interval, 60)?
                    } else {
                        sorted_unique(values)
                    }
                }
            };
            let bysecond = match raw.bysecond {
                None => {
                    if freq < Freq::Secondly {
                        vec![i64::from(dtstart.second())]
                    } else {
                        Vec::new()
                    }
                }
                Some(values) => {
                    if freq == Freq::Secondly {
                        construct_byset(i64::from(dtstart.second()), values, interval, 60)?
                    } else {
                        sorted_unique(values)
                    }
                }
            };

            // Cartesian times for `freq < HOURLY` (`:687-698`); an
            // out-of-range component raises (invalid `datetime.time`).
            let timeset = if freq < Freq::Hourly {
                let mut timeset = Vec::new();
                for hour in &byhour {
                    for minute in &byminute {
                        for second in &bysecond {
                            timeset.push(
                                NaiveTime::from_hms_opt(
                                    *hour as u32,
                                    *minute as u32,
                                    *second as u32,
                                )
                                .ok_or(FallbackError)?,
                            );
                        }
                    }
                }
                timeset.sort_unstable();
                timeset
            } else {
                Vec::new()
            };

            Ok(Self {
                freq,
                dtstart,
                aware,
                interval,
                wkst,
                count: raw.count,
                until,
                bysetpos,
                bymonth,
                byyearday,
                byeaster,
                bymonthday_pos,
                bymonthday_neg,
                byweekno,
                byweekday: plain,
                bynweekday: nth,
                byhour,
                byminute,
                bysecond,
                timeset,
            })
        }
    }

    /// `_freq_map` (`rrule.py:1463-1470`).
    fn parse_freq(value: &str) -> Result<Freq> {
        match value {
            "YEARLY" => Ok(Freq::Yearly),
            "MONTHLY" => Ok(Freq::Monthly),
            "WEEKLY" => Ok(Freq::Weekly),
            "DAILY" => Ok(Freq::Daily),
            "HOURLY" => Ok(Freq::Hourly),
            "MINUTELY" => Ok(Freq::Minutely),
            "SECONDLY" => Ok(Freq::Secondly),
            _ => Err(FallbackError),
        }
    }

    /// `_handle_int_list` (`rrule.py:1477-1478`): every item must parse.
    fn parse_int_list(value: &str) -> Result<Vec<i64>> {
        value.split(',').map(parse_py_int).collect()
    }

    /// `_parse_rfc_rrule` (`rrule.py:1537-1561`): one bare or
    /// `RRULE:`-prefixed rule line into post-constructor state. `line` is
    /// already uppercased by the bundle parser.
    fn parse_rule_line(line: &str, dtstart: NaiveDateTime, aware: bool) -> Result<Rule> {
        let value = if line.contains(':') {
            // `name, value = line.split(':')` — no maxsplit, so a second
            // colon is "too many values to unpack (expected 2)".
            let mut parts = line.split(':');
            let name = parts.next().unwrap_or("");
            let value = parts.next().ok_or(FallbackError)?;
            if parts.next().is_some() {
                return Err(FallbackError);
            }
            if name != "RRULE" {
                return Err(FallbackError);
            }
            value
        } else {
            line
        };
        let mut raw = RawRule::default();
        for pair in value.split(';') {
            // `name, value = pair.split('=')` — same unpack strictness.
            let mut eq = pair.split('=');
            let name = eq.next().unwrap_or("");
            let prop = eq.next().ok_or(FallbackError)?;
            if eq.next().is_some() {
                return Err(FallbackError);
            }
            match name {
                "FREQ" => raw.freq = Some(parse_freq(prop)?),
                "INTERVAL" => raw.interval = Some(parse_py_int(prop)?),
                "COUNT" => raw.count = Some(parse_py_int(prop)?),
                "BYSETPOS" => raw.bysetpos = Some(parse_int_list(prop)?),
                "BYMONTH" => raw.bymonth = Some(parse_int_list(prop)?),
                "BYMONTHDAY" => raw.bymonthday = Some(parse_int_list(prop)?),
                "BYYEARDAY" => raw.byyearday = Some(parse_int_list(prop)?),
                "BYEASTER" => raw.byeaster = Some(parse_int_list(prop)?),
                "BYWEEKNO" => raw.byweekno = Some(parse_int_list(prop)?),
                "BYHOUR" => raw.byhour = Some(parse_int_list(prop)?),
                "BYMINUTE" => raw.byminute = Some(parse_int_list(prop)?),
                "BYSECOND" => raw.bysecond = Some(parse_int_list(prop)?),
                "UNTIL" => raw.until = Some(parse_bundle_date(prop)?),
                "WKST" => raw.wkst = Some(weekday_code(prop)?),
                "BYDAY" | "BYWEEKDAY" => raw.byweekday = Some(parse_byday(prop)?),
                _ => return Err(FallbackError),
            }
        }
        Rule::build(raw, dtstart, aware)
    }

    /// `easter.easter(year)` with the default Western method
    /// (`dateutil/easter.py:78-89`). All `%` operands stay non-negative,
    /// so Rust `%` matches Python `%`.
    fn easter_western(year: i32) -> Result<NaiveDate> {
        let y = i64::from(year);
        let g = y % 19;
        let c = y / 100;
        let h = (c - c / 4 - (8 * c + 13) / 25 + 19 * g + 15) % 30;
        let i = h - (h / 28) * (1 - (h / 28) * (29 / (h + 1)) * ((21 - g) / 11));
        let j = (y + y / 4 + i + 2 - c + c / 4) % 7;
        let p = i - j;
        let d = 1 + (p + 27 + (p + 6) / 40) % 31;
        let m = 3 + (p + 26) / 30;
        NaiveDate::from_ymd_opt(year, m as u32, d as u32).ok_or(FallbackError)
    }

    /// Month ranges: `M365RANGE`/`M366RANGE` (`rrule.py:44-45`).
    const MRANGE_365: [i32; 13] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365];
    const MRANGE_366: [i32; 13] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366];

    /// The year masks, built exactly as `rrule.py:35-49`: 366-day tables
    /// plus a 7-day tail for cross-year weekly periods, with the Feb-29
    /// slot deleted for common years (index 59 for month/day, index 31 —
    /// the `-29` — for negative monthdays).
    struct YearMasks {
        mmask: Vec<i64>,
        mdaymask: Vec<i64>,
        nmdaymask: Vec<i64>,
    }

    fn year_masks(leap: bool) -> YearMasks {
        let mut mmask = Vec::with_capacity(373);
        let mut mday = Vec::with_capacity(373);
        let mut nmday = Vec::with_capacity(373);
        for (month, days) in [
            (1, 31),
            (2, 29),
            (3, 31),
            (4, 30),
            (5, 31),
            (6, 30),
            (7, 31),
            (8, 31),
            (9, 30),
            (10, 31),
            (11, 30),
            (12, 31),
        ] {
            for d in 1..=days {
                mmask.push(month);
                mday.push(d);
                nmday.push(d - days - 1);
            }
        }
        // 7-day tail (January of next year): `M31[:7]` in each table.
        for d in 1..=7 {
            mmask.push(1);
            mday.push(d);
            nmday.push(d - 32);
        }
        if !leap {
            mmask.remove(59);
            mday.remove(59);
            nmday.remove(31);
        }
        YearMasks {
            mmask,
            mdaymask: mday,
            nmdaymask: nmday,
        }
    }

    /// `mrange[month-1:month+1]` unpacked to `(first, last)`: a slice, so
    /// an out-of-range month yields a short slice whose unpack raises
    /// (used only by the `BYDAY`-ordinal mask under `YEARLY`).
    fn mrange_pair(mrange: [i32; 13], month: i64) -> Option<(i64, i64)> {
        let lo = month.saturating_sub(1);
        let hi = month.saturating_add(1);
        let start = (if lo < 0 { lo.saturating_add(13) } else { lo }).clamp(0, 13);
        let stop = (if hi < 0 { hi.saturating_add(13) } else { hi }).clamp(0, 13);
        if stop - start == 2 {
            Some((
                i64::from(mrange[start as usize]),
                i64::from(mrange[(start + 1) as usize]),
            ))
        } else {
            None
        }
    }

    /// dateutil's `datetime.MAXYEAR`: expansion stops past it.
    const MAXYEAR: i32 = 9999;

    /// `calendar.isleap` for proleptic-Gregorian years ≥ 1.
    fn is_leap(year: i32) -> bool {
        year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
    }

    /// `_iterinfo` (`rrule.py:1112-1304`): the rebuilt year masks plus the
    /// day/time set constructors. The weekday mask is computed modularly
    /// instead of sliced (`WDAYMASK[wday:][i] == (wday + i) % 7`).
    struct IterInfo {
        yearlen: i32,
        nextyearlen: i32,
        yearordinal: i32,
        jan1_weekday: u8,
        masks: YearMasks,
        mrange: [i32; 13],
        wnomask: Option<Vec<u8>>,
        nwdaymask: Option<Vec<u8>>,
        eastermask: Option<Vec<u8>>,
        lastyear: Option<i32>,
        lastmonth: Option<u8>,
    }

    impl IterInfo {
        fn new() -> Self {
            Self {
                yearlen: 0,
                nextyearlen: 0,
                yearordinal: 0,
                jan1_weekday: 0,
                masks: year_masks(true),
                mrange: MRANGE_366,
                wnomask: None,
                nwdaymask: None,
                eastermask: None,
                lastyear: None,
                lastmonth: None,
            }
        }

        /// `self.wdaymask[i]`: modular values, but indices outside the
        /// sliced list (`len == 385 - wday`) fail like dateutil's
        /// `IndexError` — with Python's negative wraparound.
        fn wdaymask_checked(&self, i: i64) -> Result<i64> {
            let len = 385 - i64::from(self.jan1_weekday);
            let j = if i < 0 {
                if i < -len {
                    return Err(FallbackError);
                }
                i + len
            } else {
                if i >= len {
                    return Err(FallbackError);
                }
                i
            };
            Ok((i64::from(self.jan1_weekday) + j).rem_euclid(7))
        }

        /// `rebuild` (`rrule.py:1123-1251`). Mask writes that raise
        /// `IndexError` in dateutil fail here (→ "no expansion").
        fn rebuild(&mut self, year: i32, month: u8, rule: &Rule) -> Result<()> {
            if Some(year) != self.lastyear {
                let leap = is_leap(year);
                self.yearlen = 365 + i32::from(leap);
                self.nextyearlen = 365 + i32::from(is_leap(year + 1));
                let jan1 = NaiveDate::from_ymd_opt(year, 1, 1).ok_or(FallbackError)?;
                self.yearordinal = jan1.num_days_from_ce();
                self.jan1_weekday = jan1.weekday().num_days_from_monday() as u8;
                self.masks = year_masks(leap);
                self.mrange = if leap { MRANGE_366 } else { MRANGE_365 };

                if rule.byweekno.is_empty() {
                    self.wnomask = None;
                } else {
                    self.wnomask = Some(self.build_wnomask(year, rule)?);
                }
            }
            if !rule.bynweekday.is_empty()
                && (Some(month) != self.lastmonth || Some(year) != self.lastyear)
            {
                self.build_nwdaymask(month, rule)?;
            }
            if !rule.byeaster.is_empty() {
                self.build_eastermask(year, rule)?;
            }
            self.lastyear = Some(year);
            self.lastmonth = Some(month);
            Ok(())
        }

        /// The `BYWEEKNO` mask block of `rebuild` (`rrule.py:1150-1215`).
        fn build_wnomask(&self, year: i32, rule: &Rule) -> Result<Vec<u8>> {
            let mut wnomask = vec![0u8; (self.yearlen + 7) as usize];
            let wkst = i64::from(rule.wkst);
            let yearweekday = i64::from(self.jan1_weekday);
            let firstwkst = (7 - yearweekday + wkst).rem_euclid(7);
            let (no1wkst, wyearlen) = if firstwkst >= 4 {
                (
                    0,
                    i64::from(self.yearlen) + (yearweekday - wkst).rem_euclid(7),
                )
            } else {
                (firstwkst, i64::from(self.yearlen) - firstwkst)
            };
            let numweeks = wyearlen.div_euclid(7) + wyearlen.rem_euclid(7) / 4;
            for n_raw in &rule.byweekno {
                let mut n = *n_raw;
                if n < 0 {
                    n += numweeks + 1;
                }
                if !(0 < n && n <= numweeks) {
                    continue;
                }
                let mut i = if n > 1 {
                    let mut i = no1wkst + (n - 1) * 7;
                    if no1wkst != firstwkst {
                        i -= 7 - firstwkst;
                    }
                    i
                } else {
                    no1wkst
                };
                for _ in 0..7 {
                    *wnomask.get_mut(i as usize).ok_or(FallbackError)? = 1;
                    i += 1;
                    if self.wdaymask_checked(i)? == wkst {
                        break;
                    }
                }
            }
            if rule.byweekno.contains(&1) {
                // Week number 1 of next year as well.
                let mut i = no1wkst + numweeks * 7;
                if no1wkst != firstwkst {
                    i -= 7 - firstwkst;
                }
                if i < i64::from(self.yearlen) {
                    for _ in 0..7 {
                        *wnomask.get_mut(i as usize).ok_or(FallbackError)? = 1;
                        i += 1;
                        if self.wdaymask_checked(i)? == wkst {
                            break;
                        }
                    }
                }
            }
            if no1wkst != 0 {
                // Last week number of last year as well.
                let lnumweeks = if rule.byweekno.contains(&-1) {
                    -1
                } else {
                    let lyearweekday = NaiveDate::from_ymd_opt(year - 1, 1, 1)
                        .ok_or(FallbackError)?
                        .weekday()
                        .num_days_from_monday() as i64;
                    let lno1wkst = (7 - lyearweekday + wkst).rem_euclid(7);
                    if lno1wkst >= 4 {
                        let lyearlen = 365 + i64::from(is_leap(year - 1));
                        52 + (lyearlen + (lyearweekday - wkst).rem_euclid(7)).rem_euclid(7) / 4
                    } else {
                        52 + (i64::from(self.yearlen) - no1wkst).rem_euclid(7) / 4
                    }
                };
                if rule.byweekno.contains(&lnumweeks) {
                    for i in 0..no1wkst {
                        *wnomask.get_mut(i as usize).ok_or(FallbackError)? = 1;
                    }
                }
            }
            Ok(wnomask)
        }

        /// The `BYDAY`-ordinal mask block of `rebuild`
        /// (`rrule.py:1217-1242`). Out-of-range positive ordinals fail
        /// here (dateutil's `IndexError`); negative ones just miss their
        /// range and yield nothing.
        fn build_nwdaymask(&mut self, month: u8, rule: &Rule) -> Result<()> {
            let mut ranges: Vec<(i64, i64)> = Vec::new();
            if rule.freq == Freq::Yearly {
                if rule.bymonth.is_empty() {
                    ranges.push((0, i64::from(self.yearlen)));
                } else {
                    for m in &rule.bymonth {
                        ranges.push(mrange_pair(self.mrange, *m).ok_or(FallbackError)?);
                    }
                }
            } else if rule.freq == Freq::Monthly {
                ranges.push(mrange_pair(self.mrange, i64::from(month)).ok_or(FallbackError)?);
            }
            // Finer frequencies never reach here with ordinals (the
            // constructor strips them); an empty `ranges` keeps the old
            // mask, ported as-is.
            if ranges.is_empty() {
                return Ok(());
            }
            let mut nwdaymask = vec![0u8; self.yearlen as usize];
            for (first, last) in ranges {
                let last = last - 1;
                for (wday, n) in &rule.bynweekday {
                    let wday = i64::from(*wday);
                    // Saturating: astronomic ordinals fail the mask lookup
                    // below, as dateutil's `IndexError`.
                    let i = if *n < 0 {
                        let i = last.saturating_add(n.saturating_add(1).saturating_mul(7));
                        i.saturating_sub((self.wdaymask_checked(i)? - wday).rem_euclid(7))
                    } else {
                        let i = first.saturating_add(n.saturating_sub(1).saturating_mul(7));
                        i.saturating_add((7 - self.wdaymask_checked(i)? + wday).rem_euclid(7))
                    };
                    if first <= i && i <= last {
                        *nwdaymask.get_mut(i as usize).ok_or(FallbackError)? = 1;
                    }
                }
            }
            self.nwdaymask = Some(nwdaymask);
            Ok(())
        }

        /// The `BYEASTER` mask block of `rebuild` (`rrule.py:1244-1248`).
        /// A negative index wraps (Python list assignment); a
        /// past-the-end index raises — both ported exactly.
        fn build_eastermask(&mut self, year: i32, rule: &Rule) -> Result<()> {
            let len = i64::from(self.yearlen + 7);
            let mut eastermask = vec![0u8; len as usize];
            let eyday = easter_western(year)?.num_days_from_ce() - self.yearordinal;
            for offset in &rule.byeaster {
                let mut idx = i64::from(eyday).saturating_add(*offset);
                if idx < 0 {
                    idx += len;
                    if idx < 0 {
                        return Err(FallbackError);
                    }
                } else if idx >= len {
                    return Err(FallbackError);
                }
                eastermask[idx as usize] = 1;
            }
            self.eastermask = Some(eastermask);
            Ok(())
        }

        /// `ydayset`/`mdayset`/`wdayset`/`ddayset` (`rrule.py:1253-1282`):
        /// the day-index set plus its `[start, end)` window. `None`
        /// entries are days outside the frequency's period.
        fn dayset(
            &self,
            freq: Freq,
            year: i32,
            month: u8,
            day: u32,
            wkst: u8,
        ) -> Result<(Vec<Option<i32>>, i32, i32)> {
            match freq {
                Freq::Yearly => {
                    let dset: Vec<Option<i32>> = (0..self.yearlen).map(Some).collect();
                    Ok((dset, 0, self.yearlen))
                }
                Freq::Monthly => {
                    let mut dset = vec![None; self.yearlen as usize];
                    let (start, end) =
                        mrange_pair(self.mrange, i64::from(month)).ok_or(FallbackError)?;
                    for i in start..end {
                        dset[i as usize] = Some(i as i32);
                    }
                    Ok((dset, start as i32, end as i32))
                }
                Freq::Weekly => {
                    // Cross-year weeks spill past `yearlen` (hence `+7`).
                    let mut dset = vec![None; (self.yearlen + 7) as usize];
                    let today = NaiveDate::from_ymd_opt(year, u32::from(month), day)
                        .ok_or(FallbackError)?;
                    let mut i = today.num_days_from_ce() - self.yearordinal;
                    let start = i;
                    for _ in 0..7 {
                        *dset.get_mut(i as usize).ok_or(FallbackError)? = Some(i);
                        i += 1;
                        if self.wdaymask_checked(i64::from(i))? == i64::from(wkst) {
                            break;
                        }
                    }
                    Ok((dset, start, i))
                }
                _ => {
                    let mut dset = vec![None; self.yearlen as usize];
                    let today = NaiveDate::from_ymd_opt(year, u32::from(month), day)
                        .ok_or(FallbackError)?;
                    let i = today.num_days_from_ce() - self.yearordinal;
                    *dset.get_mut(i as usize).ok_or(FallbackError)? = Some(i);
                    Ok((dset, i, i + 1))
                }
            }
        }

        /// `htimeset`/`mtimeset`/`stimeset` (`rrule.py:1284-1304`): the
        /// per-iteration times for `freq >= HOURLY`. Out-of-range
        /// components raise (invalid `datetime.time`).
        fn gettimeset(
            &self,
            freq: Freq,
            hour: i64,
            minute: i64,
            second: i64,
            rule: &Rule,
        ) -> Result<Vec<NaiveTime>> {
            let mut timeset = Vec::new();
            match freq {
                Freq::Hourly => {
                    for m in &rule.byminute {
                        for s in &rule.bysecond {
                            timeset.push(
                                NaiveTime::from_hms_opt(hour as u32, *m as u32, *s as u32)
                                    .ok_or(FallbackError)?,
                            );
                        }
                    }
                }
                Freq::Minutely => {
                    for s in &rule.bysecond {
                        timeset.push(
                            NaiveTime::from_hms_opt(hour as u32, minute as u32, *s as u32)
                                .ok_or(FallbackError)?,
                        );
                    }
                }
                Freq::Secondly => {
                    timeset.push(
                        NaiveTime::from_hms_opt(hour as u32, minute as u32, second as u32)
                            .ok_or(FallbackError)?,
                    );
                }
                _ => {}
            }
            timeset.sort_unstable();
            Ok(timeset)
        }
    }

    /// `rrule._iter` (`rrule.py:776-1030`) as a pull iterator: each `pump`
    /// is one `while True` pass. `failed` mirrors an exception escaping
    /// the generator — the caller must discard everything, as
    /// `_rrule.py`'s `except` does.
    struct RruleIter {
        rule: Rule,
        ii: IterInfo,
        year: i32,
        month: u8,
        /// 1-based day-of-month; may exceed the month between the advance
        /// and `fixday`, exactly as dateutil's `day` does.
        day: i64,
        hour: i64,
        minute: i64,
        second: i64,
        /// Only `WEEKLY` maintains this (stale otherwise, as in dateutil).
        weekday: u8,
        timeset: Vec<NaiveTime>,
        count: Option<i64>,
        buffer: VecDeque<NaiveDateTime>,
        done: bool,
        failed: bool,
        last_yield: Option<NaiveDateTime>,
    }

    impl RruleIter {
        fn new(rule: Rule) -> Result<Self> {
            let dtstart = rule.dtstart;
            let mut ii = IterInfo::new();
            ii.rebuild(dtstart.year(), dtstart.month() as u8, &rule)?;
            // Initial timeset (`rrule.py:808-822`).
            let (hour, minute, second) = (
                i64::from(dtstart.hour()),
                i64::from(dtstart.minute()),
                i64::from(dtstart.second()),
            );
            let timeset = if rule.freq < Freq::Hourly {
                rule.timeset.clone()
            } else if (!rule.byhour.is_empty() && !rule.byhour.contains(&hour))
                || (rule.freq >= Freq::Minutely
                    && !rule.byminute.is_empty()
                    && !rule.byminute.contains(&minute))
                || (rule.freq >= Freq::Secondly
                    && !rule.bysecond.is_empty()
                    && !rule.bysecond.contains(&second))
            {
                Vec::new()
            } else {
                ii.gettimeset(rule.freq, hour, minute, second, &rule)?
            };
            let count = rule.count;
            Ok(Self {
                rule,
                ii,
                year: dtstart.year(),
                month: dtstart.month() as u8,
                day: i64::from(dtstart.day()),
                hour,
                minute,
                second,
                weekday: dtstart.weekday().num_days_from_monday() as u8,
                timeset,
                count,
                buffer: VecDeque::new(),
                done: false,
                failed: false,
                last_yield: None,
            })
        }

        fn failed(&self) -> bool {
            self.failed
        }

        fn next(&mut self) -> Option<NaiveDateTime> {
            loop {
                if let Some(dt) = self.buffer.pop_front() {
                    // Yields strictly increase for `INTERVAL >= 1`; a
                    // non-advance ends the series instead of looping forever.
                    if self.last_yield.is_some_and(|last| dt <= last) {
                        self.done = true;
                        self.buffer.clear();
                        return None;
                    }
                    self.last_yield = Some(dt);
                    return Some(dt);
                }
                if self.done || self.failed {
                    return None;
                }
                self.pump();
            }
        }

        /// One `while True` pass of `_iter`: filter the dayset, buffer the
        /// yields, advance the frequency. Sets `done` on exhaustion,
        /// `failed` on any dateutil exception.
        fn pump(&mut self) {
            if self.pump_inner().is_err() {
                self.failed = true;
                self.buffer.clear();
            }
        }

        fn day_u32(&self) -> Result<u32> {
            u32::try_from(self.day).map_err(|_| FallbackError)
        }

        /// `wdaymask[i]` for dayset indices, which never exceed
        /// `yearlen + 6 < 379 <= len` — total, unlike the ordinal path.
        fn wday_value(&self, i: i32) -> u8 {
            (i64::from(self.ii.jan1_weekday) + i64::from(i)).rem_euclid(7) as u8
        }

        /// The day filter (`rrule.py:832-847`): `true` means the day is out.
        /// Mask indices are dayset values, all below `yearlen + 7`.
        fn day_filtered(&self, i: i32) -> Result<bool> {
            let rule = &self.rule;
            let ii = &self.ii;
            let idx = i as usize;
            if !rule.bymonth.is_empty() && !rule.bymonth.contains(&ii.masks.mmask[idx]) {
                return Ok(true);
            }
            if !rule.byweekno.is_empty() {
                let wno = ii.wnomask.as_ref().ok_or(FallbackError)?;
                if wno[idx] == 0 {
                    return Ok(true);
                }
            }
            if !rule.byweekday.is_empty() && !rule.byweekday.contains(&self.wday_value(i)) {
                return Ok(true);
            }
            if let Some(nw) = &ii.nwdaymask {
                if nw[idx] == 0 {
                    return Ok(true);
                }
            }
            if !rule.byeaster.is_empty() {
                let em = ii.eastermask.as_ref().ok_or(FallbackError)?;
                if em[idx] == 0 {
                    return Ok(true);
                }
            }
            if !rule.bymonthday_pos.is_empty() || !rule.bymonthday_neg.is_empty() {
                let md = ii.masks.mdaymask[idx];
                let nmd = ii.masks.nmdaymask[idx];
                if !rule.bymonthday_pos.contains(&md) && !rule.bymonthday_neg.contains(&nmd) {
                    return Ok(true);
                }
            }
            if !rule.byyearday.is_empty() {
                let d = i64::from(i);
                let out = if d < i64::from(ii.yearlen) {
                    !rule.byyearday.contains(&(d + 1))
                        && !rule.byyearday.contains(&(-i64::from(ii.yearlen) + d))
                } else {
                    !rule.byyearday.contains(&(d + 1 - i64::from(ii.yearlen)))
                        && !rule
                            .byyearday
                            .contains(&(-i64::from(ii.nextyearlen) + d - i64::from(ii.yearlen)))
                };
                if out {
                    return Ok(true);
                }
            }
            Ok(false)
        }

        /// Buffer one occurrence (`res`): the `until`/`count` handling
        /// shared by both yield sites. Returns `true` when the series ends
        /// (`rrule.py:869-880` and `:886-898`).
        fn yield_one(&mut self, res: NaiveDateTime) -> Result<bool> {
            if let Some(until) = self.rule.until {
                if res > until {
                    self.done = true;
                    return Ok(true);
                }
            }
            if res >= self.rule.dtstart {
                if let Some(count) = self.count.as_mut() {
                    // Saturating: a saturated `i64::MIN` count stays
                    // negative and ends the series, as dateutil's unbounded
                    // decrement does (the first yield already returns).
                    *count = count.saturating_sub(1);
                    if *count < 0 {
                        self.done = true;
                        return Ok(true);
                    }
                }
                self.buffer.push_back(res);
            }
            Ok(false)
        }

        /// `__mod_distance` (`rrule.py:1079-1110`): the next same-level
        /// value stepping `interval`, plus the days/hours/minutes carried.
        /// Falls off the end (→ invalid, dateutil's unpack `TypeError`)
        /// when nothing is found within `base` steps.
        fn mod_distance(&self, value: i64, byxxx: &[i64], base: i64) -> Result<(i64, i64)> {
            let mut value = value;
            let mut accumulator: i64 = 0;
            for _ in 1..=base {
                let next = value.saturating_add(self.rule.interval);
                accumulator = accumulator.saturating_add(next.div_euclid(base));
                value = next.rem_euclid(base);
                if byxxx.contains(&value) {
                    return Ok((accumulator, value));
                }
            }
            Err(FallbackError)
        }

        #[allow(clippy::too_many_lines)]
        fn pump_inner(&mut self) -> Result<()> {
            let freq = self.rule.freq;
            let interval = self.rule.interval;
            let day = self.day_u32()?;
            let (mut dayset, start, end) =
                self.ii
                    .dayset(freq, self.year, self.month, day, self.rule.wkst)?;

            // Filter the dayset (`rrule.py:831-847`).
            let mut filtered = false;
            for idx in start..end {
                let i = match dayset[idx as usize] {
                    Some(v) => v,
                    None => continue,
                };
                if self.day_filtered(i)? {
                    dayset[idx as usize] = None;
                    filtered = true;
                }
            }

            // Yield phase (`rrule.py:850-898`).
            if !self.rule.bysetpos.is_empty() && !self.timeset.is_empty() {
                let days: Vec<i32> = dayset[start as usize..end as usize]
                    .iter()
                    .filter_map(|d| *d)
                    .collect();
                let timeset_len = self.timeset.len() as i64;
                let mut poslist: Vec<NaiveDateTime> = Vec::new();
                for p in 0..self.rule.bysetpos.len() {
                    let pos = i64::from(self.rule.bysetpos[p]);
                    let (daypos, timepos) = if pos < 0 {
                        (pos.div_euclid(timeset_len), pos.rem_euclid(timeset_len))
                    } else {
                        (
                            (pos - 1).div_euclid(timeset_len),
                            (pos - 1).rem_euclid(timeset_len),
                        )
                    };
                    // `days[daypos]` with Python negative wraparound; a
                    // miss is skipped (`except IndexError: pass`).
                    let day_idx = if daypos < 0 {
                        days.len() as i64 + daypos
                    } else {
                        daypos
                    };
                    let (Some(day_i), Some(time)) = (
                        usize::try_from(day_idx)
                            .ok()
                            .and_then(|k| days.get(k).copied()),
                        self.timeset.get(timepos as usize),
                    ) else {
                        continue;
                    };
                    let date = NaiveDate::from_num_days_from_ce_opt(self.ii.yearordinal + day_i)
                        .ok_or(FallbackError)?;
                    let res = date.and_time(*time);
                    if !poslist.contains(&res) {
                        poslist.push(res);
                    }
                }
                poslist.sort_unstable();
                for res in poslist {
                    if self.yield_one(res)? {
                        return Ok(());
                    }
                }
            } else {
                for idx in start..end {
                    let Some(day_i) = dayset[idx as usize] else {
                        continue;
                    };
                    let date = NaiveDate::from_num_days_from_ce_opt(self.ii.yearordinal + day_i)
                        .ok_or(FallbackError)?;
                    for t in 0..self.timeset.len() {
                        let time = self.timeset[t];
                        let res = date.and_time(time);
                        if self.yield_one(res)? {
                            return Ok(());
                        }
                    }
                }
            }

            // Advance the frequency (`rrule.py:900-1015`). Saturating
            // arithmetic: astronomic intervals land past `MAXYEAR`, where
            // the series ends — dateutil walks there month by month.
            let mut fixday = false;
            match freq {
                Freq::Yearly => {
                    // Saturating: any saturation lands past `MAXYEAR`,
                    // where the series ends.
                    let year = i64::from(self.year).saturating_add(interval);
                    if year > i64::from(MAXYEAR) {
                        self.done = true;
                        return Ok(());
                    }
                    self.year = year as i32;
                    self.ii.rebuild(self.year, self.month, &self.rule)?;
                }
                Freq::Monthly => {
                    let mut month = i64::from(self.month).saturating_add(interval);
                    if month > 12 {
                        let div = month.div_euclid(12);
                        month = month.rem_euclid(12);
                        let mut year = i64::from(self.year).saturating_add(div);
                        if month == 0 {
                            month = 12;
                            year -= 1;
                        }
                        if year > i64::from(MAXYEAR) {
                            self.done = true;
                            return Ok(());
                        }
                        self.year = year as i32;
                    }
                    self.month = month as u8;
                    self.ii.rebuild(self.year, self.month, &self.rule)?;
                }
                Freq::Weekly => {
                    let wkst = i64::from(self.rule.wkst);
                    let weekday = i64::from(self.weekday);
                    let step = if wkst > weekday {
                        -(weekday + 1 + (6 - wkst)) + interval.saturating_mul(7)
                    } else {
                        -(weekday - wkst) + interval.saturating_mul(7)
                    };
                    self.day = self.day.saturating_add(step);
                    self.weekday = self.rule.wkst;
                    fixday = true;
                }
                Freq::Daily => {
                    self.day = self.day.saturating_add(interval);
                    fixday = true;
                }
                Freq::Hourly => {
                    if filtered {
                        // Jump to one iteration before next day.
                        self.hour += (23 - self.hour).div_euclid(interval) * interval;
                    }
                    let (ndays, hour) = if self.rule.byhour.is_empty() {
                        let next = self.hour.saturating_add(interval);
                        (next.div_euclid(24), next.rem_euclid(24))
                    } else {
                        self.mod_distance(self.hour, &self.rule.byhour, 24)?
                    };
                    self.hour = hour;
                    if ndays != 0 {
                        self.day = self.day.saturating_add(ndays);
                        fixday = true;
                    }
                    self.timeset = self.ii.gettimeset(
                        freq,
                        self.hour,
                        self.minute,
                        self.second,
                        &self.rule,
                    )?;
                }
                Freq::Minutely => {
                    if filtered {
                        // Jump to one iteration before next day.
                        let total = self.hour * 60 + self.minute;
                        self.minute += (1439 - total).div_euclid(interval) * interval;
                    }
                    let mut valid = false;
                    let rep_rate = 24 * 60;
                    for _ in 0..rep_rate / gcd(interval, rep_rate) {
                        let (nhours, minute) = if self.rule.byminute.is_empty() {
                            let next = self.minute.saturating_add(interval);
                            (next.div_euclid(60), next.rem_euclid(60))
                        } else {
                            self.mod_distance(self.minute, &self.rule.byminute, 60)?
                        };
                        self.minute = minute;
                        let (div, hour) = divmod_euclid(self.hour.saturating_add(nhours), 24);
                        self.hour = hour;
                        if div != 0 {
                            self.day = self.day.saturating_add(div);
                            fixday = true;
                            // Ported as-is (`rrule.py:968`); never read
                            // again, like in dateutil.
                            #[allow(unused_assignments)]
                            {
                                filtered = false;
                            }
                        }
                        if self.rule.byhour.is_empty() || self.rule.byhour.contains(&self.hour) {
                            valid = true;
                            break;
                        }
                    }
                    if !valid {
                        // 'Invalid combination of interval and byhour
                        // resulting in empty rule.'
                        return Err(FallbackError);
                    }
                    self.timeset = self.ii.gettimeset(
                        freq,
                        self.hour,
                        self.minute,
                        self.second,
                        &self.rule,
                    )?;
                }
                Freq::Secondly => {
                    if filtered {
                        // Jump to one iteration before next day.
                        let total = self.hour * 3600 + self.minute * 60 + self.second;
                        self.second += (86399 - total).div_euclid(interval) * interval;
                    }
                    let rep_rate = 24 * 3600;
                    let mut valid = false;
                    for _ in 0..rep_rate / gcd(interval, rep_rate) {
                        let (nminutes, second) = if self.rule.bysecond.is_empty() {
                            let next = self.second.saturating_add(interval);
                            (next.div_euclid(60), next.rem_euclid(60))
                        } else {
                            self.mod_distance(self.second, &self.rule.bysecond, 60)?
                        };
                        self.second = second;
                        let (div, minute) = divmod_euclid(self.minute.saturating_add(nminutes), 60);
                        self.minute = minute;
                        if div != 0 {
                            self.hour = self.hour.saturating_add(div);
                            let (div, hour) = divmod_euclid(self.hour, 24);
                            self.hour = hour;
                            if div != 0 {
                                self.day = self.day.saturating_add(div);
                                fixday = true;
                            }
                        }
                        if (self.rule.byhour.is_empty() || self.rule.byhour.contains(&self.hour))
                            && (self.rule.byminute.is_empty()
                                || self.rule.byminute.contains(&self.minute))
                            && (self.rule.bysecond.is_empty()
                                || self.rule.bysecond.contains(&self.second))
                        {
                            valid = true;
                            break;
                        }
                    }
                    if !valid {
                        // 'Invalid combination of interval, byhour and
                        // byminute resulting in empty rule.'
                        return Err(FallbackError);
                    }
                    self.timeset = self.ii.gettimeset(
                        freq,
                        self.hour,
                        self.minute,
                        self.second,
                        &self.rule,
                    )?;
                }
            }

            // `fixday` (`rrule.py:1017-1030`): normalize an overflowing
            // day-of-month. dateutil walks month by month (hanging on
            // giant intervals); the same date falls out of ordinal
            // arithmetic. Past `MAXYEAR` the series ends.
            if fixday && self.day > 28 {
                let base = NaiveDate::from_ymd_opt(self.year, u32::from(self.month), 1)
                    .ok_or(FallbackError)?;
                let days = u64::try_from(self.day - 1).map_err(|_| FallbackError)?;
                let target = match base.checked_add_days(chrono::Days::new(days)) {
                    Some(t) if t.year() <= MAXYEAR => t,
                    _ => {
                        self.done = true;
                        return Ok(());
                    }
                };
                if (target.year(), target.month() as u8) != (self.year, self.month) {
                    self.year = target.year();
                    self.month = target.month() as u8;
                    self.ii.rebuild(self.year, self.month, &self.rule)?;
                }
                self.day = i64::from(target.day());
            }
            Ok(())
        }
    }

    /// Floored `divmod` for possibly-negative dividends.
    fn divmod_euclid(a: i64, b: i64) -> (i64, i64) {
        (a.div_euclid(b), a.rem_euclid(b))
    }

    /// A parsed bundle: rules plus one-off dates, mirroring `_parse_rfc`
    /// assembly (`rrule.py:1655-1729`). A lone rule is the one-rule case —
    /// an `rruleset` of a single rule yields identically.
    struct Bundle {
        rules: Vec<Rule>,
        rdates: Vec<BundleDate>,
        exrules: Vec<Rule>,
        exdates: Vec<BundleDate>,
    }

    impl Bundle {
        /// Append the caller's one-off extras (always aware UTC, as
        /// `_rrule.py` attaches UTC to naive extras before assembling).
        fn push_caller_extras(&mut self, rdates: &[DateTime<Utc>], exdates: &[DateTime<Utc>]) {
            self.rdates.extend(rdates.iter().map(|d| BundleDate {
                dt: d.naive_utc(),
                aware: true,
            }));
            self.exdates.extend(exdates.iter().map(|d| BundleDate {
                dt: d.naive_utc(),
                aware: true,
            }));
        }
    }

    /// Parse one `RDATE`/`EXDATE`/`DTSTART` line's values
    /// (`_parse_date_value`, `rrule.py:1563-1613`): `TZID=` resolves UTC
    /// and GMT only (anything else is unsupported here — dateutil would
    /// attach the zone or, for unknown names, ignore it);
    /// `VALUE=DATE(-TIME)` may appear at most once. Years stay within
    /// dateutil's representable range (Python datetimes stop at 9999).
    fn parse_date_line(parms: &[&str], value: &str) -> Result<Vec<BundleDate>> {
        let mut utc_zone = false;
        let mut value_seen = false;
        for parm in parms {
            if let Some(name) = parm.strip_prefix("TZID=") {
                if name == "UTC" || name == "GMT" {
                    utc_zone = true;
                    continue;
                }
                return Err(FallbackError);
            }
            if *parm != "VALUE=DATE-TIME" && *parm != "VALUE=DATE" {
                return Err(FallbackError);
            }
            if value_seen {
                return Err(FallbackError);
            }
            value_seen = true;
        }
        let mut out = Vec::new();
        for datestr in value.split(',') {
            let mut date = parse_bundle_date(datestr)?;
            if !(1..=MAXYEAR).contains(&date.dt.year()) {
                return Err(FallbackError);
            }
            if utc_zone {
                // `DTSTART/EXDATE specifies multiple timezone`.
                if date.aware {
                    return Err(FallbackError);
                }
                date.aware = true;
            }
            out.push(date);
        }
        Ok(out)
    }

    /// `_parse_rfc` (`rrule.py:1615-1729`) minus `unfold`/`forceset`/
    /// `compatible` (never set by our callers): the bundle string plus the
    /// caller's anchor and one-off extras into rules and dates.
    fn parse_bundle(
        dtstart: NaiveDateTime,
        rrule_str: &str,
        rdates: &[DateTime<Utc>],
        exdates: &[DateTime<Utc>],
    ) -> Result<Bundle> {
        let upper = rrule_str.to_uppercase();
        if upper.trim().is_empty() {
            return Err(FallbackError);
        }
        let lines: Vec<&str> = upper.split_whitespace().collect();
        // The caller's anchor is always aware UTC.
        let mut anchor = BundleDate {
            dt: dtstart,
            aware: true,
        };
        if lines.len() == 1 && (!lines[0].contains(':') || lines[0].starts_with("RRULE:")) {
            let rule = parse_rule_line(lines[0], anchor.dt, anchor.aware)?;
            let mut bundle = Bundle {
                rules: vec![rule],
                rdates: Vec::new(),
                exrules: Vec::new(),
                exdates: Vec::new(),
            };
            bundle.push_caller_extras(rdates, exdates);
            return Ok(bundle);
        }
        let mut rrulevals: Vec<&str> = Vec::new();
        let mut rdatevals: Vec<&str> = Vec::new();
        let mut exrulevals: Vec<&str> = Vec::new();
        let mut exdatevals: Vec<BundleDate> = Vec::new();
        for line in &lines {
            let (name, value, parms) = if !line.contains(':') {
                ("RRULE", *line, Vec::new())
            } else {
                let (head, value) = line.split_once(':').ok_or(FallbackError)?;
                let mut head_parts = head.split(';');
                let name = head_parts.next().unwrap_or("");
                (name, value, head_parts.collect::<Vec<_>>())
            };
            match name {
                "RRULE" => {
                    if !parms.is_empty() {
                        return Err(FallbackError);
                    }
                    rrulevals.push(value);
                }
                "RDATE" => {
                    // Only `VALUE=DATE-TIME` is allowed here — `VALUE=DATE`
                    // raises (unlike `EXDATE`/`DTSTART`).
                    for parm in &parms {
                        if *parm != "VALUE=DATE-TIME" {
                            return Err(FallbackError);
                        }
                    }
                    rdatevals.push(value);
                }
                "EXRULE" => {
                    if !parms.is_empty() {
                        return Err(FallbackError);
                    }
                    exrulevals.push(value);
                }
                "EXDATE" => {
                    exdatevals.extend(parse_date_line(&parms, value)?);
                }
                "DTSTART" => {
                    let dtvals = parse_date_line(&parms, value)?;
                    if dtvals.len() != 1 {
                        return Err(FallbackError);
                    }
                    anchor = dtvals[0];
                }
                _ => return Err(FallbackError),
            }
        }
        if rrulevals.len() > 1
            || !rdatevals.is_empty()
            || !exrulevals.is_empty()
            || !exdatevals.is_empty()
        {
            // Set assembly: rules parse before dates (all errors map to
            // `None` here, so the order is unobservable).
            let mut rules = Vec::with_capacity(rrulevals.len());
            for value in rrulevals {
                rules.push(parse_rule_line(value, anchor.dt, anchor.aware)?);
            }
            let mut bundle_rdates = Vec::new();
            for value in rdatevals {
                for datestr in value.split(',') {
                    let date = parse_bundle_date(datestr)?;
                    if !(1..=MAXYEAR).contains(&date.dt.year()) {
                        return Err(FallbackError);
                    }
                    bundle_rdates.push(date);
                }
            }
            let mut exrules = Vec::with_capacity(exrulevals.len());
            for value in exrulevals {
                exrules.push(parse_rule_line(value, anchor.dt, anchor.aware)?);
            }
            let mut bundle = Bundle {
                rules,
                rdates: bundle_rdates,
                exrules,
                exdates: exdatevals,
            };
            bundle.push_caller_extras(rdates, exdates);
            Ok(bundle)
        } else {
            // `rrulevals[0]` — empty is dateutil's `IndexError` (a lone
            // `DTSTART` line), which also maps to "no expansion" here.
            let value = rrulevals.first().ok_or(FallbackError)?;
            let rule = parse_rule_line(value, anchor.dt, anchor.aware)?;
            let mut bundle = Bundle {
                rules: vec![rule],
                rdates: Vec::new(),
                exrules: Vec::new(),
                exdates: Vec::new(),
            };
            bundle.push_caller_extras(rdates, exdates);
            Ok(bundle)
        }
    }

    /// One merge stream: pending one-off dates or a rule iterator.
    enum StreamGen {
        Dates(std::vec::IntoIter<BundleDate>),
        Rule(Box<RruleIter>),
    }

    struct Stream {
        head: Option<BundleDate>,
        gen: StreamGen,
    }

    impl StreamGen {
        /// Pull the next item: `Ok(None)` on exhaustion, `Err` on rule
        /// failure (an exception escaping the generator).
        fn pull(&mut self) -> Result<Option<BundleDate>> {
            match self {
                StreamGen::Dates(dates) => Ok(dates.next()),
                StreamGen::Rule(iter) => {
                    let next = iter.next();
                    if next.is_none() && iter.failed() {
                        return Err(FallbackError);
                    }
                    Ok(next.map(|dt| BundleDate {
                        dt,
                        aware: iter.rule.aware,
                    }))
                }
            }
        }
    }

    impl Stream {
        fn from_dates(dates: Vec<BundleDate>) -> Option<Self> {
            let mut gen = StreamGen::Dates(dates.into_iter());
            // Pulling plain dates never fails; an empty stream is skipped,
            // as `_genitem` swallows the initial `StopIteration`.
            match gen.pull() {
                Ok(Some(head)) => Some(Self {
                    head: Some(head),
                    gen,
                }),
                _ => None,
            }
        }

        fn from_rule(rule: Rule) -> Result<Option<Self>> {
            let mut gen = StreamGen::Rule(Box::new(RruleIter::new(rule)?));
            match gen.pull()? {
                Some(head) => Ok(Some(Self {
                    head: Some(head),
                    gen,
                })),
                None => Ok(None),
            }
        }

        /// Advance past the head; `Ok(false)` when exhausted.
        fn advance(&mut self) -> Result<bool> {
            let next = self.gen.pull()?;
            self.head = next;
            Ok(next.is_some())
        }
    }

    /// `<` on merge items: awareness must match (dateutil raises
    /// `TypeError` otherwise).
    fn cmp_merge(a: BundleDate, b: BundleDate) -> Result<std::cmp::Ordering> {
        if a.aware != b.aware {
            return Err(FallbackError);
        }
        Ok(a.dt.cmp(&b.dt))
    }

    /// Index of the stream with the least head. Mixed naive/aware heads
    /// fail (dateutil's `TypeError` on `<`).
    fn min_head(streams: &[Stream]) -> Result<Option<usize>> {
        let mut best: Option<(usize, BundleDate)> = None;
        for (k, stream) in streams.iter().enumerate() {
            let head = stream.head.ok_or(FallbackError)?;
            match best {
                None => best = Some((k, head)),
                Some((_, current)) => {
                    if cmp_merge(head, current)? == std::cmp::Ordering::Less {
                        best = Some((k, head));
                    }
                }
            }
        }
        Ok(best.map(|(k, _)| k))
    }

    /// Sort one-off dates, failing on mixed naive/aware input exactly as
    /// `rruleset._iter`'s `list.sort()` raises `TypeError` there.
    fn sort_merge_dates(dates: &mut [BundleDate]) -> Result<()> {
        if dates.iter().any(|d| d.aware) && dates.iter().any(|d| !d.aware) {
            return Err(FallbackError);
        }
        dates.sort_by_key(|d| d.dt);
        Ok(())
    }

    /// `rruleset._iter` (`rrule.py:1383-1413`): the merged, deduplicated,
    /// exclusion-filtered stream. Linear minimum scans (streams are few)
    /// instead of heaps — the same order, without the `heapreplace`
    /// dance. `failed` mirrors an exception escaping the merge.
    struct SetIter {
        rstreams: Vec<Stream>,
        exstreams: Vec<Stream>,
        lastdt: Option<BundleDate>,
        failed: bool,
    }

    impl SetIter {
        fn new(mut bundle: Bundle) -> Result<Self> {
            sort_merge_dates(&mut bundle.rdates)?;
            sort_merge_dates(&mut bundle.exdates)?;
            let mut rstreams = Vec::new();
            if !bundle.rdates.is_empty() {
                if let Some(stream) = Stream::from_dates(bundle.rdates) {
                    rstreams.push(stream);
                }
            }
            for rule in bundle.rules {
                if let Some(stream) = Stream::from_rule(rule)? {
                    rstreams.push(stream);
                }
            }
            let mut exstreams = Vec::new();
            if !bundle.exdates.is_empty() {
                if let Some(stream) = Stream::from_dates(bundle.exdates) {
                    exstreams.push(stream);
                }
            }
            for rule in bundle.exrules {
                if let Some(stream) = Stream::from_rule(rule)? {
                    exstreams.push(stream);
                }
            }
            Ok(Self {
                rstreams,
                exstreams,
                lastdt: None,
                failed: false,
            })
        }

        fn failed(&self) -> bool {
            self.failed
        }

        fn next(&mut self) -> Option<BundleDate> {
            if self.failed {
                return None;
            }
            match self.next_inner() {
                Ok(item) => item,
                Err(_) => {
                    self.failed = true;
                    None
                }
            }
        }

        /// One `while rlist` pass: the minimum head, deduplicated and
        /// exclusion-filtered — or `None` when the inclusive side ends.
        fn next_inner(&mut self) -> Result<Option<BundleDate>> {
            loop {
                let Some(r) = min_head(&self.rstreams)? else {
                    return Ok(None);
                };
                let ritem = self.rstreams[r].head.ok_or(FallbackError)?;
                if self.lastdt.is_none_or(|last| last != ritem) {
                    // Drain exclusions below the candidate.
                    while let Some(ex) = min_head(&self.exstreams)? {
                        let exitem = self.exstreams[ex].head.ok_or(FallbackError)?;
                        if cmp_merge(exitem, ritem)? != std::cmp::Ordering::Less {
                            break;
                        }
                        if !self.exstreams[ex].advance()? {
                            self.exstreams.remove(ex);
                        }
                    }
                    // Yield unless excluded (`!=` never raises).
                    let excluded = match min_head(&self.exstreams)? {
                        Some(ex) => self.exstreams[ex].head.ok_or(FallbackError)? == ritem,
                        None => false,
                    };
                    self.lastdt = Some(ritem);
                    // The winning stream advances at the end of every pass.
                    if !self.rstreams[r].advance()? {
                        self.rstreams.remove(r);
                    }
                    if !excluded {
                        return Ok(Some(ritem));
                    }
                } else if !self.rstreams[r].advance()? {
                    // Duplicate of the last yield: advance and loop.
                    self.rstreams.remove(r);
                }
            }
        }
    }

    /// Fallback `next_fire_with_error`: the next bundle instant strictly
    /// after `now` (`Ok(Some)`), nothing left after `now` (`Ok(None)`), or
    /// any parse/expansion failure (`Err`, dateutil's `except` path). A
    /// naive item against the aware `now` fails (dateutil's `TypeError` in
    /// `after`), and so does a failure mid-iteration — never read as an
    /// exhausted series.
    pub fn next_fire_with_error(
        dtstart: DateTime<Utc>,
        rrule_str: &str,
        rdates: &[DateTime<Utc>],
        exdates: &[DateTime<Utc>],
        now: DateTime<Utc>,
    ) -> std::result::Result<Option<DateTime<Utc>>, String> {
        const FAILED: &str = "rrule expansion failed";
        // Python datetimes stop at year 9999; anything outside has no
        // dateutil oracle and is a parse error on the crate path too.
        if !(1..=MAXYEAR).contains(&dtstart.year()) {
            return Err(FAILED.to_owned());
        }
        let bundle = parse_bundle(dtstart.naive_utc(), rrule_str, rdates, exdates)
            .map_err(|_| FAILED.to_owned())?;
        let mut iter = SetIter::new(bundle).map_err(|_| FAILED.to_owned())?;
        let base = now.naive_utc();
        while let Some(item) = iter.next() {
            if !item.aware {
                return Err(FAILED.to_owned());
            }
            if item.dt > base {
                return Ok(Some(item.dt.and_utc()));
            }
        }
        if iter.failed() {
            Err(FAILED.to_owned())
        } else {
            Ok(None)
        }
    }

    /// Fallback `occurrences_between`: bundle instants in
    /// `[window_start, window_end]` plus the cap flag. Any failure
    /// discards the partial list (`([], false)`), as `_rrule.py`'s
    /// `except` does.
    pub fn occurrences(
        dtstart: DateTime<Utc>,
        rrule_str: &str,
        rdates: &[DateTime<Utc>],
        exdates: &[DateTime<Utc>],
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        cap: usize,
    ) -> (Vec<DateTime<Utc>>, bool) {
        if !(1..=MAXYEAR).contains(&dtstart.year()) {
            return (Vec::new(), false);
        }
        let bundle = match parse_bundle(dtstart.naive_utc(), rrule_str, rdates, exdates) {
            Ok(bundle) => bundle,
            Err(_) => return (Vec::new(), false),
        };
        let mut iter = match SetIter::new(bundle) {
            Ok(iter) => iter,
            Err(_) => return (Vec::new(), false),
        };
        let start = window_start.naive_utc();
        let end = window_end.naive_utc();
        let mut out = Vec::new();
        while let Some(item) = iter.next() {
            // A naive item against the aware window raises `TypeError`.
            if !item.aware {
                return (Vec::new(), false);
            }
            if item.dt < start {
                continue;
            }
            if item.dt > end {
                break;
            }
            out.push(item.dt.and_utc());
            if out.len() >= cap {
                return (out, true);
            }
        }
        if iter.failed() {
            (Vec::new(), false)
        } else {
            (out, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    /// Parse a golden `+00:00` instant the way the fixture records it.
    fn golden(s: &str) -> DateTime<Utc> {
        s.parse::<DateTime<Utc>>().unwrap()
    }

    /// Every golden instant, byte for byte in DRF/ISO UTC (`+00:00`) form.
    fn assert_golden(dt: DateTime<Utc>, expected: &str) {
        assert_eq!(dt, golden(expected));
        assert_eq!(dt.to_rfc3339(), expected);
    }

    // FX-TICKER-02: `rrule/cron_to_rrule.golden.json`
    // (`bgtasks/_rrule.py:125-212`; FREQ picker 159-175; `*/N` INTERVAL
    // 179-195; BY* clauses 196-210; grammar 67-122; DOW `7→0` 147-149;
    // DOM+DOW rejection 151-157).
    #[test]
    fn cron_to_rrule_matches_fx_ticker_02() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/tasks_ticker/rrule/cron_to_rrule.golden.json"
        ))
        .unwrap();
        let vectors = fixture["vectors"].as_array().unwrap();
        assert!(!vectors.is_empty());
        for vector in vectors {
            let cron = vector["cron"].as_str().unwrap();
            match cron_to_rrule(cron) {
                Ok(rrule) => {
                    assert_eq!(rrule, vector["rrule"].as_str().unwrap(), "cron {cron:?}");
                    assert!(vector.get("error").is_none(), "cron {cron:?}");
                }
                Err(e) => {
                    assert_eq!(
                        e.message(),
                        vector["message"].as_str().unwrap(),
                        "cron {cron:?}"
                    );
                    assert!(
                        vector["error"].as_str().unwrap() == "CronConversionError",
                        "cron {cron:?}"
                    );
                }
            }
        }
    }

    // FX-TICKER-02: `rrule/validate.golden.json` (`bgtasks/_rrule.py:218-263`;
    // empty ok 230-231; SECONDLY rejected 256-259).
    #[test]
    fn validate_rrule_string_matches_fx_ticker_02() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/tasks_ticker/rrule/validate.golden.json"
        ))
        .unwrap();
        let vectors = fixture["vectors"].as_array().unwrap();
        assert!(!vectors.is_empty());
        for vector in vectors {
            let rrule = vector["rrule"].as_str().unwrap();
            match validate_rrule_string(rrule) {
                Ok(()) => assert!(
                    vector["valid"].as_bool().unwrap(),
                    "rrule {rrule:?} unexpectedly valid"
                ),
                Err(e) => {
                    assert!(
                        !vector["valid"].as_bool().unwrap(),
                        "rrule {rrule:?} unexpectedly invalid: {e}"
                    );
                    assert_eq!(
                        e.message(),
                        vector["message"].as_str().unwrap(),
                        "rrule {rrule:?}"
                    );
                    assert!(
                        vector["error"].as_str().unwrap() == "RRuleValidationError",
                        "rrule {rrule:?}"
                    );
                }
            }
        }
    }

    /// Reconstructed inputs for the `next_fire` golden vectors
    /// (`rrule/next_fire.golden.json`, `_rrule.py:269-336`; outputs only in
    /// the fixture, inputs from `test_rrule.py` — same fixed instants).
    /// Tuple: `(dtstart, rrule, tzid, rdates, exdates, now)`.
    type NextFireCase = (
        DateTime<Utc>,
        &'static str,
        &'static str,
        Vec<DateTime<Utc>>,
        Vec<DateTime<Utc>>,
        DateTime<Utc>,
    );

    fn next_fire_input(name: &str) -> NextFireCase {
        let daily = "FREQ=DAILY;BYHOUR=9;BYMINUTE=0";
        match name {
            // test_next_fire_simple_daily
            "daily" => (
                utc(2026, 1, 1, 9, 0),
                daily,
                "UTC",
                vec![],
                vec![],
                utc(2026, 1, 1, 12, 0),
            ),
            // test_next_fire_minutely_interval
            "minutely_interval" => (
                utc(2026, 1, 1, 0, 0),
                "FREQ=MINUTELY;INTERVAL=5",
                "UTC",
                vec![],
                vec![],
                utc(2026, 1, 1, 0, 12),
            ),
            // test_next_fire_single_shot_empty_rrule
            "single_shot_ahead" => (
                utc(2026, 6, 1, 9, 0),
                "",
                "UTC",
                vec![],
                vec![],
                utc(2026, 1, 1, 0, 0),
            ),
            // test_next_fire_single_shot_in_the_past
            "single_shot_past" => (
                utc(2026, 1, 1, 9, 0),
                "",
                "UTC",
                vec![],
                vec![],
                utc(2026, 6, 1, 0, 0),
            ),
            // test_next_fire_honors_exdates
            "exdates_skip" => (
                utc(2026, 1, 1, 9, 0),
                daily,
                "UTC",
                vec![],
                vec![utc(2026, 1, 2, 9, 0)],
                utc(2026, 1, 1, 12, 0),
            ),
            // test_next_fire_honors_rdates
            "rdates_append" => (
                utc(2026, 1, 1, 9, 0),
                daily,
                "UTC",
                vec![utc(2026, 1, 1, 15, 0)],
                vec![],
                utc(2026, 1, 1, 12, 0),
            ),
            // single-shot with an explicit one-off after now (`:321-328`)
            "single_shot_rdate_ahead" => (
                utc(2026, 1, 1, 9, 0),
                "",
                "UTC",
                vec![utc(2026, 1, 5, 9, 0)],
                vec![],
                utc(2026, 1, 2, 0, 0),
            ),
            // test_next_fire_returns_none_on_bad_rrule
            "parse_error" => (
                utc(2026, 1, 1, 9, 0),
                "garbage",
                "UTC",
                vec![],
                vec![],
                utc(2026, 1, 1, 0, 0),
            ),
            // tzid informational: same series, UTC vs America/New_York give
            // the same instant (`_rrule.py:290-293`)
            "tzid_informational_utc" => (
                utc(2026, 1, 1, 9, 0),
                daily,
                "UTC",
                vec![],
                vec![],
                utc(2026, 1, 1, 12, 0),
            ),
            "tzid_informational_other" => (
                utc(2026, 1, 1, 9, 0),
                daily,
                "America/New_York",
                vec![],
                vec![],
                utc(2026, 1, 1, 12, 0),
            ),
            // BUG-TICKER-1 quirk (`_rrule.py:323`): a naive exdate never
            // excludes on the single-shot path. Naive instants are
            // unrepresentable as `DateTime<Utc>`, so this asserts the pinned
            // outcome — the firing is NOT excluded — with no exdate, plus
            // the note's counterpart: an aware exdate at the same instant
            // excludes (→ None, asserted by
            // `single-shot-aware-exdate-excluded`).
            "quirk-single-shot-naive-exdate-no-match" => (
                utc(2026, 6, 1, 9, 0),
                "",
                "UTC",
                vec![],
                vec![],
                utc(2026, 1, 1, 0, 0),
            ),
            "single-shot-aware-exdate-excluded" => (
                utc(2026, 6, 1, 9, 0),
                "",
                "UTC",
                vec![],
                vec![utc(2026, 6, 1, 9, 0)],
                utc(2026, 1, 1, 0, 0),
            ),
            // PIDASHCONV-1300: rules the `rrule` crate refuses that
            // dateutil expands (dtstart 2026-10-19T12:00Z, the issue's
            // anchor; `now` is the anchor throughout).
            "r1300-weekly-bymonthday" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=WEEKLY;BYDAY=FR;BYMONTHDAY=13",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-daily-byyearday" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=DAILY;BYYEARDAY=-1",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-monthly-byweekno" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=MONTHLY;BYWEEKNO=20",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-interval-wide" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=DAILY;INTERVAL=70000",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-dup-keys-last-wins" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=DAILY;BYHOUR=9;BYHOUR=10",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-bysetpos-wide" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR,SA,SU;BYHOUR=0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23;BYSETPOS=100",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-byeaster-weekly" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=WEEKLY;BYEASTER=1",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-byday-nth-weekly" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=WEEKLY;BYDAY=+10MO",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-count-wide" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=DAILY;COUNT=5000000000",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-until-past-rdate" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=DAILY;UNTIL=20200101T000000Z",
                "UTC",
                vec![Utc.with_ymd_and_hms(2026, 11, 1, 9, 0, 0).unwrap()],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-weekly-byyearday" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=WEEKLY;BYYEARDAY=100",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            "r1300-monthly-byweekno-wkst" => (
                utc(2026, 10, 19, 12, 0),
                "FREQ=MONTHLY;BYWEEKNO=20;WKST=SU",
                "UTC",
                vec![],
                vec![],
                utc(2026, 10, 19, 12, 0),
            ),
            // Review: `rrule.__init__` truncates a sub-second anchor
            // (`dtstart.replace(microsecond=0)`), so the truncated fire
            // itself is yielded.
            "r1300-dtstart-subsecond" => (
                Utc.with_ymd_and_hms(2026, 11, 13, 12, 0, 0)
                    .unwrap()
                    .with_nanosecond(500_000_000)
                    .unwrap(),
                "FREQ=WEEKLY;BYDAY=FR;BYMONTHDAY=13",
                "UTC",
                vec![],
                vec![],
                utc(2026, 11, 1, 0, 0),
            ),
            _ => panic!("unknown next_fire vector: {name}"),
        }
    }

    // FX-TICKER-02: `rrule/next_fire.golden.json` (`_rrule.py:269-336`).
    #[test]
    fn next_fire_from_rrule_matches_fx_ticker_02() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/tasks_ticker/rrule/next_fire.golden.json"
        ))
        .unwrap();
        let vectors = fixture["vectors"].as_array().unwrap();
        assert!(!vectors.is_empty());
        for vector in vectors {
            let name = vector["name"].as_str().unwrap();
            let (dtstart, rrule, tzid, rdates, exdates, now) = next_fire_input(name);
            let next = next_fire_from_rrule(dtstart, rrule, tzid, &rdates, &exdates, now);
            match vector.get("next") {
                Some(serde_json::Value::String(expected)) => {
                    assert_golden(next.unwrap(), expected);
                }
                _ => assert!(next.is_none(), "vector {name}: expected None"),
            }
        }
    }

    // FX-TICKER-02: `rrule/occurrences.golden.json` (`_rrule.py:339-406`;
    // lazy `xafter` loop 386-393; cap/`has_more` 392-393).
    #[test]
    fn occurrences_between_matches_fx_ticker_02() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../fixtures/tasks_ticker/rrule/occurrences.golden.json"
        ))
        .unwrap();

        // test_occurrences_between_daily_window: Jan 1–7 at 09:00.
        let (out, has_more) = occurrences_between(
            utc(2026, 1, 1, 9, 0),
            "FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
            "UTC",
            &[],
            &[],
            utc(2026, 1, 1, 0, 0),
            Utc.with_ymd_and_hms(2026, 1, 7, 23, 59, 0).unwrap(),
            5000,
        );
        assert_eq!(
            out.len(),
            fixture["daily_window_count"].as_u64().unwrap() as usize
        );
        assert_eq!(
            has_more,
            fixture["daily_window_has_more"].as_bool().unwrap()
        );
        assert_golden(out[0], fixture["daily_window_first"].as_str().unwrap());
        assert_golden(
            out[out.len() - 1],
            fixture["daily_window_last"].as_str().unwrap(),
        );

        // test_occurrences_between_honors_cap: FREQ=MINUTELY over 10 days.
        let (out, has_more) = occurrences_between(
            utc(2026, 1, 1, 0, 0),
            "FREQ=MINUTELY",
            "UTC",
            &[],
            &[],
            utc(2026, 1, 1, 0, 0),
            utc(2026, 1, 11, 0, 0),
            100,
        );
        assert_eq!(
            out.len(),
            fixture["minutely_cap_count"].as_u64().unwrap() as usize
        );
        assert_eq!(
            has_more,
            fixture["minutely_cap_has_more"].as_bool().unwrap()
        );

        // PIDASHCONV-1300: Friday-the-13ths (`FREQ=WEEKLY;BYDAY=FR;
        // BYMONTHDAY=13`, which the `rrule` crate refuses) over
        // 2026-10-19–2028-01-01.
        let (out, has_more) = occurrences_between(
            utc(2026, 10, 19, 12, 0),
            "FREQ=WEEKLY;BYDAY=FR;BYMONTHDAY=13",
            "UTC",
            &[],
            &[],
            utc(2026, 10, 19, 12, 0),
            utc(2028, 1, 1, 0, 0),
            5000,
        );
        assert_eq!(
            out.len(),
            fixture["friday13_window_count"].as_u64().unwrap() as usize
        );
        assert_eq!(
            has_more,
            fixture["friday13_window_has_more"].as_bool().unwrap()
        );
        assert_golden(out[0], fixture["friday13_window_first"].as_str().unwrap());
        assert_golden(
            out[out.len() - 1],
            fixture["friday13_window_last"].as_str().unwrap(),
        );

        // Review: a sub-second anchor still yields its truncated fire
        // (`dtstart.replace(microsecond=0)` in `rrule.__init__`).
        let (out, has_more) = occurrences_between(
            Utc.with_ymd_and_hms(2026, 11, 13, 12, 0, 0)
                .unwrap()
                .with_nanosecond(500_000_000)
                .unwrap(),
            "FREQ=WEEKLY;BYDAY=FR;BYMONTHDAY=13",
            "UTC",
            &[],
            &[],
            utc(2026, 11, 1, 0, 0),
            utc(2026, 12, 1, 0, 0),
            5000,
        );
        assert_eq!(
            out.len(),
            fixture["friday13_micro_window_count"].as_u64().unwrap() as usize
        );
        assert_eq!(
            has_more,
            fixture["friday13_micro_window_has_more"].as_bool().unwrap()
        );
        assert_golden(
            out[0],
            fixture["friday13_micro_window_first"].as_str().unwrap(),
        );
        assert_golden(
            out[out.len() - 1],
            fixture["friday13_micro_window_last"].as_str().unwrap(),
        );

        // Single-shot in window (`:394-402`).
        let (out, has_more) = occurrences_between(
            utc(2026, 1, 3, 9, 0),
            "",
            "UTC",
            &[],
            &[],
            utc(2026, 1, 1, 0, 0),
            Utc.with_ymd_and_hms(2026, 1, 7, 23, 59, 0).unwrap(),
            5000,
        );
        let expected: Vec<DateTime<Utc>> = fixture["single_shot_in_window"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| golden(v.as_str().unwrap()))
            .collect();
        assert_eq!(out, expected);
        assert_eq!(has_more, fixture["single_shot_has_more"].as_bool().unwrap());

        // BUG-TICKER-1 quirk (`_rrule.py:395,400`): a naive exdate does not
        // exclude the single-shot firing. Naive is unrepresentable here, so
        // the pinned count (1, not excluded) is asserted with no exdate —
        // and the representable counterpart (an aware exdate at the firing
        // instant) excludes exactly.
        let (out, _) = occurrences_between(
            utc(2026, 1, 3, 9, 0),
            "",
            "UTC",
            &[],
            &[],
            utc(2026, 1, 1, 0, 0),
            Utc.with_ymd_and_hms(2026, 1, 7, 23, 59, 0).unwrap(),
            5000,
        );
        assert_eq!(
            out.len(),
            fixture["single_shot_naive_exdate_count"].as_u64().unwrap() as usize
        );
        let (out, _) = occurrences_between(
            utc(2026, 1, 3, 9, 0),
            "",
            "UTC",
            &[],
            &[utc(2026, 1, 3, 9, 0)],
            utc(2026, 1, 1, 0, 0),
            Utc.with_ymd_and_hms(2026, 1, 7, 23, 59, 0).unwrap(),
            5000,
        );
        assert!(out.is_empty());
    }

    // Strictly-after-`now`: an occurrence exactly at `now` is not returned.
    #[test]
    fn next_fire_is_strictly_after_now() {
        let next = next_fire_from_rrule(
            utc(2026, 1, 1, 9, 0),
            "FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
            "UTC",
            &[],
            &[],
            utc(2026, 1, 2, 9, 0),
        );
        assert_golden(next.unwrap(), "2026-01-03T09:00:00+00:00");
    }

    // PIDASHCONV-1299: paren-form `BYDAY` with a non-ASCII tail
    // (`MO(1€`) expands as `+1MO` (first Monday), like `dateutil` —
    // the raw item used to panic the crate's byte-slicing parser.
    #[test]
    fn paren_byday_non_ascii_tail_expands_as_first_monday() {
        let rule = "FREQ=MONTHLY;BYDAY=MO(1\u{20ac}";
        assert!(validate_rrule_string(rule).is_ok());
        let dtstart = utc(2026, 11, 1, 9, 0);
        let next = next_fire_from_rrule(dtstart, rule, "UTC", &[], &[], utc(2026, 11, 1, 0, 0));
        assert_golden(next.unwrap(), "2026-11-02T09:00:00+00:00");
        let (out, has_more) = occurrences_between(
            dtstart,
            rule,
            "UTC",
            &[],
            &[],
            utc(2026, 11, 1, 0, 0),
            utc(2026, 12, 31, 0, 0),
            100,
        );
        assert!(!has_more);
        assert_eq!(out.len(), 2);
        assert_golden(out[0], "2026-11-02T09:00:00+00:00");
        assert_golden(out[1], "2026-12-07T09:00:00+00:00");
    }

    // PIDASHCONV-1299: ASCII paren form (`TH(+1)`) expands as `+1TH`
    // (first Thursday), like `dateutil` — the crate used to read weekday
    // `"1)"` and fail, leaving `next_run_at` NULL.
    #[test]
    fn paren_byday_ascii_expands_as_first_thursday() {
        let rule = "FREQ=MONTHLY;BYDAY=TH(+1)";
        assert!(validate_rrule_string(rule).is_ok());
        let dtstart = utc(2026, 11, 1, 9, 0);
        let next = next_fire_from_rrule(dtstart, rule, "UTC", &[], &[], utc(2026, 11, 1, 0, 0));
        assert_golden(next.unwrap(), "2026-11-05T09:00:00+00:00");
        let (out, has_more) = occurrences_between(
            dtstart,
            rule,
            "UTC",
            &[],
            &[],
            utc(2026, 11, 1, 0, 0),
            utc(2026, 12, 31, 0, 0),
            100,
        );
        assert!(!has_more);
        assert_eq!(out.len(), 2);
        assert_golden(out[0], "2026-11-05T09:00:00+00:00");
        assert_golden(out[1], "2026-12-03T09:00:00+00:00");
    }

    // PIDASHCONV-1299: a zero ordinal is a parse error in `dateutil`
    // (`weekday` rejects `n == 0`), so expansion returns `None` — never a
    // bare weekday, which the crate would expand as every-weekday.
    #[test]
    fn paren_byday_zero_ordinal_is_parse_error() {
        let dtstart = utc(2026, 11, 1, 9, 0);
        assert!(next_fire_from_rrule(
            dtstart,
            "FREQ=MONTHLY;BYDAY=MO(0)",
            "UTC",
            &[],
            &[],
            utc(2026, 11, 1, 0, 0)
        )
        .is_none());
        let (out, has_more) = occurrences_between(
            dtstart,
            "FREQ=MONTHLY;BYDAY=MO(0)",
            "UTC",
            &[],
            &[],
            utc(2026, 11, 1, 0, 0),
            utc(2026, 12, 31, 0, 0),
            100,
        );
        assert!(out.is_empty() && !has_more);
    }

    // PIDASHCONV-1299: negative, multi-item, lowercase and `BYWEEKDAY`
    // paren forms canonicalize the same way (values verified against
    // live `dateutil` 2.8.2).
    #[test]
    fn paren_byday_negative_multi_lowercase_and_byweekday() {
        let dtstart = utc(2026, 11, 1, 9, 0);
        let now = utc(2026, 11, 1, 0, 0);
        for (rule, expected) in [
            ("FREQ=MONTHLY;BYDAY=MO(-1)", "2026-11-30T09:00:00+00:00"),
            (
                "FREQ=MONTHLY;BYDAY=MO(+1),TU(-2)",
                "2026-11-02T09:00:00+00:00",
            ),
            ("freq=monthly;byday=th(+1)", "2026-11-05T09:00:00+00:00"),
            ("FREQ=MONTHLY;BYWEEKDAY=WE(+2)", "2026-11-11T09:00:00+00:00"),
            (
                "RRULE:FREQ=MONTHLY;BYDAY=TH(+1)",
                "2026-11-05T09:00:00+00:00",
            ),
        ] {
            let next = next_fire_from_rrule(dtstart, rule, "UTC", &[], &[], now);
            assert_golden(next.unwrap(), expected);
        }
    }

    // `None` on parse error, never a panic — for both expansion entries.
    #[test]
    fn expansion_parse_errors_return_none_without_panic() {
        let dtstart = utc(2026, 1, 1, 9, 0);
        assert!(next_fire_from_rrule(dtstart, "garbage", "UTC", &[], &[], dtstart).is_none());
        assert!(
            next_fire_from_rrule(dtstart, "FREQ=DAILY;BYHOUR", "UTC", &[], &[], dtstart).is_none()
        );
        let (out, has_more) = occurrences_between(
            dtstart,
            "garbage",
            "UTC",
            &[],
            &[],
            dtstart,
            utc(2026, 2, 1, 0, 0),
            5000,
        );
        assert!(out.is_empty() && !has_more);
    }

    // `next_fire_with_error` tells an exhausted series (`Ok(None)`) from a
    // parse error (`Err`); `next_fire_from_rrule` folds both into `None`.
    #[test]
    fn next_fire_with_error_splits_exhausted_from_invalid() {
        let dtstart = Utc.with_ymd_and_hms(2026, 9, 28, 11, 58, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap();
        let next = |rrule: &str| next_fire_with_error(dtstart, rrule, "UTC", &[], &[], now);
        assert_eq!(next("FREQ=MINUTELY;COUNT=2"), Ok(None));
        assert_eq!(next("FREQ=MINUTELY;UNTIL=20260928T115900Z"), Ok(None));
        assert_eq!(next(""), Ok(None));
        assert_eq!(
            next("FREQ=MINUTELY;COUNT=5"),
            Ok(Some(Utc.with_ymd_and_hms(2026, 9, 28, 12, 1, 0).unwrap()))
        );
        assert!(next("garbage").is_err());
        assert!(next("FREQ=DAILY;BYHOUR").is_err());
        // Rules the crate refuses go through the dateutil-faithful fallback
        // (PIDASHCONV-1300); it must make the same split.
        let jan1 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let fallback = |rrule: &str| next_fire_with_error(jan1, rrule, "UTC", &[], &[], now);
        assert_eq!(fallback("FREQ=DAILY;BYYEARDAY=1;COUNT=1"), Ok(None));
        assert_eq!(
            fallback("FREQ=DAILY;BYYEARDAY=1;COUNT=2"),
            Ok(Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()))
        );
        assert_eq!(
            fallback("FREQ=WEEKLY;BYMONTHDAY=1;UNTIL=20260601T000000Z"),
            Ok(None)
        );
        assert_eq!(
            next_fire_from_rrule(dtstart, "FREQ=MINUTELY;COUNT=2", "UTC", &[], &[], now),
            None
        );
    }
}
