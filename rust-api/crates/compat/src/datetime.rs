//! Datetime/UUID rendering and parsing (OVL-04 part 4, PIDASHCONV-1542).
//!
//! Canonical home of the DRF `iso-8601` render core (lifted verbatim
//! from `api::serializer`, which now delegates to it), the small
//! option/date/uuid render shapes every module re-implemented, and
//! the one shared ISO-8601 parser. Only byte-identical (or
//! semantically proven identical) copies move here:
//!
//! - moved: `render_datetime_in` + `render_datetime` +
//!   `render_naive_datetime_in` (the core; the 6 plain `render_dt`
//!   forwarders stay as thin wrappers over it),
//!   `render_datetime_opt` (5 defs), `render_date` (2),
//!   `render_date_opt` (3), `render_isoformat` (5, incl.
//!   `twin_render_isoformat`), `render_rfc3339_in` (3),
//!   `django_str_datetime` (2), `render_uuid` (7, incl. the 3
//!   `Value`-form `uuid_string`), `render_uuid_opt` (13),
//!   `uuid_string` (2, `String` form), and the 7-function
//!   `parse_iso_datetime` parser (2 defs each; the copies differ
//!   only in comments, one dead `head.len() < text.len()` check,
//!   and one folded `if`), and `parse_django_datetime` +
//!   `parse_fromisoformat` + `parse_django_datetime_re` +
//!   `ParsedDatetime` (the Django `parse_datetime` + DRF `strptime`
//!   chain, moved from `pidash_api::v1_cycles_modules::cycle` by
//!   OVL-14, PIDASHCONV-1649; the cycle module re-exports it);
//! - kept local: the `space/intake.rs` `render_dt` (`AutoSi`
//!   trimming diverges from the core for trailing-zero micros —
//!   suspected ported bug, out of scope), the `quoted` and
//!   year-checking `render_dt` variants, single-copy shapes
//!   (`render_drf_datetime`, `render_dt_utc`, the `Option<String>`
//!   `render_datetime_opt`, `uuid_opt_string`, `render_opt_uuid`),
//!   the `"None"`-rendering `opt_uuid` pair (Python-repr family,
//!   part 5), the differently-typed `parse_iso_offset` x3,
//!   `parse_python_uuid` x2 and `uuid_value` x2, the other four
//!   `parse_iso_datetime` (different return types), the
//!   row/`Map`/`Value`/`FieldValue` `opt_uuid` lookups, and the
//!   SQL-literal / `str()`-format zone renderers (different jobs).

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;
use uuid::Uuid;

/// Render an aware datetime exactly like DRF `DateTimeField` with the
/// default `iso-8601` format: ISO-8601 with `+00:00` rewritten to `Z`.
pub fn render_datetime<Tz: TimeZone>(dt: &DateTime<Tz>) -> String {
    let utc = dt.with_timezone(&chrono::Utc);
    render_utc_parts(
        &utc.format("%Y-%m-%dT%H:%M:%S").to_string(),
        utc.timestamp_subsec_nanos(),
        "Z",
    )
}

/// Render an aware datetime in a caller-chosen zone (the request's zone).
/// Non-UTC offsets are kept verbatim; a `+00:00` offset is rewritten to `Z`,
/// exactly like DRF does.
pub fn render_datetime_in<TzFrom: TimeZone, TzTo: TimeZone>(
    dt: &DateTime<TzFrom>,
    tz: &TzTo,
) -> String
where
    TzTo::Offset: std::fmt::Display,
{
    let local = dt.with_timezone(tz);
    let suffix = local.format("%:z").to_string();
    let suffix = if suffix == "+00:00" {
        "Z".to_owned()
    } else {
        suffix
    };
    render_utc_parts(
        &local.format("%Y-%m-%dT%H:%M:%S").to_string(),
        local.timestamp_subsec_nanos(),
        &suffix,
    )
}

/// Render a naive datetime through the request's zone. Under the project's
/// settings (`USE_TZ = True`) DRF makes naive values aware in the field's
/// timezone before rendering, so a naive value renders exactly like the same
/// wall time in `tz` (including the `Z` rewrite for `+00:00`).
pub fn render_naive_datetime_in<TzTo: TimeZone>(dt: &NaiveDateTime, tz: &TzTo) -> String
where
    TzTo::Offset: std::fmt::Display,
{
    use chrono::MappedLocalTime;
    let local = match tz.from_local_datetime(dt) {
        MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) => local,
        // A wall time inside a DST gap has no local offset; fall back to the
        // same wall time read as UTC (Python's zoneinfo attach never raises).
        MappedLocalTime::None => return render_datetime_in(&dt.and_utc(), tz),
    };
    let suffix = local.format("%:z").to_string();
    let suffix = if suffix == "+00:00" {
        "Z".to_owned()
    } else {
        suffix
    };
    render_utc_parts(
        &dt.format("%Y-%m-%dT%H:%M:%S").to_string(),
        dt.and_utc().timestamp_subsec_nanos(),
        &suffix,
    )
}

fn render_utc_parts(base: &str, nanos: u32, suffix: &str) -> String {
    // DRF `DateTimeField` renders Python datetimes, which carry only
    // microseconds: no fraction when micros == 0, exactly 6 digits
    // otherwise. Truncate sub-microsecond nanos (e.g. `Utc::now()`
    // stamps) so HTTP output never carries 9 digits (PIDASHCONV-867).
    let micros = nanos / 1000;
    if micros == 0 {
        format!("{base}{suffix}")
    } else {
        format!("{base}.{:06}{suffix}", micros)
    }
}

/// Render an optional datetime in the request's zone: `null` when absent.
pub fn render_datetime_opt<TzTo: TimeZone>(value: &Option<DateTime<Utc>>, timezone: &TzTo) -> Value
where
    TzTo::Offset: std::fmt::Display,
{
    value.map_or(Value::Null, |dt| {
        Value::String(render_datetime_in(&dt, timezone))
    })
}

/// DRF `DateField` wire format.
pub fn render_date(value: &NaiveDate) -> String {
    value.format("%Y-%m-%d").to_string()
}

/// Render an optional date: `null` when absent.
pub fn render_date_opt(value: &Option<NaiveDate>) -> Value {
    value.map_or(Value::Null, |date| {
        Value::String(date.format("%Y-%m-%d").to_string())
    })
}

/// Python `datetime.isoformat()` for a UTC instant: `+00:00` suffix, six
/// fraction digits when microseconds are nonzero, none otherwise.
/// (`AutoSi` would trim trailing zeros; Python always prints six.)
pub fn render_isoformat(dt: DateTime<Utc>) -> String {
    if dt.timestamp_subsec_micros() == 0 {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
    } else {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
    }
}

