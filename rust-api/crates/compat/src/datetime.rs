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
//!   and one folded `if`);
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
