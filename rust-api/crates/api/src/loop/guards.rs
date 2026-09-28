#![forbid(unsafe_code)]

//! Loop auto-pm guards: toggle-only body guard, admin write validation,
//! slug-taken predicates, and permission wiring (D-03).
//!
//! Ports (all under `apps/api/pi_dash/`):
//!
//! * `loop/views.py:52-61` ([`read_enabled`]) — exactly one boolean
//!   `enabled` key, else 400 `{"error":"invalid_payload"}`; a non-dict
//!   body counts as `{}`.
//! * `loop/admin_views.py:27-40` (`_SLUG_RE`, `_VALID_ROLES`, `_WRITABLE`),
//!   `:62-69` ([`hourly_floor_ok`]), `:72-105` ([`validate_writes`]).
//! * `loop/admin_views.py:119-120,159-162` ([`slug_taken_on_create`],
//!   [`slug_taken_on_patch`]) — 409 `{"error":"slug_taken"}` on create and
//!   on rename-to-other; self-rename is excluded. The
//!   `filter(slug, deleted_at IS NULL).exists()` queries belong to the
//!   handlers layer; these predicates pin the decision shape.
//! * `license/api/permissions/instance.py:12-18` (wiring reference only —
//!   the F-06 kernel in `pidash-auth` is read-only):
//!   [`require_instance_admin`] applies the kernel decision for the three
//!   admin endpoints (`LoopJobListCreateEndpoint`, `LoopJobDetailEndpoint`,
//!   `LoopJobTargetsEndpoint` at `admin_views.py:109,128,177`). Anonymous
//!   callers are denied by the kernel (`False`); the 401-vs-403 mapping
//!   belongs to the handlers layer.
//! * User surface (`views.py:64-99`): preferences are the requesting user's
//!   own, so there is no workspace-role gate — proven by
//!   `test_guest_can_still_toggle`. Nothing to enforce here; recorded so a
//!   future gate would be a conscious diff.
//!
//! Error bodies are the exact `{"error": …}` bytes DRF renders
//! (`COMPACT_JSON`, insertion order `error` before `detail`); this crate
//! enables `serde_json/preserve_order`, so the [`json!`] bodies below keep
//! that order on the wire.
//!
//! Ported bugs and quirks (translate, don't redesign):
//!
//! * BUG-LOOP-1 (`admin_views.py:82`): `int()` on a non-numeric `min_role`
//!   raises uncaught (`ValueError` for bad strings, `TypeError` for
//!   null/array/object), which DRF turns into a generic 500.
//!   [`ValidateFail::ServerError`] carries that path; handlers answer the
//!   generic 500 body.
//! * Unknown and read-only keys (`is_builtin`, `id`, …) are silently
//!   dropped, never rejected (`:75-78`).
//! * `enabled` / `dtstart` / `tzid` pass through unvalidated
//!   (`{"enabled": "x"}` validates clean on PATCH).
//! * [`hourly_floor_ok`] matches the `FREQ=` prefix case-sensitively, so
//!   `freq=minutely` passes the floor (the full validator still rejects it).
//! * `_SLUG_RE` anchors with `$`, which also matches before one trailing
//!   newline; [`slug_valid`] keeps that shape.
//! * Numeric-string `min_role` (`"15"`) passes and stays a string in
//!   `cleaned` (no coercion).

use axum::http::StatusCode;
use serde_json::{json, Map, Value};

use crate::permissions::DefaultPermissionDenied;

pub use pidash_auth::permissions::instance::{decide_instance_admin, INSTANCE_ADMIN_MIN_ROLE};

// ---------------------------------------------------------------------------
// error bodies (exact bytes; `error` before `detail` in insertion order)
// ---------------------------------------------------------------------------