/// The parse half of [`render_rfc3339_in`] failed. Callers map this to
/// their own 500 error (`HandlerError::ServerError`, `Denial::ServerError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatetimeError;

/// Render a `row_to_json` timestamptz string (RFC 3339) through the
/// request's zone. Unparseable input is a 500, exactly like the
/// per-module copies this replaces.
pub fn render_rfc3339_in<TzTo: TimeZone>(raw: &str, tz: &TzTo) -> Result<String, DatetimeError>
where
    TzTo::Offset: std::fmt::Display,
{
    let dt = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| DatetimeError)?;
    Ok(render_datetime_in(&dt, tz))
}

/// `str()` of an aware UTC datetime: `YYYY-MM-DD HH:MM:SS[.ffffff]+HH:MM`
/// (`USE_TZ=True`, so UTC) — the fractional part appears only when
/// microseconds are nonzero, exactly like `datetime.isoformat(sep=" ")`.
pub fn django_str_datetime(moment: &DateTime<Utc>) -> String {
    if moment.timestamp_subsec_nanos() == 0 {
        moment.format("%Y-%m-%d %H:%M:%S%:z").to_string()
    } else {
        moment.format("%Y-%m-%d %H:%M:%S%.6f%:z").to_string()
    }
}

/// Render a UUID as a JSON string.
pub fn render_uuid(value: &Uuid) -> Value {
    Value::String(value.to_string())
}

/// Render an optional UUID: `null` when absent.
pub fn render_uuid_opt(value: &Option<Uuid>) -> Value {
    value.map_or(Value::Null, |id| Value::String(id.to_string()))
}

/// Render a UUID as a plain string.
pub fn uuid_string(id: &Uuid) -> String {
    id.to_string()
}

/// Parse an all-digit run as `u32` (the `fromisoformat` surface rejects
/// anything else, including empty and multibyte input).
pub fn parse_digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn iso_weekday(day: u32) -> Option<chrono::Weekday> {
    chrono::Weekday::try_from(u8::try_from(day).ok()?.checked_sub(1)?).ok()
}

pub fn parse_iso_date_part(text: &str) -> Option<NaiveDate> {
    if text.len() >= 4 && text.as_bytes().get(4) == Some(&b'W') {
        // `YYYYWww[D]` basic week. Length + ASCII first: short inputs
        // (`"2024W"`) or multibyte bytes would panic the slices below,
        // while `fromisoformat` just rejects them.
        if !(text.len() == 7 || text.len() == 8) || !text.is_ascii() {
            return None;
        }
        let year = parse_digits(&text[0..4])? as i32;
        let week = parse_digits(&text[5..7])?;
        let weekday = if text.len() == 8 {
            parse_digits(&text[7..8])?
        } else {
            1
        };
        return NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday)?);
    }
    if text.contains('W') {
        // `YYYY-Www[-D]` extended week: exactly four digits, a dash, `W`
        // (short years like `24W05` reject).
        let mut parts = text.split('W');
        let year_text = parts.next()?;
        let year_digits = year_text.strip_suffix('-')?;
        if year_digits.len() != 4 {
            return None;
        }
        let year = parse_digits(year_digits)? as i32;
        let rest = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        // ASCII first: a multibyte `rest` of byte-length 3 would panic
        // the `[0..2]` slice below (`fromisoformat` rejects it instead).
        if !rest.is_ascii() {
            return None;
        }
        let (week_text, weekday) = match rest.split_once('-') {
            Some((week, day)) => (week, parse_digits(day)?),
            None if rest.len() == 2 => (rest, 1),
            None if rest.len() == 3 => (&rest[0..2], parse_digits(&rest[2..3])?),
            None => return None,
        };
        if week_text.len() != 2 {
            return None;
        }
        let week = parse_digits(week_text)?;
        return NaiveDate::from_isoywd_opt(year, week, iso_weekday(weekday)?);
    }
    if text.len() == 8 && text.bytes().all(|b| b.is_ascii_digit()) {
        // `YYYYMMDD` basic.
        return NaiveDate::from_ymd_opt(
            text[0..4].parse().ok()?,
            text[4..6].parse().ok()?,
            text[6..8].parse().ok()?,
        );
    }
    if text.len() == 10 {
        // `YYYY-MM-DD` extended (strictly zero-padded here; the 1-2-digit
        // shapes fall through to the `date_re`/`datetime_re` fallbacks).
        let bytes = text.as_bytes();
        if bytes[4] == b'-' && bytes[7] == b'-' {
            return NaiveDate::from_ymd_opt(
                parse_digits(&text[0..4])? as i32,
                parse_digits(&text[5..7])?,
                parse_digits(&text[8..10])?,
            );
        }
    }
    None
}

pub fn split_tz_suffix(text: &str) -> (&str, Option<i64>) {
    if let Some(head) = text.strip_suffix('Z') {
        if head.len() < text.len() && !head.is_empty() {
            return (head, Some(0));
        }
    }
    // Find the last `+`/`-` that starts the offset (not the date's dashes:
    // the offset sign sits after the time separator / at least past index
    // 10... simpler: scan from the end for the last sign preceded by a
    // digit or space, with only offset chars after it).
    let bytes = text.as_bytes();
    let mut sign_at = None;
    for (index, byte) in bytes.iter().enumerate().rev() {
        if *byte == b'+' || *byte == b'-' {
            let tail = &text[index + 1..];
            if !tail.is_empty()
                && tail
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b':' || b == b'.')
            {
                sign_at = Some(index);
                break;
            }
        }
        if *byte == b'T' || *byte == b't' || *byte == b' ' || *byte == b'W' {
            // A time/date separator left of any sign: the offset must
            // come after the time, so there is no tz suffix. Stop.
            break;
        }
    }
    let Some(at) = sign_at else {
        return (text, None);
    };
    // The sign must follow the time part (there is a time separator left
    // of it and at least `HH:MM`/`HHMM` after the separator).
    let head = text[..at].trim_end();
    let tail = text[at + 1..].trim_start();
    if tail.is_empty() {
        return (text, None);
    }
    let sign: i64 = if bytes[at] == b'-' { -1 } else { 1 };
    let digits: String = tail.chars().filter(|ch| ch.is_ascii_digit()).collect();
    // Colon offsets are strict: exactly-two-digit hours/minutes, and
    // seconds of exactly two digits plus an optional `.`/`,` fraction
    // (truncated — `+00:00:00.5` is `+00:00`).
    let offset = if tail.contains(':') {
        let mut parts = tail.split(':');
        let (Some(hours_text), Some(minutes_text)) = (parts.next(), parts.next()) else {
            return (text, None);
        };
        let seconds_text = parts.next();
        if parts.next().is_some() || hours_text.len() != 2 || minutes_text.len() != 2 {
            return (text, None);
        }
        let (Some(hours), Some(minutes)) = (parse_digits(hours_text), parse_digits(minutes_text))
        else {
            return (text, None);
        };
        let seconds: i64 = match seconds_text {
            None => 0,
            Some(sec_text) => {
                let (int, frac) = match sec_text.split_once(['.', ',']) {
                    None => (sec_text, None),
                    Some((int, frac)) => (int, Some(frac)),
                };
                if int.len() != 2 {
                    return (text, None);
                }
                let Some(seconds) = parse_digits(int) else {
                    return (text, None);
                };
                if frac.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
                    return (text, None);
                }
                seconds as i64
            }
        };
        hours as i64 * 3600 + minutes as i64 * 60 + seconds
    } else if digits.len() == tail.len() && (tail.len() == 2 || tail.len() == 4) {
        let hours: i64 = tail[0..2].parse().unwrap_or(-1);
        let minutes: i64 = if tail.len() == 4 {
            tail[2..4].parse().unwrap_or(-1)
        } else {
            0
        };
        if hours < 0 || minutes < 0 {
            return (text, None);
        }
        hours * 3600 + minutes * 60
    } else {
        return (text, None);
    };
    // `+99:99` parses numerically here; the range gate below rejects it
    // (offsets beyond ±23:59 raise in `fromisoformat`, surfacing as
    // `invalid` through DRF's suppress).
    if offset.abs() >= 24 * 3600 {
        return (head, Some(i64::MAX));
    }
    (head, Some(sign * offset))
}

