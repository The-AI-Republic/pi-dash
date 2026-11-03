//! `IssueFilterSet` and the per-view half of `ComplexFilterBackend`.
//!
//! Ports `pi_dash/utils/filters/filterset.py`. The tree shape, structure
//! rules and error cases live in [`crate::filter`] (the kernel half); this
//! module is what F-04 deferred: the concrete `FilterSet` declaration, the
//! custom soft-delete-aware relation methods, and the leaf compiler.
//!
//! Declaration inventory (every name is an allowed `filters`-param key):
//!
//! - Relation methods (each an exact and an `__in` filter, both AND-ing a
//!   `<join>.deleted_at IS NULL` soft-delete guard, mirroring the
//!   `filter_<name>` / `filter_<name>_in` methods):
//!   `assignee_id` → `issue_assignee.assignee_id`,
//!   `cycle_id` → `issue_cycle.cycle_id`,
//!   `module_id` → `issue_module.module_id`,
//!   `mention_id` → `issue_mention.mention_id`,
//!   `label_id` → `label_issue.label_id`,
//!   `subscriber_id` → `issue_subscribers.subscriber_id`.
//! - Direct fields: `created_by_id`, `state_id`, `project_id` (UUID, exact +
//!   `__in`); `state_group` (char, exact + `__in`, on `state.group`);
//!   `is_archived` (boolean method filter, see [`archived_condition`]);
//!   `start_date`, `target_date`, `created_at`, `updated_at` (exact +
//!   `__range`); `is_draft` (exact); `priority` (exact + `__in`).
//! - `BaseFilterSet.get_filters` adds a `<name>__exact` alias for every
//!   filter whose lookup is `exact` (deep-copied, same behavior).
//!
//! `build_combined_q` semantics: only filters actually provided in the
//! request are combined, all with AND. None of the declared filters sets
//! `exclude`, so there is no negation branch to port.

use std::collections::HashSet;

use chrono::{TimeZone, Timelike};
use sea_query::{Alias, Condition, Expr, SimpleExpr};
use serde_json::Value;

use crate::filter::FilterError;

/// Lifecycle ordering shared by filtering (`pi_dash/utils/constants.py`).
/// Triage is excluded upstream; the tuple starts at `backlog`.
pub const STATE_GROUP_ORDER: &[&str] = &[
    "backlog",
    "unstarted",
    "started",
    "review",
    "test",
    "completed",
    "cancelled",
];

/// `ACTIVE_STATE_GROUPS = STATE_GROUP_ORDER[1:-2]`.
pub const ACTIVE_STATE_GROUPS: &[&str] = &["unstarted", "started", "review", "test"];

/// How one declared filter name compiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    /// A UUID equality / membership on one column.
    Uuid {
        table: &'static str,
        column: &'static str,
        many: bool,
    },
    /// A relation method: equality / membership on the join column plus a
    /// `<join>.deleted_at IS NULL` soft-delete guard.
    Relation {
        join_table: &'static str,
        join_column: &'static str,
        many: bool,
    },
    /// A string equality / membership on one column.
    Text {
        table: &'static str,
        column: &'static str,
        many: bool,
    },
    /// A boolean equality on one column (`NullBooleanField`: the
    /// `NullBooleanSelect` spellings below, anything else cleans to `None`
    /// and filters `IS NULL`).
    Flag {
        table: &'static str,
        column: &'static str,
    },
    /// A date equality on a `DATE` column. Accepts Django's
    /// `DATE_INPUT_FORMATS` and normalizes to ISO; datetime strings are
    /// rejected, like `DateField` does.
    Date {
        table: &'static str,
        column: &'static str,
    },
    /// A datetime equality on a `DateTimeField` column. Accepts ISO-8601,
    /// `DATETIME_INPUT_FORMATS`, and date-only strings; resolves to an
    /// instant (naive inputs in the request user's zone) and binds it as
    /// a `timestamptz`, like Python.
    Datetime {
        table: &'static str,
        column: &'static str,
    },
    /// A two-element date range on a `DATE` column.
    DateRange {
        table: &'static str,
        column: &'static str,
    },
    /// A two-element datetime range on a `DateTimeField` column.
    DatetimeRange {
        table: &'static str,
        column: &'static str,
    },
    /// [`archived_condition`].
    Archived,
}

/// One declared filter: its `filters`-param name and how it compiles.
/// `alias_of` marks the `__exact` aliases `get_filters` synthesizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterDecl {
    pub name: &'static str,
    pub kind: FilterKind,
    pub alias_of: Option<&'static str>,
}

macro_rules! decl {
    ($name:expr, $kind:expr) => {
        FilterDecl {
            name: $name,
            kind: $kind,
            alias_of: None,
        }
    };
    ($name:expr, $kind:expr, alias_of = $base:expr) => {
        FilterDecl {
            name: $name,
            kind: $kind,
            alias_of: Some($base),
        }
    };
}

