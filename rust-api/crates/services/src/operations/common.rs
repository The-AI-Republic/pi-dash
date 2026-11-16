//! Helpers shared by every operation: the database-failure mapping,
//! the microsecond wall clock, and timezone activation.
//!
//! Moved from `pidash-api::v1_work_items::handlers_core` (OVL-14,
//! PIDASHCONV-1649), returning [`Error`](super::error::Error) instead of
//! the api-crate `Denial`.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::Value;

use super::error::Error;

/// Map a database failure onto the generic 500, logging the site (the
/// handlers' `db_error`).
pub(crate) fn db_error<E: std::fmt::Display>(error: E, site: &str) -> Error {
    tracing::warn!(%error, site, "v1_work_items operation database failure");
    Error::ServerError
}

/// `timezone.now()` truncated to microseconds: Python datetimes carry no
/// nanos, and `timestamptz` stores micros — an untruncated `Utc::now()`
/// would render nanos in the response while the DB row reads back micros.
pub(crate) fn now_utc() -> DateTime<Utc> {
    trunc_micros(Utc::now())
}

/// Truncate an instant to microsecond precision (see [`now_utc`]).
pub(crate) fn trunc_micros(dt: DateTime<Utc>) -> DateTime<Utc> {
    let nanos = dt.timestamp_subsec_nanos();
    dt - chrono::Duration::nanoseconds(i64::from(nanos % 1000))
}

/// Activate the actor's rendering timezone (`TimezoneMixin.initial` runs
/// after `super().initial()`). A missing zone defaults to UTC; an unknown
/// zone name 400s: `zoneinfo.ZoneInfo` raises `ZoneInfoNotFoundError`,
/// which subclasses `KeyError`, so `handle_exception` answers the
/// `KeyError` branch (`api/views/base.py:160-164`), never a 500.
pub(crate) fn activate_timezone(timezone: Option<&str>) -> Result<Tz, Error> {
    timezone
        .unwrap_or("UTC")
        .parse()
        .map_err(|_| Error::BadError("The required key does not exist.".to_owned()))
}

/// The activity `requested_data` text for a parsed write body
/// (PIDASHCONV-763): form bodies dump list fields as `QueryDict`
/// last-wins scalars, while validation keeps the `getlist` arrays. JSON
/// bodies dump as-is (arrays are real data there) without a copy.
pub fn requested_data_text(value: &Value, from_form: bool) -> String {
    if from_form {
        let mut projected = value.clone();
        if let Value::Object(map) = &mut projected {
            pidash_compat::http::project_list_scalars(map, &["labels"]);
        }
        pidash_compat::py::django_dumps(&projected)
    } else {
        pidash_compat::py::django_dumps(value)
    }
}

/// The `model_activity` `requested_data` object for a parsed write body:
/// the same scalar-last projection as [`requested_data_text`], as a value.
pub fn requested_data_value(value: &Value, from_form: bool) -> Value {
    if from_form {
        let mut projected = value.clone();
        if let Value::Object(map) = &mut projected {
            pidash_compat::http::project_list_scalars(map, &["labels"]);
        }
        projected
    } else {
        value.clone()
    }
}

/// Map a comment/relation-read failure: database failures keep their site
/// (the handlers' `db_error`); 404/mid-row failures are unreachable in
/// the operation flows (the handler 404s before the call; rows just
/// written exist).
pub(crate) fn map_read_error(error: crate::read::Error) -> Error {
    match error {
        crate::read::Error::NotFound | crate::read::Error::Failed(_) => Error::ServerError,
        crate::read::Error::Db(err) => {
            let (site, source) = err.split();
            db_error(source, site)
        }
    }
}
