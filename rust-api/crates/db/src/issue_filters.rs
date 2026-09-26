//! Legacy issue-list filters: `issue_filters` and friends.
//!
//! Ports `pi_dash/utils/issue_filters.py` (the `ISSUE_FILTER` dispatcher and
//! every `filter_*` function). Views call this with the request's query
//! params (`"GET"`: `request.query_params`, plain strings) or with a parsed
//! JSON body (`"POST"`: analytic/view serializers store the compiled dict as
//! `validated_data["query"]`; `"PATCH"` takes the same else-branch).
//!
//! Output is an ordered predicate list (first-write order, last value wins —
//! the Python `dict` semantics), each key already prefixed (`prefix` is
//! `"issue__"` for intake queries, else `""`).
//!
//! Ported bugs (also listed in the PR):
//!
//! - `filter_updated_at` writes `created_at__date`, never `updated_at__date`.
//! - `filter_intake_status` on POST reads the `inbox_status` param.
//! - The `"" not in ...` guard is dead for UUID filters: it runs on parsed
//!   `Uuid` values, which never equal `""`, so trailing commas do not veto.
//! - A two-part relative date (`"2_weeks;before"`) matches the pattern but
//!   has no offset part, so it filters nothing.
//! - `filter_issue_state_type` with any `type` other than `backlog`/`active`
//!   (including the `"all"` default) filters to the full group order.
//!
//! Deliberate normalizations (Python behavior no port should copy):
//!
//! - Relative magnitudes are parsed as `u64`; absurd magnitudes that Python
//!   would `OverflowError` on become [`IssueFilterError::DateOverflow`].
//! - The `\d` regex class is ASCII digits; `int()`-accepted exotic numerals
//!   fall through to the explicit-date branch.

use std::collections::HashMap;

use chrono::NaiveDate;
use thiserror::Error as ThisError;

/// `issue_filters(query_params, method)`: anything but `"GET"` takes the
/// POST branch (callers pass `"PATCH"` too).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub fn from_name(method: &str) -> Self {
        if method == "GET" {
            Method::Get
        } else {
            Method::Post
        }
    }
}

/// A POST value: a JSON array (stored raw) or a JSON string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostVal {
    List(Vec<String>),
    Text(String),
}

/// Why legacy compilation failed. Python lets these escape as 500s
/// (`TypeError`, `OverflowError`); the handler maps this error the same way.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum IssueFilterError {
    #[error("relative date magnitude overflows the calendar")]
    DateOverflow,
}

/// One compiled predicate value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterValue {
    Uuids(Vec<uuid::Uuid>),
    Strings(Vec<String>),
    Text(String),
    Flag(bool),
    Day(NaiveDate),
    /// A stored Python `None` (the POST `intake_status` branch writes
    /// `params.get("inbox_status")` verbatim, `None` when absent).
    Null,
}

/// The compiled filter: ordered `(field__lookup, value)` pairs, last value
/// winning on repeat keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueFilter {
    predicates: Vec<(String, FilterValue)>,
}

impl IssueFilter {
    pub fn set(&mut self, key: String, value: FilterValue) {
        match self.predicates.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.predicates.push((key, value)),
        }
    }

    pub fn get(&self, key: &str) -> Option<&FilterValue> {
        self.predicates
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    pub fn is_empty(&self) -> bool {
        self.predicates.is_empty()
    }

    pub fn predicates(&self) -> &[(String, FilterValue)] {
        &self.predicates
    }
}

/// `filter_valid_uuids`: invalid entries are silently dropped.
pub fn filter_valid_uuids(items: &[String]) -> Vec<uuid::Uuid> {
    items
        .iter()
        .filter_map(|item| uuid::Uuid::parse_str(item).ok())
        .collect()
}

/// `re.compile(r"\d+_(weeks|months)$")` with `match` (start-anchored):
/// ASCII digits, then exactly `_weeks` or `_months`. A digit run that
/// overflows `u64` still matched the pattern in Python, where `int()` then
/// succeeded (big ints) and `timedelta` raised `OverflowError` — so it is
/// [`RelToken::Overflow`], not "no match".
enum RelToken<'a> {
    No,
    Yes(u64, &'a str),
    Overflow,
}

fn relative_token(token: &str) -> RelToken<'_> {
    let (digits, term) = if let Some(digits) = token.strip_suffix("_weeks") {
        (digits, "weeks")
    } else if let Some(digits) = token.strip_suffix("_months") {
        (digits, "months")
    } else {
        return RelToken::No;
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return RelToken::No;
    }
    match digits.parse::<u64>() {
        Ok(duration) => RelToken::Yes(duration, term),
        Err(_) => RelToken::Overflow,
    }
}