/// Split `head` into date/separator/time: candidate date lengths
/// `YYYY-MM-DD` (10), `YYYYMMDD`/`YYYYWwwD` (8), `YYYY-Www` (7), `YYYYWww` (6);
/// the separator is any single (possibly multibyte) non-digit char.
pub fn split_date_time(head: &str) -> Option<(&str, char, &str)> {
    // Candidate date lengths: `YYYY-MM-DD` (10), `YYYYMMDD` (8),
    // `YYYY-Www-D` (10), `YYYY-Www` (7), `YYYYWwwD` (8), `YYYYWww` (6).
    for len in [10usize, 8, 7, 6] {
        if head.len() <= len {
            continue;
        }
        // Checked split: a multibyte char straddling the cut is never a
        // valid date lead here — skip the length (shorter ones may still
        // match with a multibyte separator, which `fromisoformat`
        // accepts), it must not panic.
        let Some((date_text, rest)) = head.split_at_checked(len) else {
            continue;
        };
        if parse_iso_date_part(date_text).is_none() {
            continue;
        }
        let mut chars = rest.chars();
        let sep = chars.next()?;
        if sep.is_ascii_digit() {
            continue;
        }
        let time_text = &rest[sep.len_utf8()..];
        if time_text.is_empty() {
            continue;
        }
        return Some((date_text, sep, time_text));
    }
    None
}

pub fn parse_iso_time(text: &str) -> Option<chrono::NaiveTime> {
    use chrono::NaiveTime;
    // Split off the fraction (`.` or `,` + 1+ digits; a bare trailing
    // separator is rejected).
    let (clock, micros) = match text.find(['.', ',']) {
        Some(dot) => {
            let (clock, frac) = text.split_at(dot);
            let digits = &frac[1..];
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let padded = format!("{digits:0<6}");
            let micros: u32 = padded[..6].parse().ok()?;
            (clock, micros)
        }
        None => (text, 0),
    };
    if clock.contains(':') {
        let mut parts = clock.split(':');
        let hour: u32 = parse_digits(parts.next()?)?;
        let minute: u32 = parts.next().map(parse_digits).unwrap_or(Some(0))?;
        let second: u32 = parts.next().map(parse_digits).unwrap_or(Some(0))?;
        if parts.next().is_some() {
            return None;
        }
        // `fromisoformat` requires zero-padded 2-digit fields here — the
        // 1-digit shapes fall through to the `datetime_re` fallback.
        for part in clock.split(':') {
            if part.len() != 2 {
                return None;
            }
        }
        NaiveTime::from_hms_micro_opt(hour, minute, second, micros)
    } else {
        // Basic `HH[MM[SS]]`: exactly 2/4/6 digits. ASCII first: a
        // multibyte clock of byte-length 4/6 would panic the slices
        // (`fromisoformat` rejects it instead).
        if !clock.is_ascii() {
            return None;
        }
        match clock.len() {
            2 => NaiveTime::from_hms_micro_opt(parse_digits(clock)?, 0, 0, micros),
            4 => NaiveTime::from_hms_micro_opt(
                parse_digits(&clock[0..2])?,
                parse_digits(&clock[2..4])?,
                0,
                micros,
            ),
            6 => NaiveTime::from_hms_micro_opt(
                parse_digits(&clock[0..2])?,
                parse_digits(&clock[2..4])?,
                parse_digits(&clock[4..6])?,
                micros,
            ),
            _ => None,
        }
    }
}

/// Django `parse_datetime` + DRF `enforce_timezone` in one: parses the
/// `fromisoformat` surface, returning the naive wall time plus the UTC
/// offset in seconds when the input carries one.
pub fn parse_iso_datetime(text: &str) -> Option<(NaiveDateTime, Option<i64>)> {
    let (head, offset) = split_tz_suffix(text);
    if offset == Some(i64::MAX) {
        return None;
    }
    // Date-only (midnight) — but a trailing bare separator (`'...T'`) is
    // rejected, and week/basic shapes route through the date parser.
    if let Some(date) = parse_iso_date_part(head) {
        return Some((date.and_hms_opt(0, 0, 0)?, offset));
    }
    // Split date | separator | time at the first non-date char.
    let (date_text, sep, time_text) = split_date_time(head)?;
    let _ = sep;
    let date = parse_iso_date_part(date_text)?;
    let naive_time = parse_iso_time(time_text)?;
    Some((NaiveDateTime::new(date, naive_time), offset))
}

// ---------------------------------------------------------------------------
// Django `parse_datetime` + DRF `strptime` fallthrough: moved verbatim from
// `pidash_api::v1_cycles_modules::cycle` by OVL-14 (PIDASHCONV-1649) — the
// v1-write operations in `pidash-services` need it and cannot depend on
// `pidash-api`. The cycle module re-exports it, so its own paths and
// tests keep running this copy. Named `parse_django_datetime` (not
// `parse_iso_datetime`) because this module already owns that name for
// a different parser; the cycle re-export keeps the old name.
// ---------------------------------------------------------------------------