/// 400 toggle-guard body (`views.py:56-60`).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"invalid_payload"}"#;
/// 400 bad-slug body (`admin_views.py:80-81`).
pub const INVALID_SLUG_BODY: &str = r#"{"error":"invalid_slug"}"#;
/// 400 bad-role body (`admin_views.py:82-83`).
pub const INVALID_MIN_ROLE_BODY: &str = r#"{"error":"invalid_min_role"}"#;
/// 400 sub-hourly body (`admin_views.py:95-96`).
pub const RRULE_TOO_FREQUENT_BODY: &str = r#"{"error":"rrule_too_frequent"}"#;
/// 409 slug-clash body (`admin_views.py:119-120,159-162`).
pub const SLUG_TAKEN_BODY: &str = r#"{"error":"slug_taken"}"#;
/// 404 unknown-job body (user job PATCH, admin detail/targets).
pub const LOOP_NOT_FOUND_BODY: &str = r#"{"error":"not_found"}"#;

/// 400 RRULE body with the validator detail (`admin_views.py:84-94`).
pub fn invalid_rrule_body(detail: &str) -> Value {
    json!({"error": "invalid_rrule", "detail": detail})
}

/// 400 create body with the sorted missing-field list (`:98-104`).
pub fn missing_fields_body(missing_sorted: &[String]) -> Value {
    json!({"error": "missing_fields", "detail": missing_sorted})
}

// ---------------------------------------------------------------------------
// rejection
// ---------------------------------------------------------------------------

/// A guard denial: HTTP status plus the exact JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardReject {
    pub status: StatusCode,
    pub body: Value,
}

impl GuardReject {
    fn new(status: StatusCode, body: Value) -> Self {
        Self { status, body }
    }

    fn bad_request(body: Value) -> Self {
        Self::new(StatusCode::BAD_REQUEST, body)
    }
}

impl From<GuardReject> for (StatusCode, Value) {
    fn from(reject: GuardReject) -> Self {
        (reject.status, reject.body)
    }
}

/// The uncaught-`int()` path (BUG-LOOP-1): in Django this escapes to DRF's
/// generic 500. Handlers answer the generic 500 body for this variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidateFail {
    Reject(GuardReject),
    ServerError,
}

impl From<GuardReject> for ValidateFail {
    fn from(reject: GuardReject) -> Self {
        ValidateFail::Reject(reject)
    }
}

// ---------------------------------------------------------------------------
// _read_enabled (views.py:52-61)
// ---------------------------------------------------------------------------

/// Extract the single boolean `enabled` from a toggle body.
///
/// Exactly one key, `enabled`, holding a JSON boolean — anything else
/// (extra keys, missing key, non-boolean, non-object body, which counts as
/// `{}`) is 400 `{"error":"invalid_payload"}`.
pub fn read_enabled(body: &Value) -> Result<bool, GuardReject> {
    let invalid = || {
        GuardReject::bad_request(serde_json::from_str(INVALID_PAYLOAD_BODY).expect("const parses"))
    };
    let Value::Object(map) = body else {
        return Err(invalid());
    };
    if map.len() != 1 {
        return Err(invalid());
    }
    match map.get("enabled") {
        Some(Value::Bool(enabled)) => Ok(*enabled),
        _ => Err(invalid()),
    }
}

// ---------------------------------------------------------------------------
// _hourly_floor_ok (admin_views.py:62-69)
// ---------------------------------------------------------------------------

/// Reject sub-hourly cadences. The `FREQ=` prefix match is case-sensitive
/// (so `freq=minutely` passes here); the value is uppercased. The last
/// `FREQ=` part wins — Python overwrites `freq` on every match.
pub fn hourly_floor_ok(rrule: &str) -> bool {
    let mut freq = "";
    for part in rrule.split(';') {
        if let Some(value) = part.strip_prefix("FREQ=") {
            freq = value;
        }
    }
    !matches!(freq.to_uppercase().as_str(), "SECONDLY" | "MINUTELY")
}

// ---------------------------------------------------------------------------
// Python-str helpers (`str()` / `int()` shapes the validators rely on)
// ---------------------------------------------------------------------------

/// Python `str()` of a JSON scalar, the shape `_SLUG_RE.match(str(...))`
/// and `str(cleaned["rrule"])` see. Containers render as JSON here rather
/// than Python `repr`: they fail both checks in either rendering, so only
/// an adversarial container's `invalid_rrule` detail text could differ —
/// no recorded vector covers that.
fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