/// Every filter `IssueFilterSet` declares, in source order, with the
/// `__exact` aliases `BaseFilterSet.get_filters` synthesizes for each
/// `exact`-lookup filter directly after its base.
pub const ISSUE_FILTERSET: &[FilterDecl] = &[
    decl!(
        "assignee_id",
        FilterKind::Relation {
            join_table: "issue_assignee",
            join_column: "assignee_id",
            many: false
        }
    ),
    decl!(
        "assignee_id__exact",
        FilterKind::Relation {
            join_table: "issue_assignee",
            join_column: "assignee_id",
            many: false
        },
        alias_of = "assignee_id"
    ),
    decl!(
        "assignee_id__in",
        FilterKind::Relation {
            join_table: "issue_assignee",
            join_column: "assignee_id",
            many: true
        }
    ),
    decl!(
        "cycle_id",
        FilterKind::Relation {
            join_table: "issue_cycle",
            join_column: "cycle_id",
            many: false
        }
    ),
    decl!(
        "cycle_id__exact",
        FilterKind::Relation {
            join_table: "issue_cycle",
            join_column: "cycle_id",
            many: false
        },
        alias_of = "cycle_id"
    ),
    decl!(
        "cycle_id__in",
        FilterKind::Relation {
            join_table: "issue_cycle",
            join_column: "cycle_id",
            many: true
        }
    ),
    decl!(
        "module_id",
        FilterKind::Relation {
            join_table: "issue_module",
            join_column: "module_id",
            many: false
        }
    ),
    decl!(
        "module_id__exact",
        FilterKind::Relation {
            join_table: "issue_module",
            join_column: "module_id",
            many: false
        },
        alias_of = "module_id"
    ),
    decl!(
        "module_id__in",
        FilterKind::Relation {
            join_table: "issue_module",
            join_column: "module_id",
            many: true
        }
    ),
    decl!(
        "mention_id",
        FilterKind::Relation {
            join_table: "issue_mention",
            join_column: "mention_id",
            many: false
        }
    ),
    decl!(
        "mention_id__exact",
        FilterKind::Relation {
            join_table: "issue_mention",
            join_column: "mention_id",
            many: false
        },
        alias_of = "mention_id"
    ),
    decl!(
        "mention_id__in",
        FilterKind::Relation {
            join_table: "issue_mention",
            join_column: "mention_id",
            many: true
        }
    ),
    decl!(
        "label_id",
        FilterKind::Relation {
            join_table: "label_issue",
            join_column: "label_id",
            many: false
        }
    ),
    decl!(
        "label_id__exact",
        FilterKind::Relation {
            join_table: "label_issue",
            join_column: "label_id",
            many: false
        },
        alias_of = "label_id"
    ),
    decl!(
        "label_id__in",
        FilterKind::Relation {
            join_table: "label_issue",
            join_column: "label_id",
            many: true
        }
    ),
    decl!(
        "created_by_id",
        FilterKind::Uuid {
            table: "issue",
            column: "created_by_id",
            many: false
        }
    ),
    decl!(
        "created_by_id__exact",
        FilterKind::Uuid {
            table: "issue",
            column: "created_by_id",
            many: false
        },
        alias_of = "created_by_id"
    ),
    decl!(
        "created_by_id__in",
        FilterKind::Uuid {
            table: "issue",
            column: "created_by_id",
            many: true
        }
    ),
    decl!("is_archived", FilterKind::Archived),
    decl!(
        "is_archived__exact",
        FilterKind::Archived,
        alias_of = "is_archived"
    ),
    decl!(
        "state_group",
        FilterKind::Text {
            table: "state",
            column: "group",
            many: false
        }
    ),
    decl!(
        "state_group__exact",
        FilterKind::Text {
            table: "state",
            column: "group",
            many: false
        },
        alias_of = "state_group"
    ),
    decl!(
        "state_group__in",
        FilterKind::Text {
            table: "state",
            column: "group",
            many: true
        }
    ),
    decl!(
        "state_id",
        FilterKind::Uuid {
            table: "issue",
            column: "state_id",
            many: false
        }
    ),
    decl!(
        "state_id__exact",
        FilterKind::Uuid {
            table: "issue",
            column: "state_id",
            many: false
        },
        alias_of = "state_id"
    ),
    decl!(
        "state_id__in",
        FilterKind::Uuid {
            table: "issue",
            column: "state_id",
            many: true
        }
    ),
    decl!(
        "project_id",
        FilterKind::Uuid {
            table: "issue",
            column: "project_id",
            many: false
        }
    ),
    decl!(
        "project_id__exact",
        FilterKind::Uuid {
            table: "issue",
            column: "project_id",
            many: false
        },
        alias_of = "project_id"
    ),
    decl!(
        "project_id__in",
        FilterKind::Uuid {
            table: "issue",
            column: "project_id",
            many: true
        }
    ),
    decl!(
        "subscriber_id",
        FilterKind::Relation {
            join_table: "issue_subscribers",
            join_column: "subscriber_id",
            many: false
        }
    ),
    decl!(
        "subscriber_id__exact",
        FilterKind::Relation {
            join_table: "issue_subscribers",
            join_column: "subscriber_id",
            many: false
        },
        alias_of = "subscriber_id"
    ),
    decl!(
        "subscriber_id__in",
        FilterKind::Relation {
            join_table: "issue_subscribers",
            join_column: "subscriber_id",
            many: true
        }
    ),
    decl!(
        "start_date",
        FilterKind::Date {
            table: "issue",
            column: "start_date"
        }
    ),
    decl!(
        "start_date__exact",
        FilterKind::Date {
            table: "issue",
            column: "start_date"
        },
        alias_of = "start_date"
    ),
    decl!(
        "start_date__range",
        FilterKind::DateRange {
            table: "issue",
            column: "start_date"
        }
    ),
    decl!(
        "target_date",
        FilterKind::Date {
            table: "issue",
            column: "target_date"
        }
    ),
    decl!(
        "target_date__exact",
        FilterKind::Date {
            table: "issue",
            column: "target_date"
        },
        alias_of = "target_date"
    ),
    decl!(
        "target_date__range",
        FilterKind::DateRange {
            table: "issue",
            column: "target_date"
        }
    ),
    decl!(
        "created_at",
        FilterKind::Datetime {
            table: "issue",
            column: "created_at"
        }
    ),
    decl!(
        "created_at__exact",
        FilterKind::Datetime {
            table: "issue",
            column: "created_at"
        },
        alias_of = "created_at"
    ),
    decl!(
        "created_at__range",
        FilterKind::DatetimeRange {
            table: "issue",
            column: "created_at"
        }
    ),
    decl!(
        "updated_at",
        FilterKind::Datetime {
            table: "issue",
            column: "updated_at"
        }
    ),
    decl!(
        "updated_at__exact",
        FilterKind::Datetime {
            table: "issue",
            column: "updated_at"
        },
        alias_of = "updated_at"
    ),
    decl!(
        "updated_at__range",
        FilterKind::DatetimeRange {
            table: "issue",
            column: "updated_at"
        }
    ),
    decl!(
        "is_draft",
        FilterKind::Flag {
            table: "issue",
            column: "is_draft"
        }
    ),
    decl!(
        "is_draft__exact",
        FilterKind::Flag {
            table: "issue",
            column: "is_draft"
        },
        alias_of = "is_draft"
    ),
    decl!(
        "priority",
        FilterKind::Text {
            table: "issue",
            column: "priority",
            many: false
        }
    ),
    decl!(
        "priority__exact",
        FilterKind::Text {
            table: "issue",
            column: "priority",
            many: false
        },
        alias_of = "priority"
    ),
    decl!(
        "priority__in",
        FilterKind::Text {
            table: "issue",
            column: "priority",
            many: true
        }
    ),
];

/// The `FilterSet.base_filters` keys: every declared name.
pub fn allowlist() -> HashSet<String> {
    ISSUE_FILTERSET
        .iter()
        .map(|decl| decl.name.to_owned())
        .collect()
}

fn find_decl(name: &str) -> Option<&'static FilterDecl> {
    ISSUE_FILTERSET.iter().find(|decl| decl.name == name)
}

/// The `QueryDict` round-trip `_build_leaf_q` puts every leaf value through:
/// lists repeat the key (`setlist`), scalars stringify (`None` → `""`,
/// booleans → `"True"` / `"False"`, numbers render plainly). Widgets then
/// read the LAST item (`QueryDict.get`), so a JSON list contributes only its
/// tail — e.g. `{"priority__in": ["high", "urgent"]}` filters `urgent` only.
fn stringify_scalar(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some(String::new()),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// The tail value every widget sees: a JSON list's last item, or the scalar
/// itself, serialized like `_build_leaf_q` — with one asymmetry the backend
/// has: a scalar `None` becomes `""`, but a `None` *inside* a list becomes
/// `"None"` (`str(None)` in the `setlist` comprehension). An empty list
/// reads as `""`.
fn tail_text(field: &str, value: &Value) -> Result<String, FilterError> {
    match value {
        Value::Array(items) => match items.last() {
            None => Ok(String::new()),
            Some(Value::Null) => Ok("None".to_owned()),
            Some(item) => stringify_scalar(item)
                .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned())),
        },
        Value::Null => Ok(String::new()),
        single => stringify_scalar(single)
            .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned())),
    }
}