/// `string_date_filter`: relative magnitudes resolve against `today`.
/// Months are exactly 30 days, as in Python.
fn string_date_filter(
    out: &mut IssueFilter,
    date_term: &str,
    duration: u64,
    subsequent: &str,
    term: &str,
    offset: &str,
    today: NaiveDate,
) -> Result<(), IssueFilterError> {
    let days = match term {
        "months" => (duration as i128).saturating_mul(30),
        _ => (duration as i128).saturating_mul(7),
    };
    let delta = chrono::TimeDelta::days(days.clamp(0, i64::MAX as i128) as i64);
    let day = if offset == "fromnow" {
        today.checked_add_signed(delta)
    } else {
        today.checked_sub_signed(delta)
    }
    .ok_or(IssueFilterError::DateOverflow)?;
    let key = if subsequent == "after" {
        format!("{date_term}__gte")
    } else {
        format!("{date_term}__lte")
    };
    out.set(key, FilterValue::Day(day));
    Ok(())
}

/// `date_filter`: the `;`-separated mini-language over one date term.
fn date_filter(
    out: &mut IssueFilter,
    date_term: &str,
    queries: &[String],
    today: NaiveDate,
) -> Result<(), IssueFilterError> {
    for query in queries {
        let parts: Vec<&str> = query.split(';').collect();
        if parts.len() >= 2 {
            match relative_token(parts[0]) {
                RelToken::Yes(duration, term) => {
                    // Three parts carry the offset; two-part relatives
                    // filter nothing (ported bug).
                    if parts.len() == 3 {
                        string_date_filter(
                            out, date_term, duration, parts[1], term, parts[2], today,
                        )?;
                    }
                }
                // The pattern matched but the magnitude overflows: Python's
                // `int()` succeeds and `timedelta` raises `OverflowError`.
                // (Two-part relatives match the pattern too but never reach
                // the offset math, so they still filter nothing.)
                RelToken::Overflow => {
                    if parts.len() == 3 {
                        return Err(IssueFilterError::DateOverflow);
                    }
                }
                RelToken::No => {
                    if parts.contains(&"after") {
                        out.set(
                            format!("{date_term}__gte"),
                            FilterValue::Text(parts[0].to_owned()),
                        );
                    } else {
                        out.set(
                            format!("{date_term}__lte"),
                            FilterValue::Text(parts[0].to_owned()),
                        );
                    }
                }
            }
        } else {
            out.set(
                format!("{date_term}__contains"),
                FilterValue::Text(parts[0].to_owned()),
            );
        }
    }
    Ok(())
}

// -- shared param shapes ----------------------------------------------------

/// GET id-list params: drop `"null"` tokens, record a `__isnull` when
/// `"None"` is present, keep valid UUIDs (the `""` veto is dead here —
/// parsed UUIDs never equal `""` — so any non-empty list applies).
fn get_uuid_param(
    out: &mut IssueFilter,
    params: &HashMap<String, String>,
    key: &str,
    field: &str,
    none_field: Option<&str>,
    guard: Option<&str>,
) {
    let raw = match params.get(key) {
        Some(raw) => raw,
        None => return,
    };
    let tokens: Vec<String> = raw
        .split(',')
        .filter(|item| *item != "null")
        .map(str::to_owned)
        .collect();
    if let Some(none_key) = none_field {
        if tokens.iter().any(|item| item == "None") {
            out.set(none_key.to_owned(), FilterValue::Flag(true));
        }
    }
    let ids = filter_valid_uuids(&tokens);
    if !ids.is_empty() {
        out.set(field.to_owned(), FilterValue::Uuids(ids));
    }
    if let Some(guard_key) = guard {
        out.set(guard_key.to_owned(), FilterValue::Flag(true));
    }
}

/// GET string-list params: drop `"null"` tokens, apply only when non-empty
/// and free of `""` (a trailing comma vetoes the whole filter).
fn get_string_param(
    out: &mut IssueFilter,
    params: &HashMap<String, String>,
    key: &str,
    field: &str,
) {
    let raw = match params.get(key) {
        Some(raw) => raw,
        None => return,
    };
    let tokens: Vec<&str> = raw.split(',').filter(|item| *item != "null").collect();
    if !tokens.is_empty() && !tokens.contains(&"") {
        out.set(
            field.to_owned(),
            FilterValue::Strings(tokens.iter().map(|item| item.to_string()).collect()),
        );
    }
}