/// `loop/admin_views.py:27`: `_SLUG_RE = ^[a-z0-9-]{1,64}$`, applied to
/// `str(slug)`. `$` also matches before one trailing newline, so a single
/// trailing `\n` is stripped first to keep that quirk.
pub fn slug_valid(slug: &str) -> bool {
    let s = slug.strip_suffix('\n').unwrap_or(slug);
    let bytes = s.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Outcome of the `int(cleaned["min_role"])` coercion (`:82`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MinRoleCoerce {
    /// An integer that fits `i64`.
    Value(i64),
    /// An integer too large for `i64` — never in `_VALID_ROLES`, so it is
    /// `invalid_min_role`, not a 500 (Python ints are unbounded).
    BigInt,
    /// `ValueError` (bad string) or `TypeError` (null/array/object):
    /// uncaught in Django, i.e. BUG-LOOP-1's generic 500.
    Invalid,
}

/// Python `int()` for strings: surrounding whitespace stripped, one
/// leading sign, single underscores allowed between digits (`"1_5"`).
fn parse_py_int_str(s: &str) -> MinRoleCoerce {
    let t = s.trim();
    let t = t.strip_prefix('+').unwrap_or(t);
    let (negative, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    if t.is_empty() {
        return MinRoleCoerce::Invalid;
    }
    let mut digit_count = 0usize;
    for (i, part) in t.split('_').enumerate() {
        if part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) {
            return MinRoleCoerce::Invalid;
        }
        if i > 0 && digit_count == 0 {
            return MinRoleCoerce::Invalid;
        }
        digit_count += part.len();
    }
    if digit_count == 0 {
        return MinRoleCoerce::Invalid;
    }
    let mut digits: String = t.chars().filter(|c| *c != '_').collect();
    if negative {
        digits.insert(0, '-');
    }
    match digits.parse::<i128>() {
        Ok(n) => i64::try_from(n)
            .map(MinRoleCoerce::Value)
            .unwrap_or(MinRoleCoerce::BigInt),
        Err(_) => MinRoleCoerce::Invalid,
    }
}

/// `int(cleaned["min_role"])` over a JSON value: integers pass through,
/// floats truncate toward zero like Python, booleans are 1/0
/// (`isinstance(True, int)`), numeric strings parse, and everything else
/// is the uncaught BUG-LOOP-1 path.
fn coerce_min_role(value: &Value) -> MinRoleCoerce {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                MinRoleCoerce::Value(i)
            } else if let Some(u) = n.as_u64() {
                i64::try_from(u)
                    .map(MinRoleCoerce::Value)
                    .unwrap_or(MinRoleCoerce::BigInt)
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() {
                    MinRoleCoerce::Value(f.trunc() as i64)
                } else {
                    MinRoleCoerce::Invalid
                }
            } else {
                MinRoleCoerce::Invalid
            }
        }
        Value::String(s) => parse_py_int_str(s),
        Value::Bool(true) => MinRoleCoerce::Value(1),
        Value::Bool(false) => MinRoleCoerce::Value(0),
        _ => MinRoleCoerce::Invalid,
    }
}

// ---------------------------------------------------------------------------
// RRULE validation (bgtasks/_rrule.py:218-263, via validate_rrule_string)
// ---------------------------------------------------------------------------
//
// Faithful port of the validation section of
// `pidash-jobs`' `tasks_ticker/rrule.rs` (itself a hand port of dateutil's
// `_parse_rfc` / `_parse_rfc_rrule`, verified against live dateutil
// 2.9.0): the input is uppercased whole, split on whitespace runs, then
// parsed as one bare/`RRULE:`-prefixed rule or as a multi-property set.
// Every rejection message matches byte for byte, including the
// `invalid RRULE: …` / `unknown parameter …` / `unsupported …` texts, the
// `FREQ={freq} is not allowed (allowed: …)` allowlist rendering, and the
// missing-`freq` constructor message. The empty string is valid
// (single-shot); `_validate_writes` rejects it earlier with
// `rrule is required`, exactly as Python does.
//
// The api crate cannot reuse the jobs implementation directly (worker-plane
// crate with broker deps; the crate graph keeps `api` off it), so the pure
// text parser lives here. If the kernel ever moves to a shared crate, this
// module should delegate to it (follow-up, not a stub: every vector below
// is pinned by tests).

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