/// One parsed datetime input: the wall time plus an optional fixed offset
/// in microseconds east of UTC (`None` = naive → the request user's zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedDatetime {
    pub naive: chrono::NaiveDateTime,
    pub offset_micros: Option<i64>,
}

/// `django.utils.dateparse.parse_datetime` on the oracle (Django 4.2,
/// Python 3.12, `dateparse.py:104-129`, probed): `fromisoformat` first,
/// the `datetime_re` fallback second, then DRF's
/// `strptime(value, 'iso-8601')` fallthrough (`to_internal_value` runs
/// it when `parse_datetime` returns `None`). Both `None` and
/// `ValueError` mean "invalid" to DRF (suppressed at `fields.py:1179`),
/// so one `Option`.
pub fn parse_django_datetime(text: &str) -> Option<ParsedDatetime> {
    parse_fromisoformat(text)
        .or_else(|| parse_django_datetime_re(text))
        .or_else(|| {
            // `strptime` compiles the literal format with `re.IGNORECASE`,
            // so the input `iso-8601` in any letter case yields naive
            // 1900-01-01. Exact match: padding fails on both sides (probed).
            // Python's fold also accepts İ/ı/ſ letter variants; the port is
            // ASCII-only (same family as the 765 unicode gap).
            text.eq_ignore_ascii_case("iso-8601").then(|| {
                let naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .expect("1900-01-01 valid");
                ParsedDatetime {
                    naive,
                    offset_micros: None,
                }
            })
        })
}

/// Python 3.12 `datetime.fromisoformat` (probed): extended/basic calendar
/// and ISO week dates, any single char separator (digits included:
/// `2026-10-01110:30:00` parses), `HH[MM[SS]]` times with `.`/`,`
/// fractions of any length (truncated to micros), `Z` or numeric offsets
/// (hours, hours+minutes, hours+minutes+seconds, each with an optional
/// fraction on the last unit). Years 1-9999, offsets strictly inside ±24h.
pub fn parse_fromisoformat(text: &str) -> Option<ParsedDatetime> {
    if text.is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    // Date prefix: `YYYY-MM-DD` | `YYYYMMDD` | `YYYY-Www[-D]` | `YYYYWww[D]`.
    // Only a `W` at 4/5 opens a week date; anywhere else it is an
    // ordinary char (`2026-10-01W10:30:00` parses with `W` as separator).
    if matches!(text.find('W'), Some(4) | Some(5)) {
        return parse_fromiso_week_datetime(text);
    }
    let (date, rest) = parse_fromiso_date(bytes)?;
    parse_fromiso_rest(date, rest, false).map(|(parsed, _)| parsed)
}

/// Everything after a `fromisoformat` date: bare (midnight naive) or one
/// separator char plus time plus the tz tail, plus the basic time's digit
/// count (`None` for extended times and bare dates). With `shape_only`
/// the components are grammar-checked but range checks (hour ≤ 23,
/// offset inside ±24h, ...) are skipped, so a well-formed but
/// out-of-range rest still parses; the basic week fallback uses that to
/// tell range failures (hard, unless the digit count is odd) from grammar
/// failures (fall back).
fn parse_fromiso_rest(
    date: chrono::NaiveDate,
    rest: &str,
    shape_only: bool,
) -> Option<(ParsedDatetime, Option<usize>)> {
    if rest.is_empty() {
        return Some((
            ParsedDatetime {
                naive: date.and_hms_opt(0, 0, 0)?,
                offset_micros: None,
            },
            None,
        ));
    }
    // Exactly one separator char, always (`20261001110:30:00` and
    // `2026-10-01110:30:00` both parse, probed).
    let sep_len = rest.chars().next()?.len_utf8();
    let (naive_time, rest, frac_len, basic_digits) =
        parse_fromiso_time(&rest[sep_len..], shape_only)?;
    let naive = date.and_time(naive_time);
    let offset_micros = parse_fromiso_tz_tail(rest, frac_len, shape_only)?;
    Some((
        ParsedDatetime {
            naive,
            offset_micros,
        },
        basic_digits,
    ))
}

/// The tz remainder after a `fromisoformat` time (probed): empty (naive),
/// `Z`, or a numeric offset — or skipped chars followed by `Z` or a
/// numeric offset. The allowance depends on the fraction that was just
/// consumed (`frac_len` digits, dotted or dotless, on any unit): none →
/// one skip (`T10:30:00X+05:30` parses; even a bare `.` skips:
/// `T10:30:00.+00:00` parses); 1-5 digits → no skip (`T10:30:00.5X+05:30`
/// fails); 6+ digits → unlimited skips (`T10:30:00.000000ABCDEFG+05:30`
/// parses — there is no second fraction, the middle is skipped). A
/// marker-first remainder never skips, so `Z+00:00` fails, and a `Z`
/// reached by skipping poisons the parse (`T10:30:00.000000XYZ+05:30`
/// fails: after `XY` comes `Z+05:30`); a skip with no tz after it fails
/// (`T1033404` fails); there is no skip inside the offset itself
/// (`+05X+06:00` fails). Up to `allowance` trailing NULs are tolerated
/// (`T10:30:00\0` parses naive, `T10:30:00\0\0` and `T10:30:00.5\0` fail,
/// `T10:30:00.000000\0\0` parses naive).
fn parse_fromiso_tz_tail(rest: &str, frac_len: usize, shape_only: bool) -> Option<Option<i64>> {
    let allowance = tz_skip_allowance(frac_len);
    let mut tail = rest;
    let mut stripped: u32 = 0;
    while allowance.is_none_or(|max| stripped < max) {
        match tail.strip_suffix('\0') {
            Some(shorter) => {
                tail = shorter;
                stripped += 1;
            }
            None => break,
        }
    }
    match tail {
        "" => Some(None),
        "Z" => Some(Some(0)),
        _ => {
            let mut rest = tail;
            let mut skips: u32 = 0;
            loop {
                if rest == "Z" {
                    return Some(Some(0));
                }
                let first = rest.as_bytes().first()?;
                if *first == b'+' || *first == b'-' {
                    return Some(Some(parse_fromiso_offset(rest, shape_only)?));
                }
                if *first == b'Z' || allowance.is_some_and(|max| skips >= max) {
                    return None;
                }
                let skip = rest.chars().next()?.len_utf8();
                rest = &rest[skip..];
                skips += 1;
                if rest.is_empty() {
                    return None;
                }
            }
        }
    }
}

/// Skip allowance from the consumed fraction's digit count (probed):
/// `None` means unlimited.
fn tz_skip_allowance(frac_len: usize) -> Option<u32> {
    if frac_len == 0 {
        Some(1)
    } else if frac_len < 6 {
        Some(0)
    } else {
        None
    }
}