/// POST id-list params: store the raw list when non-empty and not `"null"`.
/// (A list never equals the string `"null"`, so any non-empty list applies.)
fn post_raw_param(
    out: &mut IssueFilter,
    params: &HashMap<String, PostVal>,
    key: &str,
    field: &str,
) {
    match params.get(key) {
        Some(PostVal::List(items)) if !items.is_empty() => {
            out.set(field.to_owned(), FilterValue::Strings(items.clone()));
        }
        Some(PostVal::Text(text)) if !text.is_empty() && text != "null" => {
            out.set(field.to_owned(), FilterValue::Text(text.clone()));
        }
        _ => {}
    }
}

/// GET date params: split on `,` and run the mini-language when non-empty
/// and free of `""`.
fn get_date_param(
    out: &mut IssueFilter,
    params: &HashMap<String, String>,
    key: &str,
    date_term: &str,
    today: NaiveDate,
) -> Result<(), IssueFilterError> {
    let raw = match params.get(key) {
        Some(raw) => raw,
        None => return Ok(()),
    };
    let queries: Vec<String> = raw.split(',').map(str::to_owned).collect();
    if !queries.is_empty() && !queries.iter().any(String::is_empty) {
        date_filter(out, date_term, &queries, today)?;
    }
    Ok(())
}

// -- the 25 filter functions --------------------------------------------------

fn filter_state_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "state",
        &format!("{prefix}state__in"),
        None,
        None,
    );
}

fn filter_parent_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "parent",
        &format!("{prefix}parent__in"),
        Some(&format!("{prefix}parent__isnull")),
        None,
    );
}

fn filter_labels_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "labels",
        &format!("{prefix}labels__in"),
        Some(&format!("{prefix}labels__isnull")),
        Some(&format!("{prefix}label_issue__deleted_at__isnull")),
    );
}

fn filter_assignees_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "assignees",
        &format!("{prefix}assignees__in"),
        Some(&format!("{prefix}assignees__isnull")),
        Some(&format!("{prefix}issue_assignee__deleted_at__isnull")),
    );
}

fn filter_mentions_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "mentions",
        &format!("{prefix}issue_mention__mention__id__in"),
        None,
        None,
    );
}

fn filter_created_by_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "created_by",
        &format!("{prefix}created_by__in"),
        Some(&format!("{prefix}created_by__isnull")),
        None,
    );
}

fn filter_logged_by_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "logged_by",
        &format!("{prefix}logged_by__in"),
        Some(&format!("{prefix}logged_by__isnull")),
        None,
    );
}

fn filter_name_value(out: &mut IssueFilter, prefix: &str, value: FilterValue) {
    out.set(format!("{prefix}name__icontains"), value);
}

fn filter_project_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "project",
        &format!("{prefix}project__in"),
        None,
        None,
    );
}

fn filter_cycle_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "cycle",
        &format!("{prefix}issue_cycle__cycle_id__in"),
        Some(&format!("{prefix}issue_cycle__cycle_id__isnull")),
        Some(&format!("{prefix}issue_cycle__deleted_at__isnull")),
    );
}

fn filter_module_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "module",
        &format!("{prefix}issue_module__module_id__in"),
        Some(&format!("{prefix}issue_module__module_id__isnull")),
        Some(&format!("{prefix}issue_module__deleted_at__isnull")),
    );
}

fn filter_subscribed_get(out: &mut IssueFilter, params: &HashMap<String, String>, prefix: &str) {
    get_uuid_param(
        out,
        params,
        "subscriber",
        &format!("{prefix}issue_subscribers__subscriber_id__in"),
        None,
        Some(&format!("{prefix}issue_subscribers__deleted_at__isnull")),
    );
}

/// `filter_issue_state_type`: `backlog` pins to backlog, `active` to the
/// active groups, anything else (including the `"all"` default) takes the
/// full order. Always sets when the `type` key is present.
fn filter_issue_state_type(out: &mut IssueFilter, raw_type: &str, prefix: &str) {
    let groups: Vec<String> = if raw_type == "backlog" {
        vec!["backlog".to_owned()]
    } else if raw_type == "active" {
        crate::filterset::ACTIVE_STATE_GROUPS
            .iter()
            .map(|group| group.to_string())
            .collect()
    } else {
        crate::filterset::STATE_GROUP_ORDER
            .iter()
            .map(|group| group.to_string())
            .collect()
    };
    out.set(
        format!("{prefix}state__group__in"),
        FilterValue::Strings(groups),
    );
}