/// A parsed single RRULE value: the FREQ (if any) and INTERVAL (if any).
/// Every other property only affects validity, like in `dateutil`.
#[derive(Debug, Default)]
struct ParsedRule {
    freq: Option<String>,
    interval: Option<i64>,
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
    if chrono::DateTime::parse_from_rfc3339(value).is_ok() {
        return true;
    }
    const FORMATS: [&str; 7] = [
        "%Y%m%dT%H%M%SZ",
        "%Y%m%dT%H%M%S",
        "%Y%m%d",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
    ];
    FORMATS.iter().any(|fmt| {
        chrono::NaiveDateTime::parse_from_str(value, fmt).is_ok()
            || chrono::NaiveDate::parse_from_str(value, fmt)
                .map(|_| ())
                .is_ok()
    })
}

/// Check one `BYDAY`/`BYWEEKDAY` value the way `dateutil`'s
/// `_handle_BYWEEKDAY` does: comma-separated items of either `+1MO` /
/// `1MO` / `MO` form or `TH(+1)` paren form.
fn check_byday(value: &str) -> bool {
    value.split(',').all(|wday| {
        if wday.contains('(') {
            // `TH(+1)` form: `splt = wday.split('(')`; `w = splt[0]`;
            // `n = int(splt[1][:-1])` (last char dropped unconditionally).
            let mut parts = wday.split('(');
            let w = parts.next().unwrap_or("");
            let rest = match parts.next() {
                Some(r) => r,
                None => return false,
            };
            let n_str: String = {
                let mut chars: Vec<char> = rest.chars().collect();
                chars.pop();
                chars.into_iter().collect()
            };
            if parse_rrule_int(&n_str).is_none() {
                return false;
            }
            WEEKDAYS.contains(&w)
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
            let ok = prop_value
                .split(',')
                .map(parse_rrule_int)
                .collect::<Option<Vec<_>>>()
                .is_some();
            if !ok {
                return Err(format!("invalid '{name}': {prop_value}"));
            }
        } else if name == "UNTIL" {
            if !parse_loose_datetime(prop_value) {
                return Err(format!("invalid 'UNTIL': {prop_value}"));
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
/// Returns the `invalid_rrule` detail fragment on failure.
fn check_freq_interval(parsed: &ParsedRule) -> Result<(), String> {
    // Without FREQ the `rrule(dtstart, **kwargs)` constructor itself fails.
    // The qualifier is `rrule.__init__` (confirmed against live dateutil:
    // `TypeError: rrule.__init__() missing 1 required positional argument:
    // 'freq'`); the message below keeps it byte for byte.
    let freq = parsed.freq.as_deref().ok_or_else(|| {
        "invalid RRULE: rrule.__init__() missing 1 required positional argument: 'freq'".to_owned()
    })?;
    if !ALLOWED_FREQS.contains(&freq) {
        return Err(format!(
            "FREQ={freq} is not allowed (allowed: {ALLOWED_FREQS_SORTED_REPR})"
        ));
    }
    let interval = match parsed.interval.unwrap_or(1) {
        0 => 1,
        n => n,
    };
    if interval < 1 {
        return Err(format!("INTERVAL must be >= 1, got {interval}"));
    }
    Ok(())
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

/// `validate_rrule_string`: our constraints on an RRULE string before
/// persisting it. Returns the `invalid_rrule` detail text on violation.
/// The empty string is valid (single-shot); callers that require a rule
/// reject it first with `rrule is required`.
pub fn validate_rrule_string(rrule_str: &str) -> Result<(), String> {
    if rrule_str.is_empty() {
        return Ok(()); // empty = single-shot, fine.
    }
    // `dateutil` uppercases the whole input before parsing.
    let upper = rrule_str.to_uppercase();
    if upper.trim().is_empty() {
        return Err("invalid RRULE: empty string".to_owned());
    }
    // Without `unfold`, `dateutil` splits lines on any whitespace run.
    let lines: Vec<&str> = upper.split_whitespace().collect();
    let invalid = |msg: String| format!("invalid RRULE: {msg}");

    if lines.len() == 1 && (!lines[0].contains(':') || lines[0].starts_with("RRULE:")) {
        let parsed = parse_single_rule(lines[0]).map_err(invalid)?;
        return check_freq_interval(&parsed);
    }

    // Multi-property mode. `dateutil` assembles a set (which never carries
    // `_freq`, so the verdict is "missing FREQ") when there are several
    // `RRULE` values or any `RDATE`/`EXDATE`/`EXRULE` lines — a lone
    // `DTSTART` line does *not* force a set. Each member parses first, so a
    // malformed member fails as invalid before assembly.
    let mut rrule_count = 0usize;
    let mut forces_set = false;
    let mut only_rule: Option<ParsedRule> = None;
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
            }
            _ => {
                return Err(invalid(format!("unsupported property: {prop}")));
            }
        }
    }
    if rrule_count == 1 && !forces_set {
        if let Some(parsed) = &only_rule {
            return check_freq_interval(parsed);
        }
    }
    // `rrule_count == 0` (e.g. a lone `DTSTART:` line) hits `rrulevals[0]`
    // in `dateutil`, raising a bare `IndexError` that is *not* an
    // `RRuleValidationError`. A typed `Result` cannot propagate that, so it
    // reports the same verdict as its sibling shapes (lone `RDATE:` line).
    Err("RRULE is missing FREQ".to_owned())
}

// ---------------------------------------------------------------------------
// _validate_writes (admin_views.py:72-105)
// ---------------------------------------------------------------------------

/// Writable job keys (`_WRITABLE`, `:29-40`). Unknown and read-only keys
/// (`is_builtin`, `id`, …) are silently dropped, never rejected (`:75-78`).
pub const WRITABLE_KEYS: &[&str] = &[
    "slug",
    "name",
    "public_name",
    "public_description",
    "prompt",
    "min_role",
    "enabled",
    "dtstart",
    "rrule",
    "tzid",
];

/// Keys a create must carry (`:99`); anything absent is reported sorted.
const REQUIRED_KEYS: &[&str] = &["slug", "name", "public_name", "prompt", "rrule"];

/// Roles `_VALID_ROLES = {5, 15, 20}` (`:28`).
const VALID_ROLES: &[i64] = &[5, 15, 20];

/// Validate and filter a job write payload.
///
/// `partial = false` is the admin POST (create): the five required keys
/// must survive filtering or the sorted `missing_fields` body answers.
/// `partial = true` is the admin PATCH: anything (including `{}`) passes
/// the required check. A non-object body counts as `{}` in both modes.
/// The returned map keeps the request's key order over the writable subset.
///
/// Check order is slug → role → rrule → required, so the first failure
/// wins exactly as in Python.
pub fn validate_writes(body: &Value, partial: bool) -> Result<Map<String, Value>, ValidateFail> {
    let mut cleaned = Map::new();
    if let Value::Object(obj) = body {
        for (key, value) in obj {
            if WRITABLE_KEYS.contains(&key.as_str()) {
                cleaned.insert(key.clone(), value.clone());
            }
        }
    }

    if let Some(slug) = cleaned.get("slug") {
        if !slug_valid(&py_str(slug)) {
            return Err(GuardReject::bad_request(
                serde_json::from_str(INVALID_SLUG_BODY).expect("const parses"),
            )
            .into());
        }
    }
    if let Some(role) = cleaned.get("min_role") {
        match coerce_min_role(role) {
            MinRoleCoerce::Value(v) if VALID_ROLES.contains(&v) => {}
            MinRoleCoerce::Value(_) | MinRoleCoerce::BigInt => {
                return Err(GuardReject::bad_request(
                    serde_json::from_str(INVALID_MIN_ROLE_BODY).expect("const parses"),
                )
                .into());
            }
            // BUG-LOOP-1: uncaught `int()` failure → generic 500.
            MinRoleCoerce::Invalid => return Err(ValidateFail::ServerError),
        }
    }
    if let Some(rrule) = cleaned.get("rrule") {
        let rrule = py_str(rrule);
        if rrule.is_empty() {
            return Err(GuardReject::bad_request(invalid_rrule_body("rrule is required")).into());
        }
        if let Err(detail) = validate_rrule_string(&rrule) {
            return Err(GuardReject::bad_request(invalid_rrule_body(&detail)).into());
        }
        if !hourly_floor_ok(&rrule) {
            return Err(GuardReject::bad_request(
                serde_json::from_str(RRULE_TOO_FREQUENT_BODY).expect("const parses"),
            )
            .into());
        }
    }

    if !partial {
        let mut missing: Vec<String> = REQUIRED_KEYS
            .iter()
            .filter(|key| !cleaned.contains_key(**key))
            .map(|key| (*key).to_owned())
            .collect();
        if !missing.is_empty() {
            missing.sort();
            return Err(GuardReject::bad_request(missing_fields_body(&missing)).into());
        }
    }
    Ok(cleaned)
}

// ---------------------------------------------------------------------------
// slug_taken (admin_views.py:119-120,159-162)
// ---------------------------------------------------------------------------

/// Create path: 409 when an active row already carries the slug.
pub fn slug_taken_on_create(active_slug_exists: bool) -> bool {
    active_slug_exists
}

/// PATCH path: 409 only when the cleaned slug differs from the row's own
/// slug (self-rename is excluded) and another active row carries it.
pub fn slug_taken_on_patch(
    cleaned_slug: Option<&str>,
    current_slug: &str,
    other_active_slug_exists: bool,
) -> bool {
    matches!(cleaned_slug, Some(slug) if slug != current_slug) && other_active_slug_exists
}

// ---------------------------------------------------------------------------
// InstanceAdminPermission wiring (permissions/instance.py:12-18)
// ---------------------------------------------------------------------------

/// Gate the three admin endpoints (`admin_views.py:109,128,177`) on the
/// F-06 kernel: anonymous denies, otherwise the instance's first row must
/// have an `InstanceAdmin` row for the user with `role >= 15`
/// ([`INSTANCE_ADMIN_MIN_ROLE`]). Denial answers DRF's default 403
/// ([`DefaultPermissionDenied`]); the anonymous-401-vs-403 mapping and the
/// row fetching stay in the handlers layer.
pub fn require_instance_admin(
    authenticated: bool,
    has_instance: bool,
    has_admin_row: bool,
) -> Result<(), DefaultPermissionDenied> {
    if decide_instance_admin(authenticated, has_instance, has_admin_row) {
        Ok(())
    } else {
        Err(DefaultPermissionDenied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/../../fixtures/loop/guards/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    /// Recursive sorted-keys canonicalizer, shared with the sibling
    /// replay suites: the goldens are stored in canonical form, and this
    /// also keeps the suite hermetic under workspace feature unification
    /// (`preserve_order` is on for this crate, so the byte-order pins
    /// below additionally lock the wire order).
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries: Vec<(String, Value)> =
                    map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Value::Object(entries.into_iter().collect())
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }

    fn assert_body(produced: &Value, expected: &Value) {
        assert_eq!(
            produced, expected,
            "field-for-field mismatch against golden body"
        );
        assert_eq!(
            serde_json::to_string(&canonical(produced)).expect("serializes"),
            serde_json::to_string(&canonical(expected)).expect("serializes"),
            "byte-identical replay mismatch (canonical sorted-keys form)"
        );
    }

    fn assert_reject(reject: &GuardReject, status: u16, expected_body: &Value) {
        assert_eq!(reject.status.as_u16(), status, "status mismatch");
        assert_body(&reject.body, expected_body);
    }

    #[test]
    fn read_enabled_replays_golden() {
        // Fixture guards/read_enabled.golden.json:
        // `loop/views.py:52-61`, 11 vectors.
        let golden = fixture("read_enabled.golden.json");
        let vectors = golden
            .get("vectors")
            .and_then(Value::as_array)
            .expect("vectors array");
        assert_eq!(vectors.len(), 11, "fixture vector count");
        for vector in vectors {
            let input = vector.get("input").expect("input");
            let output = vector.get("output").expect("output");
            if output.get("ok") == Some(&Value::Bool(true)) {
                let expected = output.get("enabled").expect("enabled");
                assert_eq!(
                    &read_enabled(input).expect("toggle accepts"),
                    expected,
                    "input {input}"
                );
            } else {
                let reject = read_enabled(input).expect_err("toggle rejects");
                assert_reject(
                    &reject,
                    output
                        .get("status")
                        .and_then(Value::as_u64)
                        .expect("status") as u16,
                    output.get("body").expect("body"),
                );
            }
        }
    }

    fn assert_validate_ok(
        produced: &Result<Map<String, Value>, ValidateFail>,
        output: &Value,
        input: &Value,
    ) {
        let cleaned = produced.as_ref().expect("write validates").clone();
        if let Some(expected) = output.get("cleaned") {
            assert_body(&Value::Object(cleaned), expected);
        } else {
            // The numeric-string `min_role` vector carries no `cleaned`
            // snapshot: it pins that the value passes uncoerced instead.
            assert_eq!(
                cleaned.get("min_role"),
                Some(&Value::String("15".to_owned())),
                "numeric-string min_role stays a string, input {input}"
            );
        }
    }

    fn replay_validate_vectors(vectors: &[Value], partial: bool) {
        for vector in vectors {
            let input = vector.get("input").expect("input");
            let output = vector.get("output").expect("output");
            let produced = validate_writes(input, partial);
            if output.get("raises").is_some() {
                // BUG-LOOP-1: uncaught `int()` → generic 500.
                assert_eq!(
                    produced,
                    Err(ValidateFail::ServerError),
                    "non-numeric min_role is the 500 path, input {input}"
                );
            } else if output.get("ok") == Some(&Value::Bool(true)) {
                assert_validate_ok(&produced, output, input);
            } else {
                let ValidateFail::Reject(reject) = produced.expect_err("write rejects") else {
                    panic!("expected a 4xx rejection, input {input}");
                };
                assert_reject(
                    &reject,
                    output
                        .get("status")
                        .and_then(Value::as_u64)
                        .expect("status") as u16,
                    output.get("body").expect("body"),
                );
            }
        }
    }

    #[test]
    fn validate_writes_create_replays_golden() {
        // Fixture guards/validate_writes.golden.json `create_vectors`:
        // `loop/admin_views.py:27-105`, 14 vectors.
        let golden = fixture("validate_writes.golden.json");
        let vectors = golden
            .get("create_vectors")
            .and_then(Value::as_array)
            .expect("create_vectors array");
        assert_eq!(vectors.len(), 14, "fixture vector count");
        replay_validate_vectors(vectors, false);
    }

    #[test]
    fn validate_writes_partial_replays_golden() {
        // Same fixture, `partial_vectors`: PATCH accepts anything
        // writable-shaped, including `{}` and `{"enabled": "x"}`.
        let golden = fixture("validate_writes.golden.json");
        let vectors = golden
            .get("partial_vectors")
            .and_then(Value::as_array)
            .expect("partial_vectors array");
        assert_eq!(vectors.len(), 5, "fixture vector count");
        replay_validate_vectors(vectors, true);
    }

    #[test]
    fn validate_writes_nondict_body_counts_as_empty() {
        // `request.data if isinstance(request.data, dict) else {}`:
        // create then reports all five required fields, sorted.
        let Err(ValidateFail::Reject(reject)) = validate_writes(&json!([1]), false) else {
            panic!("array body must reject on create");
        };
        assert_eq!(reject.status, StatusCode::BAD_REQUEST);
        assert_body(
            &reject.body,
            &json!({"error": "missing_fields",
                    "detail": ["name", "prompt", "public_name", "rrule", "slug"]}),
        );
        assert!(validate_writes(&json!("x"), true)
            .expect("patch")
            .is_empty());
    }

    #[test]
    fn hourly_floor_replays_golden() {
        // Fixture guards/hourly_floor.golden.json:
        // `loop/admin_views.py:62-69`, 7 vectors.
        let golden = fixture("hourly_floor.golden.json");
        let vectors = golden
            .get("vectors")
            .and_then(Value::as_array)
            .expect("vectors array");
        assert_eq!(vectors.len(), 7, "fixture vector count");
        for vector in vectors {
            let rrule = vector.get("rrule").and_then(Value::as_str).expect("rrule");
            assert_eq!(
                hourly_floor_ok(rrule),
                vector.get("ok") == Some(&Value::Bool(true)),
                "rrule {rrule}"
            );
        }
    }

    #[test]
    fn slug_taken_predicates_match_admin_views() {
        // Fixture guards/slug_taken.golden.json records the DB
        // observations (`:119-120` create, `:159-162` patch); the
        // predicates pin the decision shape, incl. the self-rename exclusion.
        assert!(slug_taken_on_create(true));
        assert!(!slug_taken_on_create(false));
        assert!(!slug_taken_on_patch(Some("same"), "same", true));
        assert!(slug_taken_on_patch(Some("other"), "current", true));
        assert!(!slug_taken_on_patch(Some("other"), "current", false));
        assert!(!slug_taken_on_patch(None, "current", true));
    }

    #[test]
    fn instance_admin_wiring_matches_golden() {
        // Fixture guards/instance_admin_permission.golden.json:
        // member without an InstanceAdmin row is denied, role=20 passes,
        // anonymous denies before any DB read.
        assert!(require_instance_admin(true, true, false).is_err());
        assert!(require_instance_admin(true, true, true).is_ok());
        assert!(require_instance_admin(false, true, true).is_err());
        assert!(require_instance_admin(true, false, true).is_err());
    }

    #[tokio::test]
    async fn instance_admin_denial_is_the_drf_default_403() {
        // The member vector pins
        // `{"detail": "You do not have permission to perform this action."}`
        // at 403 — the DRF-default denial, since
        // `InstanceAdminPermission` sets no `message`.
        let golden = fixture("instance_admin_permission.golden.json");
        let member_body = golden
            .get("vectors")
            .and_then(Value::as_array)
            .expect("vectors array")[0]
            .get("output")
            .expect("output")
            .get("body")
            .expect("body")
            .clone();
        let response = require_instance_admin(true, true, false)
            .expect_err("member is denied")
            .into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("read body");
        let body: Value = serde_json::from_slice(&bytes).expect("denial is JSON");
        assert_body(&body, &member_body);
    }

    #[test]
    fn error_bodies_are_byte_exact_in_wire_order() {
        // `serde_json/preserve_order` keeps `error` before `detail`; these
        // pins fail if a future edit rebuilds a body key-sorted.
        assert_eq!(
            serde_json::to_string(&read_enabled(&json!({})).expect_err("rejects").body)
                .expect("serializes"),
            INVALID_PAYLOAD_BODY
        );
        assert_eq!(
            serde_json::to_string(&invalid_rrule_body("rrule is required")).expect("serializes"),
            r#"{"error":"invalid_rrule","detail":"rrule is required"}"#
        );
        let ValidateFail::Reject(slug_reject) =
            validate_writes(&json!({"slug": "Bad Slug"}), true).expect_err("rejects")
        else {
            panic!("bad slug must be a 400 rejection");
        };
        assert_eq!(slug_reject.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            serde_json::to_string(&slug_reject.body).expect("serializes"),
            INVALID_SLUG_BODY
        );
        assert_eq!(
            serde_json::to_string(&missing_fields_body(&[
                "name".to_owned(),
                "prompt".to_owned(),
                "public_name".to_owned(),
                "rrule".to_owned(),
                "slug".to_owned()
            ]))
            .expect("serializes"),
            r#"{"error":"missing_fields","detail":["name","prompt","public_name","rrule","slug"]}"#
        );
        for constant in [
            INVALID_SLUG_BODY,
            INVALID_MIN_ROLE_BODY,
            RRULE_TOO_FREQUENT_BODY,
            SLUG_TAKEN_BODY,
            LOOP_NOT_FOUND_BODY,
        ] {
            let parsed: Value = serde_json::from_str(constant).expect("const parses");
            assert_eq!(
                serde_json::to_string(&parsed).expect("serializes"),
                constant,
                "const round-trips byte-exact"
            );
        }
    }
}