/// The `fromisoformat` calendar date prefix plus the unparsed remainder
/// (week dates, which contain `W`, are handled before this is reached).
fn parse_fromiso_date(bytes: &[u8]) -> Option<(chrono::NaiveDate, &str)> {
    let text = std::str::from_utf8(bytes).ok()?;
    if bytes.len() >= 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        let year = digits_to_i32(&bytes[0..4])?;
        if !(1..=9999).contains(&year) {
            return None;
        }
        let month = digits_to_u32(&bytes[5..7])?;
        let day = digits_to_u32(&bytes[8..10])?;
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, &text[10..]));
    }
    if bytes.len() >= 8 && bytes[0..8].iter().all(|b| b.is_ascii_digit()) {
        let year = digits_to_i32(&bytes[0..4])?;
        if !(1..=9999).contains(&year) {
            return None;
        }
        let month = digits_to_u32(&bytes[4..6])?;
        let day = digits_to_u32(&bytes[6..8])?;
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, &text[8..]));
    }
    None
}

/// A `fromisoformat` ISO week datetime. The weekday takes the date's
/// own shape only — `-D` after `YYYY-Www`, a trailing `D` after `YYYYWww`
/// — and it backtracks: the with-weekday parse is tried first, then the
/// bare-week (Monday) parse (`2026W404110:30:00` is Thursday, `2026W40610:30`
/// is Monday, `2026-W40-11:30:00` is Monday, all probed). The basic `D`
/// wins outright only with an even basic digit count (or an extended
/// time). On an odd count Monday wins when it parses (`2026W404103340Z`
/// is Monday), a Monday grammar failure keeps a valid with-day parse
/// (`2026W404Z103+05.5` is Thursday), and a Monday range failure fails
/// hard. A with-day grammar failure falls back to Monday-or-None, and a
/// with-day range failure does the same on odd counts but fails hard on
/// even counts (`2026W401030:00` fails even though Monday 03:00 would
/// parse). The extended `-D` additionally needs
/// end-or-nondigit after D (`2026-W40-4110:30:00` fails the with-day
/// attempt and falls back to Monday, probed). With no dash the bare week
/// takes any separator, digits included (`2026-W40610:30:00` is Monday,
/// probed).
fn parse_fromiso_week_datetime(text: &str) -> Option<ParsedDatetime> {
    let bytes = text.as_bytes();
    let wpos = text.find('W')?;
    if wpos != 4 && wpos != 5 {
        return None;
    }
    let extended = wpos == 5;
    if extended && bytes[4] != b'-' {
        return None;
    }
    let year = digits_to_i32(&bytes[0..4])?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    let rest = &text[wpos + 1..];
    if rest.len() < 2 || !rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let week: u32 = rest[0..2].parse().ok()?;
    let tail = &rest[2..];
    let monday = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)?;
    if extended {
        match tail.strip_prefix('-') {
            Some(day) => {
                // Try `-D` (D must be followed by end or a non-digit),
                // fall back to Monday with the dash as separator.
                let day_end = day.len() <= 1 || !day.as_bytes()[1].is_ascii_digit();
                if day_end {
                    if let Some((parsed, _)) = weekday_try_backtrack(year, week, day, false) {
                        return Some(parsed);
                    }
                }
                parse_fromiso_rest(monday, tail, false).map(|(parsed, _)| parsed)
            }
            None => parse_fromiso_rest(monday, tail, false).map(|(parsed, _)| parsed),
        }
    } else {
        // Try `D`: it wins outright only with an even basic digit count
        // (or an extended time). On an odd count Monday wins when it
        // parses, a Monday grammar failure keeps a valid with-day parse,
        // and a Monday range failure fails hard. A with-day grammar
        // failure falls back to Monday-or-None, and a with-day range
        // failure does the same on odd counts but fails hard on even
        // counts — so check the shape when the full parse fails, and the
        // parity in both cases.
        let with_full = weekday_try_backtrack(year, week, tail, false);
        let with_odd = match &with_full {
            Some((_, basic_digits)) => basic_digits.is_some_and(|count| count % 2 == 1),
            None => match weekday_try_backtrack(year, week, tail, true) {
                None => {
                    return parse_fromiso_rest(monday, tail, false).map(|(parsed, _)| parsed);
                }
                Some((_, basic_digits)) => basic_digits.is_some_and(|count| count % 2 == 1),
            },
        };
        if !with_odd {
            return with_full.map(|(parsed, _)| parsed);
        }
        let without_full = parse_fromiso_rest(monday, tail, false);
        if without_full.is_some() {
            return without_full.map(|(parsed, _)| parsed);
        }
        if parse_fromiso_rest(monday, tail, true).is_none() {
            return with_full.map(|(parsed, _)| parsed);
        }
        None
    }
}

/// The with-weekday attempt: a leading digit 1-7 names the day, and the
/// whole rest (separator, time, tz) must parse after it.
fn weekday_try_backtrack(
    year: i32,
    week: u32,
    text: &str,
    shape_only: bool,
) -> Option<(ParsedDatetime, Option<usize>)> {
    let digit = *text.as_bytes().first()?;
    if !(b'1'..=b'7').contains(&digit) {
        return None;
    }
    let weekday = chrono::Weekday::try_from(digit - b'0' - 1).ok()?;
    let date = chrono::NaiveDate::from_isoywd_opt(year, week, weekday)?;
    parse_fromiso_rest(date, &text[1..], shape_only)
}