fn filter_sub_issue_toggle(out: &mut IssueFilter, raw: &str, prefix: &str) {
    if raw == "false" {
        out.set(format!("{prefix}parent__isnull"), FilterValue::Flag(true));
    }
}

fn filter_start_target_date(out: &mut IssueFilter, raw: &str, prefix: &str) {
    if raw == "true" {
        out.set(
            format!("{prefix}target_date__isnull"),
            FilterValue::Flag(false),
        );
        out.set(
            format!("{prefix}start_date__isnull"),
            FilterValue::Flag(false),
        );
    }
}

// -- dispatch -----------------------------------------------------------------

/// The `ISSUE_FILTER` key order. Present keys run in this order.
pub const ISSUE_FILTER_KEYS: &[&str] = &[
    "state",
    "state_group",
    "estimate_point",
    "priority",
    "parent",
    "labels",
    "assignees",
    "mentions",
    "created_by",
    "logged_by",
    "name",
    "created_at",
    "updated_at",
    "start_date",
    "target_date",
    "completed_at",
    "type",
    "project",
    "cycle",
    "module",
    "intake_status",
    "inbox_status",
    "sub_issue",
    "subscriber",
    "start_target_date",
];

/// `issue_filters(query_params, "GET", prefix)` over query-string params.
pub fn issue_filters_get(
    params: &HashMap<String, String>,
    prefix: &str,
    today: NaiveDate,
) -> Result<IssueFilter, IssueFilterError> {
    let mut out = IssueFilter::default();
    for key in ISSUE_FILTER_KEYS {
        if !params.contains_key(*key) {
            continue;
        }
        match *key {
            "state" => filter_state_get(&mut out, params, prefix),
            "state_group" => get_string_param(
                &mut out,
                params,
                "state_group",
                &format!("{prefix}state__group__in"),
            ),
            "estimate_point" => get_string_param(
                &mut out,
                params,
                "estimate_point",
                &format!("{prefix}estimate_point__in"),
            ),
            "priority" => get_string_param(
                &mut out,
                params,
                "priority",
                &format!("{prefix}priority__in"),
            ),
            "parent" => filter_parent_get(&mut out, params, prefix),
            "labels" => filter_labels_get(&mut out, params, prefix),
            "assignees" => filter_assignees_get(&mut out, params, prefix),
            "mentions" => filter_mentions_get(&mut out, params, prefix),
            "created_by" => filter_created_by_get(&mut out, params, prefix),
            "logged_by" => filter_logged_by_get(&mut out, params, prefix),
            "name" => {
                if let Some(name) = params.get("name").filter(|name| !name.is_empty()) {
                    filter_name_value(&mut out, prefix, FilterValue::Text(name.clone()));
                }
            }
            // Ported bug: updated_at writes created_at__date.
            "created_at" => get_date_param(
                &mut out,
                params,
                "created_at",
                &format!("{prefix}created_at__date"),
                today,
            )?,
            "updated_at" => get_date_param(
                &mut out,
                params,
                "updated_at",
                &format!("{prefix}created_at__date"),
                today,
            )?,
            "start_date" => get_date_param(
                &mut out,
                params,
                "start_date",
                &format!("{prefix}start_date"),
                today,
            )?,
            "target_date" => get_date_param(
                &mut out,
                params,
                "target_date",
                &format!("{prefix}target_date"),
                today,
            )?,
            "completed_at" => get_date_param(
                &mut out,
                params,
                "completed_at",
                &format!("{prefix}completed_at__date"),
                today,
            )?,
            "type" => filter_issue_state_type(
                &mut out,
                params.get("type").map_or("all", String::as_str),
                prefix,
            ),
            "project" => filter_project_get(&mut out, params, prefix),
            "cycle" => filter_cycle_get(&mut out, params, prefix),
            "module" => filter_module_get(&mut out, params, prefix),
            "intake_status" => get_string_param(
                &mut out,
                params,
                "intake_status",
                &format!("{prefix}issue_intake__status__in"),
            ),
            "inbox_status" => get_string_param(
                &mut out,
                params,
                "inbox_status",
                &format!("{prefix}issue_intake__status__in"),
            ),
            "sub_issue" => filter_sub_issue_toggle(
                &mut out,
                params.get("sub_issue").map_or("false", String::as_str),
                prefix,
            ),
            "subscriber" => filter_subscribed_get(&mut out, params, prefix),
            "start_target_date" => filter_start_target_date(
                &mut out,
                params
                    .get("start_target_date")
                    .map_or("false", String::as_str),
                prefix,
            ),
            _ => {}
        }
    }
    Ok(out)
}