/// A CSV-widget value: the tail item, comma-split. A wholly empty value
/// cleans to `[]` before per-item cleaning runs (`BaseCSVWidget`).
fn csv_parts(field: &str, value: &Value) -> Result<Vec<String>, FilterError> {
    let tail = tail_text(field, value)?;
    if tail.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(tail.split(',').map(str::to_owned).collect())
    }
}

fn parse_uuid_field(field: &str, text: &str) -> Result<Option<uuid::Uuid>, FilterError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    uuid::Uuid::parse_str(text)
        .map(Some)
        .map_err(|_| FilterError::InvalidLookupValue(field.to_owned()))
}

/// An `__in` list of UUIDs: the tail item, comma-split, each part cleaned
/// strictly. `""` parts clean to `None` (`UUIDField.to_python`) and are
/// dropped, the way `In.process_rhs` discards `None` (`lookups.py`); the
/// survivors bind as typed UUIDs. An empty list matches nothing for
/// direct filters (`IN ()` → `FALSE`); relation (method) filters skip the
/// method on wholly-empty input instead, but an all-`""` parts list runs
/// the method against `[]` (`EmptyResultSet`, match-nothing) — see
/// [`compile_leaf`], which distinguishes the two via [`csv_parts`].
fn uuid_in_list(field: &str, value: &Value) -> Result<Vec<uuid::Uuid>, FilterError> {
    let mut ids = Vec::new();
    for part in csv_parts(field, value)? {
        // `""` cleans to `None` and is dropped, like `In.process_rhs`.
        if let Some(id) = parse_uuid_field(field, &part)? {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// `NullBooleanSelect.value_from_datadict`: the exact widget map, including
/// the Django<2.2 `"2"` / `"3"` backcompat spellings. Anything else cleans
/// to `None`. No stripping: `" true "` misses the map and cleans to `None`.
/// (The typed `True`/`False` map keys only matter for un-stringified data,
/// which `_build_leaf_q` never sends — everything arrives stringified.)
fn parse_null_boolean(text: &str) -> Option<bool> {
    match text {
        "True" | "true" | "2" => Some(true),
        "False" | "false" | "3" => Some(false),
        _ => None,
    }
}

/// `filter_is_archived` on the tail value: `True` → `archived_at IS NOT
/// NULL`, `False` → `IS NULL`, `None` (anything else) → the method is
/// skipped, i.e. match-all (`Q(pk__in=qs)`), rendered here as `1 = 1`.
pub fn archived_condition(value: &Value) -> SimpleExpr {
    let col = Expr::col((Alias::new("issue"), Alias::new("archived_at")));
    let text = tail_text("is_archived", value).unwrap_or_default();
    match parse_null_boolean(&text) {
        Some(true) => col.is_not_null(),
        Some(false) => col.is_null(),
        // Method skipped: no filter.
        None => Expr::cust("1 = 1"),
    }
}

/// Django's `DATE_INPUT_FORMATS`: the only strings a `DATE` column accepts.
/// Python's `strptime` is strict about digit runs (`%Y` is exactly 4 digits,
/// `%y` exactly 2 with a 68/69 pivot), while chrono's `%Y`/`%y` accept any
/// width without pivoting — so the shapes below are tokenized by hand and
/// chrono only validates ranges and month names.
fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn parse_iso_date(text: &str) -> Option<chrono::NaiveDate> {
    let mut parts = text.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || year.len() != 4
        || !is_digits(year)
        || month.len() > 2
        || !is_digits(month)
        || day.len() > 2
        || !is_digits(day)
    {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

/// `%m/%d/%Y` and `%m/%d/%y` (pivot: 00-68 → 20xx, 69-99 → 19xx).
fn parse_us_date(text: &str) -> Option<chrono::NaiveDate> {
    let mut parts = text.split('/');
    let (month, day, year) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || month.len() > 2
        || !is_digits(month)
        || day.len() > 2
        || !is_digits(day)
        || !is_digits(year)
    {
        return None;
    }
    let year: i32 = match year.len() {
        4 => year.parse().ok()?,
        2 => {
            let short: i32 = year.parse().ok()?;
            if short >= 69 {
                1900 + short
            } else {
                2000 + short
            }
        }
        _ => return None,
    };
    chrono::NaiveDate::from_ymd_opt(year, month.parse().ok()?, day.parse().ok()?)
}

/// The month-name shapes (`%b`/`%B`, day first or month first, optional
/// comma): the year always comes last and is 4 digits; commas and extra
/// spaces are tolerated via tokenization.
fn parse_named_date(text: &str) -> Option<chrono::NaiveDate> {
    let cleaned = text.replace(',', " ");
    let tokens: Vec<&str> = cleaned.split_whitespace().collect();
    if tokens.len() != 3 {
        return None;
    }
    let year = tokens[2];
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let normalized = tokens.join(" ");
    ["%b %d %Y", "%d %b %Y", "%B %d %Y", "%d %B %Y"]
        .iter()
        .find_map(|format| chrono::NaiveDate::parse_from_str(&normalized, format).ok())
}

/// A `DATE` column value, cleaned exactly like `DateField`: ISO, US and
/// month-name shapes (Django's `DATE_INPUT_FORMATS`, strict digit runs);
/// datetime strings are rejected. Binds as a real `date`, like Python.
fn parse_date_value(field: &str, text: &str) -> Result<chrono::NaiveDate, FilterError> {
    parse_iso_date(text)
        .or_else(|| parse_us_date(text))
        .or_else(|| parse_named_date(text))
        .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))
}

/// Django's `DATETIME_INPUT_FORMATS`, plus the `T`-separated ISO shapes
/// `parse_datetime` accepts (offsets via `%:z` and `%z`).
const DATETIME_INPUT_FORMATS: &[&str] = &[
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%d %H:%M",
    "%m/%d/%Y %H:%M:%S",
    "%m/%d/%Y %H:%M:%S%.f",
    "%m/%d/%Y %H:%M",
    "%m/%d/%y %H:%M:%S",
    "%m/%d/%y %H:%M:%S%.f",
    "%m/%d/%y %H:%M",
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%dT%H:%M:%S%:z",
    "%Y-%m-%dT%H:%M:%S%.f%:z",
    "%Y-%m-%dT%H:%M%:z",
    "%Y-%m-%dT%H:%M:%S%z",
    "%Y-%m-%dT%H:%M:%S%.f%z",
    "%Y-%m-%d %H:%M:%S%:z",
    "%Y-%m-%d %H:%M:%S%.f%:z",
];

/// A US (`%m/%d/%...`) datetime whose fraction runs past 6 digits:
/// Django's `strptime` `%f` stops at 6 (`unconverted data remains`),
/// while chrono's `%.f` would take up to 9 and truncate. Only `/`
/// strings can take this shape (ISO never contains `/`, and
/// `parse_datetime` never matches one), so a `/` plus a trailing
/// 7-plus digit run is exactly the shape Django 400s.
fn has_long_us_fraction(text: &str) -> bool {
    let Some(dot) = text.rfind('.') else {
        return false;
    };
    let fraction = &text[dot + 1..];
    fraction.len() > 6 && fraction.bytes().all(|b| b.is_ascii_digit()) && text[..dot].contains('/')
}

/// Python's `fromisoformat` truncates >6-digit fractions to microseconds;
/// Postgres `timestamptz` stores micros (rounding on bind), so truncate
/// first to compare the instant Django compares.
fn truncate_to_micros(moment: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    let nanos = moment.timestamp_subsec_nanos();
    moment
        .with_nanosecond(nanos / 1000 * 1000)
        .expect("truncated nanos are in range")
}

/// `from_current_timezone` (`forms/utils.py`): a naive wall time resolves
/// in the request user's zone (`TimezoneMixin` activates `user_timezone`
/// in `app/views/base.py`). DST gaps and ambiguous (fold) wall times are
/// a filterset error (400), exactly like Django's `ValidationError`.
fn attach_user_tz(
    field: &str,
    naive: &chrono::NaiveDateTime,
    tz: &chrono_tz::Tz,
) -> Result<chrono::DateTime<chrono::Utc>, FilterError> {
    match tz.from_local_datetime(naive) {
        chrono::LocalResult::Single(aware) => {
            Ok(truncate_to_micros(aware.with_timezone(&chrono::Utc)))
        }
        chrono::LocalResult::Ambiguous(_, _) | chrono::LocalResult::None => {
            Err(FilterError::InvalidLookupValue(field.to_owned()))
        }
    }
}

/// A `DateTimeField` column value, cleaned like Django: ISO-8601 with an
/// offset (via `parse_datetime`, incl. `Z`) honors the offset; the naive
/// `DATETIME_INPUT_FORMATS` shapes and date-only strings (midnight)
/// resolve in the request user's zone. Binds as a real `timestamptz`,
/// like Python. (A lowercase `z` is rejected: Django's `parse_datetime`
/// does not accept it.)
fn parse_datetime_utc(
    field: &str,
    text: &str,
    tz: &chrono_tz::Tz,
) -> Result<chrono::DateTime<chrono::Utc>, FilterError> {
    // A lowercase `z` is never valid: Django's `parse_datetime` only
    // takes an uppercase `Z`, while chrono's `%z` would take either.
    if text.ends_with('z') {
        return Err(FilterError::InvalidLookupValue(field.to_owned()));
    }
    // Python's `strptime` `%f` takes at most 6 digits, while chrono's
    // `%.f` takes up to 9: a US datetime with a 7-9 digit fraction fails
    // every `DATETIME_INPUT_FORMAT` in Django (400) but would parse here
    // and truncate. Reject it before chrono sees it. (ISO shapes keep
    // truncating, like `fromisoformat`.)
    if has_long_us_fraction(text) {
        return Err(FilterError::InvalidLookupValue(field.to_owned()));
    }
    // ISO-8601 with an offset: the offset wins, like `parse_datetime` —
    // the request timezone plays no part.
    if let Ok(aware) = chrono::DateTime::parse_from_rfc3339(text) {
        return Ok(truncate_to_micros(aware.with_timezone(&chrono::Utc)));
    }
    for format in DATETIME_INPUT_FORMATS {
        if format.contains("%z") || format.contains("%:z") {
            // Offset-bearing shapes: honor the offset, never the zone.
            // (`NaiveDateTime` would parse these too and silently drop
            // the offset.)
            if let Ok(aware) = chrono::DateTime::parse_from_str(text, format) {
                return Ok(truncate_to_micros(aware.with_timezone(&chrono::Utc)));
            }
        } else if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return attach_user_tz(field, &naive, tz);
        }
    }
    // Date-only strings clean to midnight (`DateTimeField` falls back to
    // `parse_date`); strictness matches the `DATE` columns above.
    if let Some(date) = parse_iso_date(text)
        .or_else(|| parse_us_date(text))
        .or_else(|| parse_named_date(text))
    {
        let midnight = date.and_hms_opt(0, 0, 0).expect("midnight exists");
        return attach_user_tz(field, &midnight, tz);
    }
    Err(FilterError::InvalidLookupValue(field.to_owned()))
}

fn col(table: &'static str, column: &'static str) -> Expr {
    Expr::col((Alias::new(table), Alias::new(column)))
}

/// Compile one `{name: value}` leaf against the declaration table.
/// Mirrors `_validate_fields` (unknown names are rejected) plus
/// `build_combined_q` for a single filter. Every value goes through the
/// `QueryDict` tail model first ([`tail_text`]/[`csv_parts`]). `tz` is the
/// request user's zone, for naive datetime inputs.
pub fn compile_leaf(
    name: &str,
    value: &Value,
    tz: &chrono_tz::Tz,
) -> Result<SimpleExpr, FilterError> {
    let decl = find_decl(name).ok_or_else(|| FilterError::InvalidField(name.to_owned()))?;
    let field = decl.alias_of.unwrap_or(decl.name);
    match decl.kind {
        FilterKind::Uuid {
            table,
            column,
            many,
        } => {
            let target = col(table, column);
            if many {
                let ids = uuid_in_list(field, value)?;
                if ids.is_empty() {
                    // `Q(x__in=[])`: matches nothing.
                    Ok(Expr::cust("FALSE"))
                } else {
                    Ok(target.is_in(ids))
                }
            } else {
                let text = tail_text(field, value)?;
                match parse_uuid_field(field, &text)? {
                    Some(id) => Ok(target.eq(id)),
                    // `UUIDField("")` cleans to None.
                    None => Ok(target.is_null()),
                }
            }
        }
        FilterKind::Relation {
            join_table,
            join_column,
            many,
        } => {
            let target = col(join_table, join_column);
            let guard = col(join_table, "deleted_at").is_null();
            if many {
                let parts = csv_parts(field, value)?;
                if parts.is_empty() {
                    // The custom method is skipped on empty input
                    // (`Filter.filter` empty-check): match-all, no guard.
                    Ok(Expr::cust("1 = 1"))
                } else {
                    let mut ids = Vec::with_capacity(parts.len());
                    for part in &parts {
                        if let Some(id) = parse_uuid_field(field, part)? {
                            ids.push(id);
                        }
                    }
                    if ids.is_empty() {
                        // Every part cleaned to `None`: the method runs
                        // against `[]` (`EmptyResultSet`) and matches
                        // nothing.
                        Ok(Expr::cust("FALSE"))
                    } else {
                        Ok(target.is_in(ids).and(guard))
                    }
                }
            } else {
                let text = tail_text(field, value)?;
                match parse_uuid_field(field, &text)? {
                    Some(id) => Ok(target.eq(id).and(guard)),
                    // The custom method is skipped: match-all, no guard.
                    None => Ok(Expr::cust("1 = 1")),
                }
            }
        }
        FilterKind::Text {
            table,
            column,
            many,
        } => {
            let target = col(table, column);
            if many {
                // Items are verbatim (no stripping, `""` kept); an empty
                // list is `Q(x__in=[])` and matches nothing.
                let parts = csv_parts(field, value)?;
                if parts.is_empty() {
                    Ok(Expr::cust("FALSE"))
                } else {
                    Ok(target.is_in(parts))
                }
            } else {
                // `CharField` strips.
                Ok(target.eq(tail_text(field, value)?.trim().to_owned()))
            }
        }
        FilterKind::Flag { table, column } => {
            let text = tail_text(field, value)?;
            match parse_null_boolean(&text) {
                Some(flag) => Ok(col(table, column).eq(flag)),
                // `NullBooleanField` cleans anything else to `None`.
                None => Ok(col(table, column).is_null()),
            }
        }
        FilterKind::Date { table, column } => {
            let text = tail_text(field, value)?;
            let text = text.trim();
            if text.is_empty() {
                Ok(col(table, column).is_null())
            } else {
                Ok(col(table, column).eq(parse_date_value(field, text)?))
            }
        }
        FilterKind::Datetime { table, column } => {
            let text = tail_text(field, value)?;
            let text = text.trim();
            if text.is_empty() {
                Ok(col(table, column).is_null())
            } else {
                Ok(col(table, column).eq(parse_datetime_utc(field, text, tz)?))
            }
        }
        FilterKind::DateRange { table, column } => {
            compile_range(field, value, col(table, column), RangeKind::Date, tz)
        }
        FilterKind::DatetimeRange { table, column } => {
            compile_range(field, value, col(table, column), RangeKind::Datetime, tz)
        }
        FilterKind::Archived => Ok(archived_condition(value)),
    }
}

/// Which column flavor a `__range` filter binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeKind {
    Date,
    Datetime,
}

/// A `BaseRangeField`: the tail item, comma-split, exactly two parts
/// (`BaseRangeField.clean`, else `Range query expects two values`). An empty
/// whole value cleans to `[]` and only fails at SQL-compile time in Python
/// ([`FilterError::EmptyRangeBounds`]); an empty *part* cleans to `None` and
/// binds `NULL`. Bounds bind as typed parameters, like the `eq`/`IN` arms
/// (a `Constant` here would render inline as an untyped literal, leaving
/// naive datetimes to the Postgres session zone instead of `tz`).
fn compile_range(
    field: &str,
    value: &Value,
    target: Expr,
    kind: RangeKind,
    tz: &chrono_tz::Tz,
) -> Result<SimpleExpr, FilterError> {
    use sea_query::Value as SeaValue;
    let tail = tail_text(field, value)?;
    if tail.is_empty() {
        return Err(FilterError::EmptyRangeBounds(field.to_owned()));
    }
    let parts: Vec<&str> = tail.split(',').collect();
    if parts.len() != 2 {
        return Err(FilterError::InvalidLookupValue(field.to_owned()));
    }
    let mut bounds = Vec::with_capacity(2);
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            bounds.push(match kind {
                RangeKind::Date => SimpleExpr::Value(SeaValue::ChronoDate(None)),
                RangeKind::Datetime => SimpleExpr::Value(SeaValue::ChronoDateTimeUtc(None)),
            });
        } else if kind == RangeKind::Date {
            bounds.push(SimpleExpr::Value(SeaValue::ChronoDate(Some(Box::new(
                parse_date_value(field, part)?,
            )))));
        } else {
            bounds.push(SimpleExpr::Value(SeaValue::ChronoDateTimeUtc(Some(
                Box::new(parse_datetime_utc(field, part, tz)?),
            ))));
        }
    }
    Ok(target.between(bounds[0].clone(), bounds[1].clone()))
}