/// The `fromisoformat` time plus the tz remainder plus the consumed
/// fraction's digit count (which drives the tz skip allowance):
/// `HH[[:]MM[[:]SS]][.,frac]` (a fraction after any unit means sub-second
/// micros, probed: `T10:30.5` is 10:30:00.5), plus a basic-only dotless
/// fraction: 2+ digits glued after SS (`T103340040` is .040s; a single
/// glued digit ends the time instead, probed: `T1033404` fails). Mixed
/// basic/extended forms never parse (`T10:3000`, `T1030:00` fail), and a
/// colon is never backtracked (`T10:30:0+00:00` fails). With
/// `shape_only` the units are grammar-checked (two digits each) but the
/// clock ranges are not enforced (midnight stands in for the time). The
/// digit count covers basic unit digits plus the dotless run (including a
/// lone run-1 digit that stays in the tz tail) plus one lone tail digit
/// after a short basic time; dotted fractions never count, and extended
/// times report `None`.
fn parse_fromiso_time(
    text: &str,
    shape_only: bool,
) -> Option<(chrono::NaiveTime, &str, usize, Option<usize>)> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || !bytes[0..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hour: u32 = text[0..2].parse().ok()?;
    let mut rest = &text[2..];
    let mut minute: u32 = 0;
    let mut second: u32 = 0;
    let mut digits: Option<usize> = Some(2);
    // Optional `:MM` / `MM`.
    if let Some(tail) = rest.strip_prefix(':') {
        digits = None;
        if tail.len() < 2 || !tail.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        minute = tail[0..2].parse().ok()?;
        rest = &tail[2..];
        // Optional `:SS`.
        if let Some(tail) = rest.strip_prefix(':') {
            if tail.len() < 2 || !tail.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            second = tail[0..2].parse().ok()?;
            rest = &tail[2..];
        }
    } else if rest.len() >= 2 && rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
        digits = Some(4);
        minute = rest[0..2].parse().ok()?;
        rest = &rest[2..];
        if rest.len() >= 2 && rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
            digits = Some(6);
            second = rest[0..2].parse().ok()?;
            rest = &rest[2..];
            // Basic dotless fraction: a run of 2+ digits.
            let run: usize = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
            if run >= 2 {
                let mut micros: i64 = 0;
                for (index, byte) in rest.bytes().take(6).enumerate() {
                    if !byte.is_ascii_digit() {
                        break;
                    }
                    micros += (byte - b'0') as i64 * 10_i64.pow(5 - index as u32);
                }
                let time = time_or_shape_midnight(hour, minute, second, micros, shape_only)?;
                return Some((time, &rest[run..], run, Some(6 + run)));
            }
        }
    }
    // A lone digit left in the tz tail still counts (a run-1 after SS or
    // after a short basic time).
    if digits.is_some() && rest.as_bytes().first().is_some_and(|b| b.is_ascii_digit()) {
        digits = digits.map(|count| count + 1);
    }
    let (micros, frac_len, rest) = parse_frac_micros_opt(rest)?;
    let time = time_or_shape_midnight(hour, minute, second, micros, shape_only)?;
    Some((time, rest, frac_len, digits))
}

/// Build the clock time, or midnight when only the shape is wanted.
fn time_or_shape_midnight(
    hour: u32,
    minute: u32,
    second: u32,
    micros: i64,
    shape_only: bool,
) -> Option<chrono::NaiveTime> {
    if shape_only {
        return chrono::NaiveTime::from_hms_opt(0, 0, 0);
    }
    chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micros as u32)
}

/// The optional dotted fraction after a time component: `.`/`,` plus ≥1
/// digit (any length, truncated to micros), returning micros, digit count
/// and remainder. No dot — or a dot with no digits — means `(0, 0, rest)`
/// unconsumed: a bare dot ends the time and the tz tail skips it
/// (`T10:30:00.+00:00` parses, probed).
fn parse_frac_micros_opt(text: &str) -> Option<(i64, usize, &str)> {
    let tail = match text.strip_prefix('.').or_else(|| text.strip_prefix(',')) {
        Some(tail) => tail,
        None => return Some((0, 0, text)),
    };
    let digits: usize = tail.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return Some((0, 0, text));
    }
    let mut micros: i64 = 0;
    for (index, byte) in tail.bytes().take(6).enumerate() {
        if !byte.is_ascii_digit() {
            break;
        }
        micros += (byte - b'0') as i64 * 10_i64.pow(5 - index as u32);
    }
    Some((micros, digits, &tail[digits..]))
}

/// The `fromisoformat` numeric offset in micros east of UTC:
/// `±HH[MM[SS]]` basic or `±HH:MM[:SS]` extended (the mode locks on the
/// first separator: `+0530:15` and `+05:3015` fail, probed), with an
/// optional `.`/`,` fraction after any unit (always sub-second micros,
/// probed: `+05.5` is 5h + 0.5s) — except a zero `HH:MM:SS` drops the
/// fraction entirely (`+00:00:00.5` is UTC, probed). Components are never
/// range-checked (`+05:99` normalises to +6:39); only the total must sit
/// strictly inside ±24h (skipped with `shape_only`).
fn parse_fromiso_offset(text: &str, shape_only: bool) -> Option<i64> {
    let bytes = text.as_bytes();
    let sign: i64 = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &text[1..];
    if rest.len() < 2 || !rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = rest[0..2].parse().ok()?;
    let mut rest = &rest[2..];
    if rest.starts_with('.') || rest.starts_with(',') {
        let (frac, _, tail) = parse_frac_micros_opt(rest)?;
        return finish_fromiso_offset(sign, hours, 0, 0, frac, tail, shape_only);
    }
    let mut minutes: i64 = 0;
    let mut seconds: i64 = 0;
    if rest.starts_with(':') {
        // Extended: `HH:MM[:SS]`.
        rest = &rest[1..];
        if rest.len() < 2 || !rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        minutes = rest[0..2].parse().ok()?;
        rest = &rest[2..];
        if rest.starts_with('.') || rest.starts_with(',') {
            let (frac, _, tail) = parse_frac_micros_opt(rest)?;
            return finish_fromiso_offset(sign, hours, minutes, 0, frac, tail, shape_only);
        }
        if let Some(tail) = rest.strip_prefix(':') {
            if tail.len() < 2 || !tail.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            seconds = tail[0..2].parse().ok()?;
            rest = &tail[2..];
        }
    } else {
        // Basic: `HH[MM[SS]]`.
        if rest.len() >= 2 && rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
            minutes = rest[0..2].parse().ok()?;
            rest = &rest[2..];
            if rest.starts_with('.') || rest.starts_with(',') {
                let (frac, _, tail) = parse_frac_micros_opt(rest)?;
                return finish_fromiso_offset(sign, hours, minutes, 0, frac, tail, shape_only);
            }
            if rest.len() >= 2 && rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
                seconds = rest[0..2].parse().ok()?;
                rest = &rest[2..];
            }
        }
    }
    let (frac, _, tail) = parse_frac_micros_opt(rest)?;
    finish_fromiso_offset(sign, hours, minutes, seconds, frac, tail, shape_only)
}

/// Total a `fromisoformat` offset: the sign applies to the whole-unit part
/// and the fraction alike; a zero whole part drops the fraction; the total
/// must sit strictly inside ±24h; nothing may remain.
fn finish_fromiso_offset(
    sign: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    frac_micros: i64,
    rest: &str,
    shape_only: bool,
) -> Option<i64> {
    if !rest.is_empty() {
        return None;
    }
    if hours == 0 && minutes == 0 && seconds == 0 {
        return Some(0);
    }
    let micros = sign * (hours * 3_600_000_000 + minutes * 60_000_000 + seconds * 1_000_000)
        + sign * frac_micros;
    if !shape_only && micros.abs() >= 24 * 3_600_000_000 {
        return None;
    }
    Some(micros)
}