/// `issue_filters(query_params, "POST", prefix)` over parsed-JSON params.
/// `"PATCH"` takes this same branch.
pub fn issue_filters_post(
    params: &HashMap<String, PostVal>,
    prefix: &str,
    today: NaiveDate,
) -> Result<IssueFilter, IssueFilterError> {
    let mut out = IssueFilter::default();
    for key in ISSUE_FILTER_KEYS {
        if !params.contains_key(*key) {
            continue;
        }
        match *key {
            "state" => post_raw_param(&mut out, params, "state", &format!("{prefix}state__in")),
            "state_group" => post_raw_param(
                &mut out,
                params,
                "state_group",
                &format!("{prefix}state__group__in"),
            ),
            "estimate_point" => post_raw_param(
                &mut out,
                params,
                "estimate_point",
                &format!("{prefix}estimate_point__in"),
            ),
            "priority" => post_raw_param(
                &mut out,
                params,
                "priority",
                &format!("{prefix}priority__in"),
            ),
            "parent" => post_raw_param(&mut out, params, "parent", &format!("{prefix}parent__in")),
            "labels" => {
                post_raw_param(&mut out, params, "labels", &format!("{prefix}labels__in"));
                out.set(
                    format!("{prefix}label_issue__deleted_at__isnull"),
                    FilterValue::Flag(true),
                );
            }
            "assignees" => {
                post_raw_param(
                    &mut out,
                    params,
                    "assignees",
                    &format!("{prefix}assignees__in"),
                );
                out.set(
                    format!("{prefix}issue_assignee__deleted_at__isnull"),
                    FilterValue::Flag(true),
                );
            }
            "mentions" => post_raw_param(
                &mut out,
                params,
                "mentions",
                &format!("{prefix}issue_mention__mention__id__in"),
            ),
            "created_by" => post_raw_param(
                &mut out,
                params,
                "created_by",
                &format!("{prefix}created_by__in"),
            ),
            "logged_by" => post_raw_param(
                &mut out,
                params,
                "logged_by",
                &format!("{prefix}logged_by__in"),
            ),
            "name" => match params.get("name") {
                Some(PostVal::Text(name)) if !name.is_empty() => {
                    filter_name_value(&mut out, prefix, FilterValue::Text(name.clone()));
                }
                Some(PostVal::List(items)) if !items.is_empty() => {
                    filter_name_value(&mut out, prefix, FilterValue::Strings(items.clone()));
                }
                _ => {}
            },
            // Ported bug: updated_at writes created_at__date.
            "created_at" => post_date_param(
                &mut out,
                params,
                "created_at",
                &format!("{prefix}created_at__date"),
                today,
            )?,
            "updated_at" => post_date_param(
                &mut out,
                params,
                "updated_at",
                &format!("{prefix}created_at__date"),
                today,
            )?,
            // POST stores the value raw (`issue_filter[...] =
            // params.get(...)`): no mini-language, even for lists.
            "start_date" => match params.get("start_date") {
                Some(PostVal::List(items)) if !items.is_empty() => {
                    out.set(
                        format!("{prefix}start_date"),
                        FilterValue::Strings(items.clone()),
                    );
                }
                Some(PostVal::Text(text)) if !text.is_empty() => {
                    out.set(
                        format!("{prefix}start_date"),
                        FilterValue::Text(text.clone()),
                    );
                }
                _ => {}
            },
            "target_date" => match params.get("target_date") {
                Some(PostVal::List(items)) if !items.is_empty() => {
                    out.set(
                        format!("{prefix}target_date"),
                        FilterValue::Strings(items.clone()),
                    );
                }
                Some(PostVal::Text(text)) if !text.is_empty() => {
                    out.set(
                        format!("{prefix}target_date"),
                        FilterValue::Text(text.clone()),
                    );
                }
                _ => {}
            },
            "completed_at" => post_date_param(
                &mut out,
                params,
                "completed_at",
                &format!("{prefix}completed_at__date"),
                today,
            )?,
            "type" => {
                let raw = match params.get("type") {
                    Some(PostVal::Text(text)) => text.clone(),
                    _ => "all".to_owned(),
                };
                filter_issue_state_type(&mut out, &raw, prefix);
            }
            "project" => {
                post_raw_param(&mut out, params, "project", &format!("{prefix}project__in"))
            }
            "cycle" => {
                post_raw_param(
                    &mut out,
                    params,
                    "cycle",
                    &format!("{prefix}issue_cycle__cycle_id__in"),
                );
                out.set(
                    format!("{prefix}issue_cycle__deleted_at__isnull"),
                    FilterValue::Flag(true),
                );
            }
            "module" => {
                post_raw_param(
                    &mut out,
                    params,
                    "module",
                    &format!("{prefix}issue_module__module_id__in"),
                );
                out.set(
                    format!("{prefix}issue_module__deleted_at__isnull"),
                    FilterValue::Flag(true),
                );
            }
            // Ported bug: the POST branch gates on `intake_status` but
            // stores `params.get("inbox_status")` — `None` when absent.
            "intake_status" => {
                let gate = match params.get("intake_status") {
                    Some(PostVal::List(items)) => !items.is_empty(),
                    Some(PostVal::Text(text)) => !text.is_empty() && text != "null",
                    None => false,
                };
                if gate {
                    // Stored raw, like Python (`params.get(...)` verbatim).
                    let field = format!("{prefix}issue_intake__status__in");
                    match params.get("inbox_status") {
                        Some(PostVal::List(items)) => {
                            out.set(field, FilterValue::Strings(items.clone()));
                        }
                        Some(PostVal::Text(text)) => {
                            out.set(field, FilterValue::Text(text.clone()));
                        }
                        None => out.set(field, FilterValue::Null),
                    }
                }
            }
            "inbox_status" => post_raw_param(
                &mut out,
                params,
                "inbox_status",
                &format!("{prefix}issue_intake__status__in"),
            ),
            "sub_issue" => {
                let raw = match params.get("sub_issue") {
                    Some(PostVal::Text(text)) => text.clone(),
                    _ => "false".to_owned(),
                };
                filter_sub_issue_toggle(&mut out, &raw, prefix);
            }
            "subscriber" => {
                post_raw_param(
                    &mut out,
                    params,
                    "subscriber",
                    &format!("{prefix}issue_subscribers__subscriber_id__in"),
                );
                out.set(
                    format!("{prefix}issue_subscribers__deleted_at__isnull"),
                    FilterValue::Flag(true),
                );
            }
            "start_target_date" => {
                let raw = match params.get("start_target_date") {
                    Some(PostVal::Text(text)) => text.clone(),
                    _ => "false".to_owned(),
                };
                filter_start_target_date(&mut out, &raw, prefix);
            }
            _ => {}
        }
    }
    Ok(out)
}