/// Port of `build_combined_q`: AND the conditions for the filters actually
/// provided, in the caller's order. Unknown names are rejected. `tz` is the
/// request user's zone, for naive datetime inputs.
pub fn build_combined(
    provided: &[(String, Value)],
    tz: &chrono_tz::Tz,
) -> Result<Condition, FilterError> {
    let mut combined = Condition::all();
    for (name, value) in provided {
        combined = combined.add(compile_leaf(name, value, tz)?);
    }
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    fn select_where(expr: SimpleExpr) -> String {
        Query::select()
            .expr(Expr::cust("1"))
            .and_where(expr)
            .to_string(PostgresQueryBuilder)
    }

    /// [`compile_leaf`] in UTC (naive datetimes resolve to the same wall
    /// time); tests needing another zone call it directly.
    fn leaf(name: &str, value: &Value) -> Result<SimpleExpr, FilterError> {
        compile_leaf(name, value, &chrono_tz::UTC)
    }

    /// [`build_combined`] in UTC.
    fn combined(provided: &[(String, Value)]) -> Result<Condition, FilterError> {
        build_combined(provided, &chrono_tz::UTC)
    }

    #[test]
    fn state_group_constants_match_python() {
        assert_eq!(
            STATE_GROUP_ORDER,
            &[
                "backlog",
                "unstarted",
                "started",
                "review",
                "test",
                "completed",
                "cancelled"
            ]
        );
        assert_eq!(
            ACTIVE_STATE_GROUPS,
            &["unstarted", "started", "review", "test"]
        );
    }

    #[test]
    fn allowlist_covers_every_declared_name_and_alias_rule() {
        let allowed = allowlist();
        assert_eq!(allowed.len(), ISSUE_FILTERSET.len());
        for decl in ISSUE_FILTERSET {
            assert!(allowed.contains(decl.name), "missing {0}", decl.name);
            match decl.alias_of {
                Some(base) => {
                    assert!(decl.name.ends_with("__exact"), "{}", decl.name);
                    assert!(allowed.contains(base), "alias base {base} missing");
                }
                None => {
                    let alias = format!("{}__exact", decl.name);
                    let is_exact_lookup =
                        !decl.name.ends_with("__in") && !decl.name.ends_with("__range");
                    if is_exact_lookup {
                        assert!(allowed.contains(&alias), "missing alias {alias}");
                    } else {
                        assert!(!allowed.contains(&alias), "unexpected alias {alias}");
                    }
                }
            }
        }
        // Spot checks against filterset.py.
        for name in [
            "assignee_id",
            "assignee_id__in",
            "cycle_id__in",
            "module_id__in",
            "mention_id__in",
            "label_id__in",
            "created_by_id__in",
            "is_archived",
            "state_group__in",
            "state_id__in",
            "project_id__in",
            "subscriber_id__in",
            "start_date__range",
            "priority__in",
            "is_draft",
        ] {
            assert!(allowed.contains(name), "missing {name}");
        }
    }

    #[test]
    fn relation_method_adds_soft_delete_guard() {
        let id = "123e4567-e89b-42d3-a456-426614174000";
        let rendered = select_where(leaf("assignee_id", &Value::from(id)).unwrap());
        assert_eq!(
            rendered,
            format!(
                r#"SELECT 1 WHERE "issue_assignee"."assignee_id" = '{id}' AND "issue_assignee"."deleted_at" IS NULL"#
            )
        );
        let rendered =
            select_where(leaf("label_id__in", &Value::Array(vec![Value::from(id)])).unwrap());
        assert!(
            rendered.contains(r#""label_issue"."label_id" IN ('"#),
            "{rendered}"
        );
        assert!(
            rendered.contains(r#""label_issue"."deleted_at" IS NULL"#),
            "{rendered}"
        );
    }

    #[test]
    fn invalid_uuid_is_rejected() {
        assert_eq!(
            leaf("assignee_id", &Value::from("not-a-uuid")),
            Err(FilterError::InvalidLookupValue("assignee_id".to_owned()))
        );
        assert_eq!(
            leaf("nope", &Value::from("x")),
            Err(FilterError::InvalidField("nope".to_owned()))
        );
    }

    #[test]
    fn archived_tri_state_matches_python() {
        let truthy = select_where(leaf("is_archived", &Value::from("true")).unwrap());
        assert!(truthy.contains(r#""archived_at" IS NOT NULL"#), "{truthy}");
        let falsy = select_where(leaf("is_archived", &Value::Bool(false)).unwrap());
        assert!(falsy.contains(r#""archived_at" IS NULL"#), "{falsy}");
        assert!(!falsy.contains("IS NOT NULL"), "{falsy}");
        // The Django<2.2 backcompat spellings ride along.
        let two = select_where(leaf("is_archived", &Value::from("2")).unwrap());
        assert!(two.contains("IS NOT NULL"), "{two}");
        let three = select_where(leaf("is_archived", &Value::from("3")).unwrap());
        assert!(three.contains(r#""archived_at" IS NULL"#), "{three}");
        // Unrecognized values skip the method (match-all): the backend
        // stringifies first, so `"1"`, `"1.0"` and `"maybe"` all miss the
        // widget map — typed matching never happens on this path.
        for raw in ["maybe", "1", "0", "TRUE", "1.0", ""] {
            let noop = select_where(leaf("is_archived", &Value::from(raw)).unwrap());
            assert!(noop.contains("1 = 1"), "{raw}: {noop}");
        }
        let one = select_where(leaf("is_archived", &serde_json::json!(1.0)).unwrap());
        assert!(one.contains("1 = 1"), "{one}");
        let null = select_where(leaf("is_archived", &Value::Null).unwrap());
        assert!(null.contains("1 = 1"), "{null}");
    }

    #[test]
    fn in_filters_read_the_last_item_and_comma_split_it() {
        // Oracle: the CSV widget reads `QueryDict.get` (the LAST item) and
        // comma-splits it — a JSON list contributes only its tail.
        let rendered = select_where(
            leaf(
                "priority__in",
                &Value::Array(vec![Value::from("high"), Value::from("urgent")]),
            )
            .unwrap(),
        );
        assert!(
            rendered.contains(r#""issue"."priority" IN ('urgent')"#),
            "{rendered}"
        );
        let rendered = select_where(leaf("priority__in", &Value::from("high,urgent")).unwrap());
        assert!(
            rendered.contains(r#""issue"."priority" IN ('high', 'urgent')"#),
            "{rendered}"
        );
        // Items are verbatim: no stripping, `""` and `"None"` kept.
        let rendered =
            select_where(leaf("priority__in", &Value::Array(vec![Value::Null])).unwrap());
        assert!(
            rendered.contains(r#""issue"."priority" IN ('None')"#),
            "{rendered}"
        );
        // An empty tail is `Q(x__in=[])` and matches nothing.
        let rendered = select_where(leaf("priority__in", &Value::from("")).unwrap());
        assert!(rendered.contains("FALSE"), "{rendered}");
        // UUID entries canonicalize to lowercase; the tail wins.
        let id = "123e4567-e89b-42d3-a456-426614174000";
        let rendered = select_where(
            leaf(
                "created_by_id__in",
                &Value::Array(vec![
                    Value::from("bogus-should-be-ignored"),
                    Value::from(id),
                ]),
            )
            .unwrap(),
        );
        assert!(rendered.contains(id), "{rendered}");
        assert!(!rendered.contains("bogus"), "{rendered}");
        // ...while a bad tail is a filterset error, even with a good head.
        assert_eq!(
            leaf(
                "created_by_id__in",
                &Value::Array(vec![Value::from(id), Value::from("bogus")]),
            ),
            Err(FilterError::InvalidLookupValue(
                "created_by_id__in".to_owned()
            ))
        );
    }

    #[test]
    fn relation_empty_means_match_all_without_guard() {
        // Oracle: the custom method is skipped on empty input
        // (`Filter.filter` empty-check), so the soft-delete guard goes too.
        for raw in [Value::Null, Value::from("")] {
            let rendered = select_where(leaf("assignee_id", &raw).unwrap());
            assert!(rendered.contains("1 = 1"), "{rendered}");
            assert!(!rendered.contains("deleted_at"), "{rendered}");
            let rendered = select_where(leaf("assignee_id__in", &raw).unwrap());
            assert!(rendered.contains("1 = 1"), "{rendered}");
            assert!(!rendered.contains("deleted_at"), "{rendered}");
        }
        // A literal "None" is not empty: UUID cleaning rejects it.
        assert_eq!(
            leaf("assignee_id", &Value::from("None")),
            Err(FilterError::InvalidLookupValue("assignee_id".to_owned()))
        );
        assert_eq!(
            leaf("assignee_id__in", &Value::Array(vec![Value::Null])),
            Err(FilterError::InvalidLookupValue(
                "assignee_id__in".to_owned()
            ))
        );
    }

    #[test]
    fn null_boolean_spellings_match_the_widget_map() {
        // Oracle (`NullBooleanSelect.value_from_datadict`): only these six
        // spellings clean to a boolean; everything else (`"1"`, `"0"`,
        // `"TRUE"`, `"1.0"`, `""`, null) cleans to `None` → `IS NULL`.
        for raw in ["True", "true", "2"] {
            let rendered = select_where(leaf("is_draft", &Value::from(raw)).unwrap());
            assert!(
                rendered.contains(r#""issue"."is_draft" = TRUE"#),
                "{raw}: {rendered}"
            );
        }
        for raw in ["False", "false", "3"] {
            let rendered = select_where(leaf("is_draft", &Value::from(raw)).unwrap());
            assert!(
                rendered.contains(r#""issue"."is_draft" = FALSE"#),
                "{raw}: {rendered}"
            );
        }
        for raw in [
            Value::from("1"),
            Value::from("0"),
            Value::from("TRUE"),
            Value::from("maybe"),
            Value::from(""),
            Value::Null,
            serde_json::json!(1.0),
            serde_json::json!(0.0),
        ] {
            let rendered = select_where(leaf("is_draft", &raw).unwrap());
            assert!(
                rendered.contains(r#""issue"."is_draft" IS NULL"#),
                "{raw:?}: {rendered}"
            );
        }
        // Booleans arrive stringified (`"True"`/`"False"`), like the backend.
        let rendered = select_where(leaf("is_draft", &Value::Bool(true)).unwrap());
        assert!(
            rendered.contains(r#""issue"."is_draft" = TRUE"#),
            "{rendered}"
        );
    }

    #[test]
    fn direct_fields_compile() {
        let rendered = select_where(
            leaf(
                "state_group__in",
                &Value::Array(vec![Value::from("backlog")]),
            )
            .unwrap(),
        );
        assert!(
            rendered.contains(r#""state"."group" IN ('backlog')"#),
            "{rendered}"
        );
        let rendered = select_where(leaf("priority", &Value::from("urgent")).unwrap());
        assert!(
            rendered.contains(r#""issue"."priority" = 'urgent'"#),
            "{rendered}"
        );
        // Exact text strips (`CharField`); the tail wins for lists.
        let rendered = select_where(leaf("priority", &Value::from(" high ")).unwrap());
        assert!(
            rendered.contains(r#""issue"."priority" = 'high'"#),
            "{rendered}"
        );
        let rendered = select_where(
            leaf(
                "priority",
                &Value::Array(vec![Value::from("high"), Value::from("urgent")]),
            )
            .unwrap(),
        );
        assert!(
            rendered.contains(r#""issue"."priority" = 'urgent'"#),
            "{rendered}"
        );
        // A comma string ranges; a two-list is one value too many.
        let rendered =
            select_where(leaf("start_date__range", &Value::from("2024-01-01,2024-02-01")).unwrap());
        assert!(rendered.contains("BETWEEN"), "{rendered}");
        assert!(rendered.contains("2024-01-01"), "{rendered}");
        assert!(rendered.contains("2024-02-01"), "{rendered}");
        assert_eq!(
            leaf(
                "start_date__range",
                &Value::Array(vec![Value::from("2024-01-01"), Value::from("2024-02-01")]),
            ),
            Err(FilterError::InvalidLookupValue(
                "start_date__range".to_owned()
            ))
        );
        assert_eq!(
            leaf("start_date__range", &Value::from("2024-01-01")),
            Err(FilterError::InvalidLookupValue(
                "start_date__range".to_owned()
            ))
        );
        // An empty range stays valid in Python and fails at SQL-compile
        // time — a 500, unlike the 400 above.
        assert_eq!(
            leaf("start_date__range", &Value::from("")),
            Err(FilterError::EmptyRangeBounds(
                "start_date__range".to_owned()
            ))
        );
        // An empty part binds NULL.
        let rendered =
            select_where(leaf("start_date__range", &Value::from("2024-01-01,")).unwrap());
        assert!(rendered.contains("BETWEEN"), "{rendered}");
        assert!(rendered.contains("NULL"), "{rendered}");
        let rendered = select_where(leaf("is_draft", &Value::Bool(true)).unwrap());
        assert!(
            rendered.contains(r#""issue"."is_draft" = TRUE"#),
            "{rendered}"
        );
    }

    #[test]
    fn date_columns_take_django_formats_and_reject_datetimes() {
        // US and month-name dates clean; values normalize to ISO.
        for (raw, iso) in [
            ("01/15/2024", "2024-01-15"),
            ("1/5/24", "2024-01-05"),
            ("15 Jan 2024", "2024-01-15"),
            ("October 25, 2006", "2006-10-25"),
        ] {
            let rendered = select_where(leaf("start_date", &Value::from(raw)).unwrap());
            assert!(rendered.contains(iso), "{raw}: {rendered}");
        }
        // Datetime strings are rejected on DATE columns (400).
        for raw in ["2024-01-02T03:04:05Z", "2024-01-02 03:04:05"] {
            assert_eq!(
                leaf("start_date", &Value::from(raw)),
                Err(FilterError::InvalidLookupValue("start_date".to_owned())),
                "{raw}"
            );
        }
        // ...but resolved to instants on datetime columns (UTC here, so
        // naive walls are unchanged; offsets shift to UTC; date-only
        // means midnight).
        for (raw, instant) in [
            ("2024-01-02T03:04:05Z", "2024-01-02 03:04:05"),
            ("2024-01-02 03:04:05", "2024-01-02 03:04:05"),
            ("01/15/2024", "2024-01-15 00:00:00"),
            ("2024-01-02T03:04:05+05:30", "2024-01-01 21:34:05"),
        ] {
            let rendered = select_where(leaf("created_at", &Value::from(raw)).unwrap());
            assert!(rendered.contains(instant), "{raw}: {rendered}");
        }
        // Garbage is still a filterset error.
        assert_eq!(
            leaf("start_date", &Value::from("yesterday")),
            Err(FilterError::InvalidLookupValue("start_date".to_owned()))
        );
        assert_eq!(
            leaf("created_at", &Value::from("yesterday")),
            Err(FilterError::InvalidLookupValue("created_at".to_owned()))
        );
    }

    #[test]
    fn null_and_empty_string_follow_the_querydict_round_trip() {
        // A direct UUID `""` cleans to None: IS NULL, never an error.
        let rendered = select_where(leaf("created_by_id", &Value::Null).unwrap());
        assert!(
            rendered.contains(r#""issue"."created_by_id" IS NULL"#),
            "{rendered}"
        );
        let rendered = select_where(leaf("created_by_id", &Value::from("  ")).unwrap());
        assert!(rendered.contains("IS NULL"), "{rendered}");
        // CharField("") matches the empty string (stripped first).
        let rendered = select_where(leaf("priority", &Value::Null).unwrap());
        assert!(
            rendered.contains(r#""issue"."priority" = ''"#),
            "{rendered}"
        );
        // NullBooleanField cleans null to None: IS NULL, never FALSE.
        let rendered = select_where(leaf("is_draft", &Value::Null).unwrap());
        assert!(
            rendered.contains(r#""issue"."is_draft" IS NULL"#),
            "{rendered}"
        );
        // DateField("") cleans to None.
        let rendered = select_where(leaf("start_date", &Value::Null).unwrap());
        assert!(
            rendered.contains(r#""issue"."start_date" IS NULL"#),
            "{rendered}"
        );
        // Datetime strings resolve to instants for datetime columns.
        let rendered =
            select_where(leaf("created_at", &Value::from("2024-01-02T03:04:05Z")).unwrap());
        assert!(rendered.contains("2024-01-02 03:04:05"), "{rendered}");
        // A direct `__in` of only nulls is a filterset error: the `"None"`
        // tail item fails UUID cleaning.
        assert_eq!(
            leaf("created_by_id__in", &Value::Array(vec![Value::Null])),
            Err(FilterError::InvalidLookupValue(
                "created_by_id__in".to_owned()
            ))
        );
    }

    #[test]
    fn uuid_in_lists_drop_empty_parts() {
        let id = "123e4567-e89b-42d3-a456-426614174000";
        // A blank part cleans to `None` and is dropped: the survivors
        // filter, and the values bind as typed UUIDs.
        let rendered = select_where(leaf("state_id__in", &Value::from(format!("{id},"))).unwrap());
        assert!(rendered.contains(id), "{rendered}");
        assert!(!rendered.contains("FALSE"), "{rendered}");
        // All blank: direct filters match nothing ...
        let rendered = select_where(leaf("state_id__in", &Value::from(",")).unwrap());
        assert!(rendered.contains("FALSE"), "{rendered}");
        // ... and relation filters run against `[]` (match nothing) ...
        let rendered = select_where(leaf("assignee_id__in", &Value::from(",")).unwrap());
        assert!(rendered.contains("FALSE"), "{rendered}");
        assert!(!rendered.contains("1 = 1"), "{rendered}");
        // ... while a wholly empty value skips the method (match-all).
        // (Covered by `relation_empty_means_match_all_without_guard`.)
    }

    #[test]
    fn naive_datetimes_resolve_in_request_timezone() {
        // 08:00 on the New York wall clock (EDT) is 12:00 UTC.
        let rendered = select_where(
            compile_leaf(
                "created_at",
                &Value::from("2026-05-01 08:00:00"),
                &chrono_tz::America::New_York,
            )
            .unwrap(),
        );
        assert!(rendered.contains("2026-05-01 12:00:00"), "{rendered}");
        // An explicit offset wins over the zone.
        let rendered = select_where(
            compile_leaf(
                "created_at",
                &Value::from("2026-05-01T08:00:00+02:00"),
                &chrono_tz::America::New_York,
            )
            .unwrap(),
        );
        assert!(rendered.contains("2026-05-01 06:00:00"), "{rendered}");
    }

    #[test]
    fn datetime_edges_match_django() {
        let ny = &chrono_tz::America::New_York;
        // DST gap and ambiguous wall times are a filterset error (400),
        // like Django's `ValidationError`.
        for raw in ["2026-03-08 02:30:00", "2026-11-01 01:30:00"] {
            assert_eq!(
                compile_leaf("created_at", &Value::from(raw), ny),
                Err(FilterError::InvalidLookupValue("created_at".to_owned())),
                "{raw}"
            );
        }
        // Django's `parse_datetime` rejects a lowercase `z`.
        assert_eq!(
            compile_leaf("created_at", &Value::from("2026-05-01T08:00:00z"), ny),
            Err(FilterError::InvalidLookupValue("created_at".to_owned()))
        );
        // >6-digit fractions truncate to microseconds (no rounding).
        let moment = parse_datetime_utc(
            "created_at",
            "2026-05-01 08:00:00.123456789",
            &chrono_tz::UTC,
        )
        .unwrap();
        assert_eq!(moment.timestamp_subsec_nanos(), 123_456_000);
    }

    #[test]
    fn us_datetimes_reject_fractions_past_six_digits() {
        // Django's `strptime` `%f` takes at most 6 digits, so US datetimes
        // with 7-9 digit fractions fail every `DATETIME_INPUT_FORMAT`
        // (400); chrono's `%.f` would take up to 9 and truncate.
        for raw in [
            "05/01/2026 08:00:00.1234567",
            "05/01/2026 08:00:00.12345678",
            "05/01/2026 08:00:00.123456789",
            "05/01/26 08:00:00.1234567",
        ] {
            assert_eq!(
                leaf("created_at", &Value::from(raw)),
                Err(FilterError::InvalidLookupValue("created_at".to_owned())),
                "{raw}"
            );
        }
        // The same cleaning path 400s a `__range` bound.
        assert_eq!(
            leaf(
                "created_at__range",
                &Value::from("05/01/2026 08:00:00.1234567,05/02/2026 08:00:00"),
            ),
            Err(FilterError::InvalidLookupValue(
                "created_at__range".to_owned()
            ))
        );
        // The 6-digit boundary still cleans, to the exact microsecond.
        let moment =
            parse_datetime_utc("created_at", "05/01/2026 08:00:00.123456", &chrono_tz::UTC)
                .unwrap();
        assert_eq!(moment.timestamp_subsec_nanos(), 123_456_000);
    }

    #[test]
    fn ranges_bind_typed_parameters() {
        // `BETWEEN` bounds parameterize (no inline literals), carrying
        // typed `date` / `timestamptz` values — and naive datetimes
        // resolve in the request zone before binding.
        let expr = leaf("start_date__range", &Value::from("2024-01-01,2024-02-01")).unwrap();
        let (sql, values) = Query::select()
            .expr(Expr::cust("1"))
            .and_where(expr)
            .build(PostgresQueryBuilder);
        assert!(sql.contains("BETWEEN $1 AND $2"), "{sql}");
        assert_eq!(values.0.len(), 2);
        assert!(matches!(values.0[0], sea_query::Value::ChronoDate(_)));
        assert!(matches!(values.0[1], sea_query::Value::ChronoDate(_)));
        // An empty part binds a typed NULL.
        let expr = leaf("start_date__range", &Value::from("2024-01-01,")).unwrap();
        let (_, values) = Query::select()
            .expr(Expr::cust("1"))
            .and_where(expr)
            .build(PostgresQueryBuilder);
        assert!(matches!(values.0[0], sea_query::Value::ChronoDate(Some(_))));
        assert!(matches!(values.0[1], sea_query::Value::ChronoDate(None)));
        // 08:00 wall in New York (EDT) binds as 12:00 UTC.
        let expr = compile_leaf(
            "created_at__range",
            &Value::from("2026-05-01 08:00:00,2026-05-02 08:00:00"),
            &chrono_tz::America::New_York,
        )
        .unwrap();
        let (sql, values) = Query::select()
            .expr(Expr::cust("1"))
            .and_where(expr)
            .build(PostgresQueryBuilder);
        assert!(sql.contains("BETWEEN $1 AND $2"), "{sql}");
        let instants: Vec<String> = values
            .0
            .iter()
            .map(|bound| match bound {
                sea_query::Value::ChronoDateTimeUtc(Some(moment)) => {
                    moment.format("%Y-%m-%d %H:%M:%S").to_string()
                }
                other => panic!("expected a timestamptz bound, got {other:?}"),
            })
            .collect();
        assert_eq!(instants, &["2026-05-01 12:00:00", "2026-05-02 12:00:00"]);
    }

    #[test]
    fn combined_ands_provided_filters_in_order() {
        let cond = combined(&[
            ("priority".to_owned(), Value::from("high")),
            ("is_draft".to_owned(), Value::Bool(false)),
        ])
        .unwrap();
        let rendered = Query::select()
            .expr(Expr::cust("1"))
            .cond_where(cond)
            .to_string(PostgresQueryBuilder);
        assert!(
            rendered.contains(r#""issue"."priority" = 'high'"#),
            "{rendered}"
        );
        assert!(
            rendered.contains(r#""issue"."is_draft" = FALSE"#),
            "{rendered}"
        );
        assert!(combined(&[("nope".to_owned(), Value::Null)]).is_err());
    }
}