/// Django's `datetime_re` fallback (`dateparse.py`, exact):
/// padded-or-not `YYYY-M-D`, `T`/space, `H:M[:S[.,frac]]]`, optional
/// whitespace, optional `Z|±HH[[:]MM]`. The frac is `\d{1,6}\d{0,6}`
/// (micros come from the first 6, left-justified). Offsets are whole minutes
/// strictly inside ±24h (`timezone()` raises past that).
pub fn parse_django_datetime_re(text: &str) -> Option<ParsedDatetime> {
    let bytes = text.as_bytes();
    if bytes.len() < 5 {
        return None;
    }
    if !bytes[0..4].iter().all(|b| b.is_ascii_digit()) || bytes[4] != b'-' {
        return None;
    }
    let year = digits_to_i32(&bytes[0..4])?;
    // Django builds `datetime(...)` from the groups, which raises for year
    // 0 (and DRF turns the raise into invalid, probed).
    if !(1..=9999).contains(&year) {
        return None;
    }
    let mut rest = &text[5..];
    let (month, tail) = take_1_or_2_digits(rest)?;
    rest = tail.strip_prefix('-')?;
    let (day, tail) = take_1_or_2_digits(rest)?;
    rest = tail;
    let sep = rest.as_bytes().first()?;
    if *sep != b'T' && *sep != b' ' {
        return None;
    }
    rest = &rest[1..];
    let (hour, tail) = take_1_or_2_digits(rest)?;
    rest = tail.strip_prefix(':')?;
    let (minute, tail) = take_1_or_2_digits(rest)?;
    rest = tail;
    let mut second: u32 = 0;
    let mut micros: i64 = 0;
    if let Some(tail) = rest.strip_prefix(':') {
        let (value, tail) = take_1_or_2_digits(tail)?;
        second = value;
        rest = tail;
        if let Some(tail) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(',')) {
            let digits: usize = tail.bytes().take_while(|b| b.is_ascii_digit()).count();
            if digits == 0 || digits > 12 {
                return None;
            }
            let (frac, _, tail) = parse_frac_micros_opt(rest)?;
            micros = frac;
            rest = tail;
        }
    }
    // `\s*` then the optional tzinfo, then end.
    let trimmed = rest.trim_start_matches(|c: char| c.is_whitespace());
    let offset_micros = match trimmed {
        "" => None,
        "Z" => Some(0),
        _ => Some(parse_django_offset(trimmed)?),
    };
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let time = chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micros as u32)?;
    let naive = date.and_time(time);
    Some(ParsedDatetime {
        naive,
        offset_micros,
    })
}