/// POST date params: lists run the mini-language. A plain string is iterated
/// character by character (`for query in queries` over a `str`), so every
/// char becomes a `__contains` predicate and the last char wins. Ported as
/// is; listed with the other bugs in the PR.
fn post_date_param(
    out: &mut IssueFilter,
    params: &HashMap<String, PostVal>,
    key: &str,
    date_term: &str,
    today: NaiveDate,
) -> Result<(), IssueFilterError> {
    match params.get(key) {
        Some(PostVal::List(items)) if !items.is_empty() => {
            date_filter(out, date_term, items, today)?;
        }
        Some(PostVal::Text(text)) if !text.is_empty() => {
            let chars: Vec<String> = text.chars().map(|ch| ch.to_string()).collect();
            date_filter(out, date_term, &chars, today)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 26).unwrap()
    }

    fn get_map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn post_map(pairs: &[(&str, PostVal)]) -> HashMap<String, PostVal> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    const UUID_A: &str = "123e4567-e89b-42d3-a456-426614174000";
    const UUID_B: &str = "123e4567-e89b-42d3-a456-426614174001";

    #[test]
    fn dispatcher_runs_present_keys_in_order() {
        let filter = issue_filters_get(
            &get_map(&[("priority", "high"), ("state", UUID_A)]),
            "",
            today(),
        )
        .unwrap();
        let keys: Vec<&str> = filter
            .predicates()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        // ISSUE_FILTER order: state before priority, regardless of input order.
        assert_eq!(keys, vec!["state__in", "priority__in"]);
    }

    #[test]
    fn uuid_params_drop_invalid_and_null_tokens() {
        let filter = issue_filters_get(
            &get_map(&[("state", &format!("{UUID_A},null,bogus,{UUID_B}"))]),
            "",
            today(),
        )
        .unwrap();
        match &filter.get("state__in") {
            Some(FilterValue::Uuids(ids)) => assert_eq!(ids.len(), 2),
            other => panic!("unexpected {other:?}"),
        }
        // All-invalid yields no predicate.
        let filter = issue_filters_get(&get_map(&[("state", "bogus")]), "", today()).unwrap();
        assert!(filter.get("state__in").is_none());
    }

    #[test]
    fn none_tokens_set_isnull_and_guards_are_unconditional() {
        let filter = issue_filters_get(&get_map(&[("labels", "None")]), "", today()).unwrap();
        assert_eq!(filter.get("labels__isnull"), Some(&FilterValue::Flag(true)));
        assert_eq!(
            filter.get("label_issue__deleted_at__isnull"),
            Some(&FilterValue::Flag(true))
        );
        assert!(filter.get("labels__in").is_none());
    }

    #[test]
    fn trailing_comma_vetoes_string_filters_but_not_uuid_filters() {
        let filter = issue_filters_get(&get_map(&[("priority", "high,")]), "", today()).unwrap();
        assert!(filter.get("priority__in").is_none());
        let filter =
            issue_filters_get(&get_map(&[("state", &format!("{UUID_A},"))]), "", today()).unwrap();
        assert!(filter.get("state__in").is_some());
    }

    #[test]
    fn prefix_applies_to_every_key() {
        let filter =
            issue_filters_get(&get_map(&[("labels", "None")]), "issue__", today()).unwrap();
        assert!(filter.get("issue__labels__isnull").is_some());
        assert!(filter
            .get("issue__label_issue__deleted_at__isnull")
            .is_some());
    }

    #[test]
    fn relative_dates_resolve_against_today() {
        // Oracle: 2026-09-26 + 60d = 2026-11-25 (months are 30 days).
        let filter = issue_filters_get(
            &get_map(&[("start_date", "2_months;after;fromnow")]),
            "",
            today(),
        )
        .unwrap();
        assert_eq!(
            filter.get("start_date__gte"),
            Some(&FilterValue::Day(
                NaiveDate::from_ymd_opt(2026, 11, 25).unwrap()
            ))
        );
        // Oracle: 2026-09-26 - 3w = 2026-09-05.
        let filter = issue_filters_get(
            &get_map(&[("target_date", "3_weeks;before;past")]),
            "",
            today(),
        )
        .unwrap();
        assert_eq!(
            filter.get("target_date__lte"),
            Some(&FilterValue::Day(
                NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()
            ))
        );
        // Two-part relatives filter nothing.
        let filter =
            issue_filters_get(&get_map(&[("start_date", "2_weeks;before")]), "", today()).unwrap();
        assert!(filter.is_empty());
    }

    #[test]
    fn explicit_dates_use_after_and_contains_branches() {
        let filter =
            issue_filters_get(&get_map(&[("created_at", "2024-01-01;after")]), "", today())
                .unwrap();
        assert_eq!(
            filter.get("created_at__date__gte"),
            Some(&FilterValue::Text("2024-01-01".to_owned()))
        );
        let filter =
            issue_filters_get(&get_map(&[("created_at", "2024-01-01")]), "", today()).unwrap();
        assert_eq!(
            filter.get("created_at__date__contains"),
            Some(&FilterValue::Text("2024-01-01".to_owned()))
        );
    }

    #[test]
    fn updated_at_writes_created_at_date() {
        let filter =
            issue_filters_get(&get_map(&[("updated_at", "2024-01-01;after")]), "", today())
                .unwrap();
        assert!(filter.get("created_at__date__gte").is_some());
        assert!(filter.get("updated_at__date__gte").is_none());
    }

    #[test]
    fn state_type_mapping() {
        let filter = issue_filters_get(&get_map(&[("type", "backlog")]), "", today()).unwrap();
        assert_eq!(
            filter.get("state__group__in"),
            Some(&FilterValue::Strings(vec!["backlog".to_owned()]))
        );
        let filter = issue_filters_get(&get_map(&[("type", "active")]), "", today()).unwrap();
        match filter.get("state__group__in") {
            Some(FilterValue::Strings(groups)) => {
                assert_eq!(groups, &vec!["unstarted", "started", "review", "test"]);
            }
            other => panic!("unexpected {other:?}"),
        }
        // The "all" default takes the full order.
        let filter = issue_filters_get(&get_map(&[("type", "all")]), "", today()).unwrap();
        match filter.get("state__group__in") {
            Some(FilterValue::Strings(groups)) => assert_eq!(groups.len(), 7),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn toggles_and_name() {
        let filter = issue_filters_get(&get_map(&[("sub_issue", "false")]), "", today()).unwrap();
        assert_eq!(filter.get("parent__isnull"), Some(&FilterValue::Flag(true)));
        let filter = issue_filters_get(&get_map(&[("sub_issue", "true")]), "", today()).unwrap();
        assert!(filter.get("parent__isnull").is_none());

        let filter = issue_filters_get(
            &get_map(&[("start_target_date", "true"), ("name", "login")]),
            "",
            today(),
        )
        .unwrap();
        assert_eq!(
            filter.get("target_date__isnull"),
            Some(&FilterValue::Flag(false))
        );
        assert_eq!(
            filter.get("start_date__isnull"),
            Some(&FilterValue::Flag(false))
        );
        assert_eq!(
            filter.get("name__icontains"),
            Some(&FilterValue::Text("login".to_owned()))
        );
    }

    #[test]
    fn post_stores_raw_lists_and_keeps_guards() {
        let filter = issue_filters_post(
            &post_map(&[
                ("labels", PostVal::List(vec!["a".to_owned()])),
                (
                    "priority",
                    PostVal::List(vec!["high".to_owned(), "urgent".to_owned()]),
                ),
            ]),
            "",
            today(),
        )
        .unwrap();
        assert_eq!(
            filter.get("labels__in"),
            Some(&FilterValue::Strings(vec!["a".to_owned()]))
        );
        assert_eq!(
            filter.get("label_issue__deleted_at__isnull"),
            Some(&FilterValue::Flag(true))
        );
        // POST branch reads inbox_status for intake_status (ported bug):
        // intake present but inbox absent stores None.
        let filter = issue_filters_post(
            &post_map(&[("intake_status", PostVal::List(vec!["s".to_owned()]))]),
            "",
            today(),
        )
        .unwrap();
        assert_eq!(
            filter.get("issue_intake__status__in"),
            Some(&FilterValue::Null)
        );
        let filter = issue_filters_post(
            &post_map(&[("inbox_status", PostVal::List(vec!["s".to_owned()]))]),
            "",
            today(),
        )
        .unwrap();
        assert!(filter.get("issue_intake__status__in").is_some());
    }

    #[test]
    fn post_date_string_iterates_characters() {
        let filter = issue_filters_post(
            &post_map(&[("created_at", PostVal::Text("2024-01-01".to_owned()))]),
            "",
            today(),
        )
        .unwrap();
        assert_eq!(
            filter.get("created_at__date__contains"),
            Some(&FilterValue::Text("1".to_owned()))
        );
    }

    #[test]
    fn post_start_and_target_dates_store_raw_lists() {
        // `filter_start_date` POST: `issue_filter[...] = params.get(...)`
        // verbatim — no mini-language, even for lists.
        let filter = issue_filters_post(
            &post_map(&[(
                "start_date",
                PostVal::List(vec!["2_months;after;fromnow".to_owned()]),
            )]),
            "",
            today(),
        )
        .unwrap();
        assert_eq!(
            filter.get("start_date"),
            Some(&FilterValue::Strings(vec![
                "2_months;after;fromnow".to_owned()
            ]))
        );
        let filter = issue_filters_post(
            &post_map(&[(
                "target_date",
                PostVal::List(vec!["a".to_owned(), "b".to_owned()]),
            )]),
            "",
            today(),
        )
        .unwrap();
        assert_eq!(
            filter.get("target_date"),
            Some(&FilterValue::Strings(vec!["a".to_owned(), "b".to_owned()]))
        );
    }

    #[test]
    fn overflowing_relative_magnitude_is_date_overflow() {
        // The pattern matches, `int()` succeeds (big ints), `timedelta`
        // raises `OverflowError` — but only on the three-part path.
        let filter = issue_filters_get(
            &get_map(&[("start_date", "99999999999999999999999_weeks;after;fromnow")]),
            "",
            today(),
        );
        assert_eq!(filter, Err(IssueFilterError::DateOverflow));
        // Two-part relatives match the pattern but never reach the offset
        // math: still filter nothing, no error.
        let filter = issue_filters_get(
            &get_map(&[("start_date", "99999999999999999999999_weeks;after")]),
            "",
            today(),
        )
        .unwrap();
        assert!(filter.is_empty());
    }

    #[test]
    fn method_from_name_treats_patch_as_post() {
        assert_eq!(Method::from_name("GET"), Method::Get);
        assert_eq!(Method::from_name("POST"), Method::Post);
        assert_eq!(Method::from_name("PATCH"), Method::Post);
    }
}