/// Django's tzinfo arm: `±HH[[:]MM]` whole minutes, strictly inside ±24h.
fn parse_django_offset(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let sign: i64 = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &text[1..];
    if rest.len() < 2 || !rest.as_bytes()[0..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = rest[0..2].parse().ok()?;
    let rest = &rest[2..];
    let (colon, rest) = match rest.strip_prefix(':') {
        Some(tail) => (true, tail),
        None => (false, rest),
    };
    let minutes: i64 = if rest.is_empty() {
        if colon {
            return None;
        }
        0
    } else {
        if rest.len() != 2 || !rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        rest.parse().ok()?
    };
    let micros = (hours * 3_600 + minutes * 60) * 1_000_000;
    if micros.abs() >= 24 * 3_600_000_000 {
        return None;
    }
    Some(sign * micros)
}

/// 1-2 ASCII digits plus the remainder (`\d{1,2}`).
fn take_1_or_2_digits(text: &str) -> Option<(u32, &str)> {
    let bytes = text.as_bytes();
    let first = *bytes.first()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let mut value: u32 = (first - b'0') as u32;
    let mut rest = &text[1..];
    if let Some(second) = rest.as_bytes().first() {
        if second.is_ascii_digit() {
            value = value * 10 + (second - b'0') as u32;
            rest = &rest[1..];
        }
    }
    Some((value, rest))
}

fn digits_to_i32(digits: &[u8]) -> Option<i32> {
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

fn digits_to_u32(digits: &[u8]) -> Option<u32> {
    if digits.is_empty() || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike as _;
    use chrono_tz::Tz;
    use std::str::FromStr;

    // Moved alongside the core from `api::serializer`.
    #[test]
    fn datetime_renders_drf_iso8601_with_z() {
        let dt = chrono::Utc
            .with_ymd_and_hms(2024, 1, 2, 3, 4, 5)
            .unwrap()
            .with_nanosecond(123_456_000)
            .unwrap();
        assert_eq!(render_datetime(&dt), "2024-01-02T03:04:05.123456Z");
    }

    #[test]
    fn datetime_without_micros_has_no_fraction() {
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(render_datetime(&dt), "2024-01-02T03:04:05Z");
    }

    #[test]
    fn datetime_in_user_zone_keeps_offset() {
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        let eastern: Tz = "America/New_York".parse().unwrap();
        assert_eq!(
            render_datetime_in(&dt, &eastern),
            "2024-01-01T22:04:05-05:00"
        );
    }

    #[test]
    fn datetime_fixed_offset_zone() {
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        let zone = chrono::FixedOffset::east_opt(5 * 3600 + 1800).unwrap();
        assert_eq!(render_datetime_in(&dt, &zone), "2024-01-02T08:34:05+05:30");
    }

    #[test]
    fn utc_zone_renders_z_not_offset() {
        // Oracle: DRF rewrites a trailing `+00:00` to `Z` for every zone,
        // so the default request zone (UTC) takes the `Z` path.
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(
            render_datetime_in(&dt, &chrono::Utc),
            "2024-01-02T03:04:05Z"
        );
    }

    #[test]
    fn naive_datetime_renders_through_the_request_zone() {
        // Oracle (DRF 3.15, USE_TZ=True): naive values are made aware in the
        // field timezone before rendering.
        let dt = chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(3, 4, 5)
            .unwrap();
        assert_eq!(
            render_naive_datetime_in(&dt, &chrono::Utc),
            "2024-01-02T03:04:05Z"
        );
        let eastern: Tz = "America/New_York".parse().unwrap();
        assert_eq!(
            render_naive_datetime_in(&dt, &eastern),
            "2024-01-02T03:04:05-05:00"
        );
    }

    #[test]
    fn sub_microsecond_nanos_truncate_to_micros() {
        // PIDASHCONV-867: Python datetimes carry only micros, so DRF
        // never emits 9 digits; sub-micro nanos truncate.
        let dt = chrono::Utc
            .with_ymd_and_hms(2024, 1, 2, 3, 4, 5)
            .unwrap()
            .with_nanosecond(123_456_789)
            .unwrap();
        assert_eq!(render_datetime(&dt), "2024-01-02T03:04:05.123456Z");
        // Sub-micro-only nanos (< 1µs) render with no fraction, like
        // DRF with microsecond == 0.
        let tiny = chrono::Utc
            .with_ymd_and_hms(2024, 1, 2, 3, 4, 5)
            .unwrap()
            .with_nanosecond(999)
            .unwrap();
        assert_eq!(render_datetime(&tiny), "2024-01-02T03:04:05Z");
    }

    // Moved alongside `render_isoformat` from `runner_enroll/enroll.rs`.
    #[test]
    fn isoformat_matches_django() {
        let whole = DateTime::from_timestamp(1_700_000_000, 0).expect("ts");
        assert_eq!(render_isoformat(whole), "2023-11-14T22:13:20+00:00");
        let micros = DateTime::from_timestamp(1_700_000_000, 123_456_000).expect("ts");
        assert_eq!(render_isoformat(micros), "2023-11-14T22:13:20.123456+00:00");
        // Millis-exact micros keep six digits (the PIDASHCONV-716 trap).
        let millis = DateTime::from_timestamp(1_700_000_000, 123_000_000).expect("ts");
        assert_eq!(render_isoformat(millis), "2023-11-14T22:13:20.123000+00:00");
    }

    // Moved alongside `django_str_datetime` from `tasks_mail/notifications.rs`.
    #[test]
    fn django_str_datetime_renders_like_python() {
        // `str()` of an aware UTC datetime: `+00:00`, never `UTC`.
        let moment =
            chrono::DateTime::parse_from_rfc3339("2026-09-01T12:00:00Z").expect("test time");
        assert_eq!(
            django_str_datetime(&moment.with_timezone(&chrono::Utc)),
            "2026-09-01 12:00:00+00:00"
        );
        // `str()` keeps microseconds when nonzero (`:605`); `created_at`
        // values from Postgres virtually always carry them.
        let moment =
            chrono::DateTime::parse_from_rfc3339("2026-09-01T12:00:00.123456Z").expect("test time");
        assert_eq!(
            django_str_datetime(&moment.with_timezone(&chrono::Utc)),
            "2026-09-01 12:00:00.123456+00:00"
        );
    }

    #[test]
    fn rfc3339_renders_through_zone_and_rejects_garbage() {
        let utc = chrono::Utc;
        assert_eq!(
            render_rfc3339_in("2024-01-02T03:04:05+00:00", &utc),
            Ok("2024-01-02T03:04:05Z".to_owned())
        );
        let eastern: Tz = "America/New_York".parse().unwrap();
        assert_eq!(
            render_rfc3339_in("2024-01-02T03:04:05Z", &eastern),
            Ok("2024-01-01T22:04:05-05:00".to_owned())
        );
        assert_eq!(
            render_rfc3339_in("not-a-datetime", &utc),
            Err(DatetimeError)
        );
    }

    #[test]
    fn option_renders_are_null_when_absent() {
        let utc = chrono::Utc;
        let dt = chrono::Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(
            render_datetime_opt(&Some(dt), &utc),
            Value::String("2024-01-02T03:04:05Z".to_owned())
        );
        assert_eq!(render_datetime_opt(&None, &utc), Value::Null);
        let day = chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        assert_eq!(render_date(&day), "2024-01-02");
        assert_eq!(
            render_date_opt(&Some(day)),
            Value::String("2024-01-02".to_owned())
        );
        assert_eq!(render_date_opt(&None), Value::Null);
        let id = Uuid::from_str("12345678-1234-5678-1234-567812345678").unwrap();
        assert_eq!(
            render_uuid(&id),
            Value::String("12345678-1234-5678-1234-567812345678".to_owned())
        );
        assert_eq!(
            render_uuid_opt(&Some(id)),
            Value::String("12345678-1234-5678-1234-567812345678".to_owned())
        );
        assert_eq!(render_uuid_opt(&None), Value::Null);
        assert_eq!(uuid_string(&id), "12345678-1234-5678-1234-567812345678");
    }

    #[test]
    fn iso_parser_vectors() {
        // Basic date-only means midnight, no offset. (Extended
        // `2024-01-02` does NOT parse: the `-02` reads as a tz
        // offset, verbatim ported behavior.)
        let (naive, offset) = parse_iso_datetime("20240102").expect("date parses");
        assert_eq!(
            naive,
            chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
        );
        assert_eq!(offset, None);
        // `Z` splits to a zero offset.
        let (_, offset) = parse_iso_datetime("2024-01-02T03:04:05Z").expect("z parses");
        assert_eq!(offset, Some(0));
        // Numeric offsets come back in seconds.
        let (naive, offset) =
            parse_iso_datetime("2024-01-02T03:04:05+05:30").expect("offset parses");
        assert_eq!(offset, Some(5 * 3600 + 30 * 60));
        assert_eq!(naive.format("%H:%M:%S").to_string(), "03:04:05");
        // Out-of-range offsets reject through the `i64::MAX` sentinel.
        assert_eq!(parse_iso_datetime("2024-01-02T03:04:05+99:99"), None);
        // A bare `Z` has no head to split.
        assert_eq!(split_tz_suffix("Z"), ("Z", None));
        assert_eq!(
            split_tz_suffix("2024-01-02T03:04:05Z"),
            ("2024-01-02T03:04:05", Some(0))
        );
    }

    // Moved alongside the parser from `app_issues/handlers_core.rs`.
    #[test]
    fn tz_offsets_are_strict_two_digit() {
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+05:30:05"),
            ("2024-01-01T00:00:00", Some(5 * 3600 + 30 * 60 + 5))
        );
        // One-digit fields reject; fractions truncate.
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+1:00"),
            ("2024-01-01T00:00:00+1:00", None)
        );
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+05:30:5"),
            ("2024-01-01T00:00:00+05:30:5", None)
        );
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+00:00:00.5"),
            ("2024-01-01T00:00:00", Some(0))
        );
        assert_eq!(
            split_tz_suffix("2024-01-01T00:00:00+00:00:99"),
            ("2024-01-01T00:00:00", Some(99))
        );
    }

    // Moved alongside the parser from `app_issues/handlers_core.rs`
    // (`short_and_multibyte_dates_reject_without_panicking`: the
    // `parse_drf_date` assertions stay with that function).
    #[test]
    fn split_and_time_reject_multibyte_without_panicking() {
        // Multibyte bytes straddling a cut reject without panicking.
        assert_eq!(split_date_time("2024-01-1äT00:00"), None);
        assert_eq!(parse_iso_time("1ä3"), None);
        // Valid shapes still parse, including multibyte separators
        // (`fromisoformat` accepts any single char there).
        assert!(split_date_time("20240101é00:00").is_some());
        assert!(parse_iso_time("1234").is_some());
    }
}
