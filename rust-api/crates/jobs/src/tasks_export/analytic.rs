//! `analytic_export_task` handler (stage 8, PIDASHCONV-1106).
//!
//! Port of `apps/api/pi_dash/bgtasks/analytic_plot_export.py:349-406`
//! (`analytic_export_task`) plus its helpers `:52-346`. The Celery wire
//! name is [`ANALYTIC_EXPORT_TASK`]; the view's `.delay()` call passes
//! `email`/`data`/`slug` as keywords (`app/views/analytic/base.py:244`).
//!
//! The body is a pure pipeline over shared kernels, never re-implemented
//! here: `issue_filters(data, "POST")` compiles through
//! [`pidash_db::issue_filters::issue_filters_post`]; the `WHERE` fragment
//! compiles through [`filter_sql`] (a jobs-side mirror of the GET
//! handler's `handlers_a::filter_sql` — jobs cannot depend on the api
//! crate — with the POST-only shapes: `Strings` under UUID columns parse
//! or fail, like Django's `UUIDField.get_prep_value`); the plot and
//! detail statements come from the `app_analytics::queries` builders (the
//! same builders the verified GET chart path executes); rows regroup
//! through [`pidash_services::tasks_cleanup::exports`] (
//! [`sort_data_keys`](pidash_services::app_analytics::queries::sort_data_keys),
//! `generate_segmented_rows` / `generate_non_segmented_rows`,
//! [`write_csv_rows`](pidash_services::tasks_cleanup::exports::write_csv_rows));
//! the mail goes through the live
//! [`mail_send`](crate::tasks_mail::mail_send) pipeline with the embedded
//! `emails/exports/analytics.html` template.
//!
//! Ack parity: the bare `@shared_task` swallows every in-body exception
//! (`log_exception` + `return`), so a bound payload always settles to
//! [`Verdict::Ack`]. Only an unbindable payload (`TypeError` in Python)
//! returns `Err`, which the worker settles into requeue-with-budget
//! exactly like the F-09 mechanism does for every handler.
//!
//! Ported bugs (also listed in the PR): BUG-1 (`:278-282`, MODULE
//! segment headers read `label_details`) and BUG-2 (`:217`, the missing
//! `"0"` string default) live in the shared row kernels; the POST
//! `intake_status`/`inbox_status` mix-up (`issue_filters.py:359-364`)
//! lives in the db compiler; the `name=[]` → `icontains` crash and the
//! truthy-non-sized `len()` `TypeError`s are preserved by
//! [`post_params`]; `segment == x_axis` (view-forbidden) keeps the GET
//! path's duplicate-join shape instead of Django's join reuse.

use std::collections::HashMap;
use std::sync::Arc;

use pidash_db::issue_filters::{issue_filters_post, FilterValue, IssueFilter, PostVal};
use pidash_services::app_analytics::queries as q;
use pidash_services::tasks_cleanup::exports as ex;
use pidash_types::tasks_cleanup::exports_dto::{self, CsvCell};
use serde_json::{Map, Value};
use sqlx::PgPool;

use crate::queue::JobRow;
use crate::tasks_mail::mail_send;
use crate::worker::{Handler, Verdict};

/// `analytic_export_task` (`analytic_plot_export.py:349-350`, bare
/// `@shared_task`).
pub use exports_dto::ANALYTIC_EXPORT_TASK_NAME as ANALYTIC_EXPORT_TASK;

/// `send_export_email` subject (`analytic_plot_export.py:54`).
pub use exports_dto::EXPORT_EMAIL_SUBJECT as ANALYTIC_EXPORT_SUBJECT;

/// Bound `analytic_export_task` arguments, in signature order
/// (`analytic_plot_export.py:350`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyticExportCall {
    pub email: String,
    pub data: Value,
    pub slug: String,
}

/// Bind one task argument: the kwarg wins when the key is present (even
/// when null, mirroring Python binding), otherwise the positional. The
/// view's `.delay()` call passes everything as keywords
/// (`app/views/analytic/base.py:244`).
fn bind_arg(args: &[Value], kwargs: &Map<String, Value>, key: &str, pos: usize) -> Option<Value> {
    if let Some(value) = kwargs.get(key) {
        return Some(value.clone());
    }
    args.get(pos).cloned()
}

/// Bind a Celery `(args, kwargs)` payload to [`AnalyticExportCall`].
/// Shape errors are the Python `TypeError` path (unbindable — the worker
/// requeues with budget); every semantic failure binds fine and resolves
/// to a swallowed task error downstream. `data` keeps its raw JSON shape:
/// non-object payloads follow Python's `in`-operator semantics in
/// [`post_params`] instead of failing here.
pub fn bind_analytic_export_call(
    args: &Value,
    kwargs: &Value,
) -> Result<AnalyticExportCall, String> {
    let args = args
        .as_array()
        .ok_or("analytic_export_task: args must be a list")?;
    let kwargs = kwargs
        .as_object()
        .ok_or("analytic_export_task: kwargs must be an object")?;
    let need = |key: &str, pos: usize| {
        bind_arg(args, kwargs, key, pos)
            .ok_or_else(|| format!("analytic_export_task: missing argument '{key}'"))
    };
    let email = need("email", 0)?;
    let data = need("data", 1)?;
    let slug = need("slug", 2)?;
    let email = email
        .as_str()
        .ok_or("analytic_export_task: email must be a string")?
        .to_owned();
    let slug = slug
        .as_str()
        .ok_or("analytic_export_task: slug must be a string")?
        .to_owned();
    Ok(AnalyticExportCall { email, data, slug })
}

/// Python `str()` over a JSON value (`None` → `"None"`, bools
/// capitalized, numbers verbatim, strings as-is). Objects and arrays have
/// no faithful mapping (Python renders single-quoted `repr`); callers
/// only stringify scalars and fail anything else.
fn py_str(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("None".to_owned()),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(text.clone()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// Python truthiness over a JSON value (empty string/list/object, zero,
/// `False` and `None` are falsy).
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            number.as_i64().is_some_and(|n| n != 0)
                || number.as_u64().is_some_and(|n| n != 0)
                || number.as_f64().is_some_and(|n| n != 0.0)
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Convert the task's `data` payload to `issue_filters_post` params.
/// Every POST filter arm reads `params.get(key)` with its own
/// truthiness/`len()` shape, and each is preserved:
/// * `type`, `sub_issue`, `start_target_date` never call `len()`: any
///   value stringifies (scalars) or renders (lists/objects are never
///   equal to the trigger words, matching Python's `==`).
/// * `name` gates on `!= ""`: empty text skips, other text sets, lists
///   set (the compiler fails them, like Django's `icontains` prep);
///   every other present value is the `icontains` crash.
/// * every other filter key gates on truthiness + `len()`: falsy values
///   skip (except `labels`/`assignees`/`cycle`/`module`/`subscriber`,
///   whose through-table guards apply on key presence alone), strings
///   and string lists pass through, dicts iterate their keys (what
///   Django's `__in` does), and truthy non-sized scalars are the `len()`
///   `TypeError`.
/// * a non-object `data` follows the `in` operator: lists/strings that
///   contain no filter key filter nothing; numbers/bools/`None` (and any
///   container naming a filter key, whose `.get` then raises) fail.
pub fn post_params(data: &Value) -> Result<HashMap<String, PostVal>, String> {
    const GUARDED: &[&str] = &["labels", "assignees", "cycle", "module", "subscriber"];
    const STRINGIFIED: &[&str] = &["type", "sub_issue", "start_target_date"];
    let fail = |detail: &str| format!("analytic_export_task: bad filter params ({detail})");
    let entries: Vec<(&str, &Value)> = match data {
        Value::Object(map) => map.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        Value::Array(items) => {
            let names: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
            if pidash_db::issue_filters::ISSUE_FILTER_KEYS
                .iter()
                .any(|key| names.contains(key))
            {
                return Err(fail("list data names a filter key"));
            }
            return Ok(HashMap::new());
        }
        Value::String(text) => {
            if pidash_db::issue_filters::ISSUE_FILTER_KEYS
                .iter()
                .any(|key| text.contains(key))
            {
                return Err(fail("text data names a filter key"));
            }
            return Ok(HashMap::new());
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {
            return Err(fail("data is not a mapping"));
        }
    };
    let mut params: HashMap<String, PostVal> = HashMap::new();
    for (key, value) in entries {
        if !pidash_db::issue_filters::ISSUE_FILTER_KEYS.contains(&key) {
            continue;
        }
        if STRINGIFIED.contains(&key) {
            // Never `len()`d: scalars stringify; lists/objects render to
            // a word that never equals the trigger (`"false"`/`"true"`).
            let rendered = py_str(value).unwrap_or_else(|| {
                if value.is_array() {
                    "[]".to_owned()
                } else {
                    "{}".to_owned()
                }
            });
            params.insert(key.to_owned(), PostVal::Text(rendered));
            continue;
        }
        if key == "name" {
            match value {
                Value::String(text) if text.is_empty() => {}
                Value::String(text) => {
                    params.insert(key.to_owned(), PostVal::Text(text.clone()));
                }
                Value::Array(items) if !items.is_empty() => {
                    let mut rendered = Vec::with_capacity(items.len());
                    for item in items {
                        rendered.push(py_str(item).ok_or_else(|| fail("name element"))?);
                    }
                    params.insert(key.to_owned(), PostVal::List(rendered));
                }
                _ => return Err(fail("name is not text")),
            }
            continue;
        }
        match value {
            Value::String(text) => {
                params.insert(key.to_owned(), PostVal::Text(text.clone()));
            }
            Value::Array(items) => {
                let mut rendered = Vec::with_capacity(items.len());
                for item in items {
                    rendered.push(py_str(item).ok_or_else(|| fail("nested element"))?);
                }
                if !rendered.is_empty() || GUARDED.contains(&key) {
                    params.insert(key.to_owned(), PostVal::List(rendered));
                }
            }
            Value::Object(map) => {
                if map.is_empty() {
                    if GUARDED.contains(&key) {
                        params.insert(key.to_owned(), PostVal::List(Vec::new()));
                    }
                    continue;
                }
                // Django's `__in` iterates the dict keys; the exact
                // lookups (`start_date`/`target_date`) die on a dict.
                if key == "start_date" || key == "target_date" {
                    return Err(fail("date dict"));
                }
                let keys: Vec<String> = map.keys().cloned().collect();
                params.insert(key.to_owned(), PostVal::List(keys));
            }
            scalar => {
                if !py_truthy(scalar) {
                    if GUARDED.contains(&key) {
                        params.insert(key.to_owned(), PostVal::List(Vec::new()));
                    }
                    continue;
                }
                return Err(fail("truthy non-sized value"));
            }
        }
    }
    Ok(params)
}

/// A bound value in filter-SQL position. Mirrors the GET chart
/// handler's `BindVal` (same three shapes bind every predicate).
#[derive(Debug, Clone)]
pub enum BindVal {
    Text(String),
    Uid(uuid::Uuid),
    Int(i32),
}

/// Sequential bind collector. Placeholders start at `$2` (`$1` is the
/// workspace slug in every Q-01 builder); each compiled statement binds
/// slug first, then these values in order.
#[derive(Debug, Default)]
pub struct Binds {
    values: Vec<BindVal>,
}

impl Binds {
    pub fn placeholder(&mut self, value: BindVal) -> String {
        self.values.push(value);
        format!("${}", self.values.len() + 1)
    }

    pub fn values(&self) -> &[BindVal] {
        &self.values
    }
}

/// A relation join the scope may need: handler-local alias plus SQL table.
const RELATION_JOINS: &[(&str, &str)] = &[
    ("label_issue", "issue_labels"),
    ("issue_assignee", "issue_assignees"),
    ("issue_cycle", "cycle_issues"),
    ("issue_module", "module_issues"),
    ("issue_mention", "issue_mentions"),
    ("issue_subscribers", "issue_subscribers"),
    ("issue_intake", "intake_issues"),
];

/// True when every `"alias".` reference in the filter SQL is an
/// `IS NULL` / `IS NOT NULL` test (the `__isnull` predicates): Django
/// joins those relations `LEFT OUTER`, value predicates `INNER`.
fn alias_nullable_only(where_sql: &str, alias: &str) -> bool {
    let marker = format!("\"{alias}\".");
    let mut rest = where_sql;
    while let Some(start) = rest.find(&marker) {
        let after = &rest[start + marker.len()..];
        let ident_len = after
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '"')
            .map(|ch| ch.len_utf8())
            .sum::<usize>();
        let tail = after[ident_len..].trim_start();
        if !(tail.starts_with("IS NULL") || tail.starts_with("IS NOT NULL")) {
            return false;
        }
        rest = after;
    }
    true
}

/// Join legs for the relation aliases a filter fragment references.
/// `INNER` when a value predicate touches the alias, else `LEFT OUTER`
/// (Django's `filter()` vs `__isnull` join). Through-table FK attnames are
/// `issue_id` on every leg (Django `<model>_id` for the `issue` FK).
pub fn relation_joins(fragment: &str) -> String {
    let mut out = String::new();
    for (alias, table) in RELATION_JOINS {
        if !fragment.contains(&format!("\"{alias}\".")) {
            continue;
        }
        let join = if alias_nullable_only(fragment, alias) {
            "LEFT OUTER JOIN"
        } else {
            "INNER JOIN"
        };
        out.push_str(&format!(
            " {join} \"{table}\" \"{alias}\" ON (\"issues\".\"id\" = \"{alias}\".\"issue_id\")"
        ));
    }
    out
}

/// Columns for `__isnull` predicates (and bare `Null` values).
fn isnull_column(path: &str) -> Option<&'static str> {
    Some(match path {
        "parent" => "\"issues\".\"parent_id\"",
        "labels" => "\"label_issue\".\"label_id\"",
        "assignees" => "\"issue_assignee\".\"assignee_id\"",
        "created_by" => "\"issues\".\"created_by_id\"",
        "issue_cycle__cycle_id" => "\"issue_cycle\".\"cycle_id\"",
        "issue_module__module_id" => "\"issue_module\".\"module_id\"",
        "label_issue__deleted_at" => "\"label_issue\".\"deleted_at\"",
        "issue_assignee__deleted_at" => "\"issue_assignee\".\"deleted_at\"",
        "issue_cycle__deleted_at" => "\"issue_cycle\".\"deleted_at\"",
        "issue_module__deleted_at" => "\"issue_module\".\"deleted_at\"",
        "issue_subscribers__deleted_at" => "\"issue_subscribers\".\"deleted_at\"",
        "target_date" => "\"issues\".\"target_date\"",
        "start_date" => "\"issues\".\"start_date\"",
        _ => return None,
    })
}

/// Columns for UUID `__in` predicates. `logged_by` has no model field:
/// Django raises `FieldError` (the task dies, like the GET 500).
fn uuid_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "state__in" => "\"issues\".\"state_id\"",
        "parent__in" => "\"issues\".\"parent_id\"",
        "labels__in" => "\"label_issue\".\"label_id\"",
        "assignees__in" => "\"issue_assignee\".\"assignee_id\"",
        "issue_mention__mention__id__in" => "\"issue_mention\".\"mention_id\"",
        "created_by__in" => "\"issues\".\"created_by_id\"",
        "project__in" => "\"issues\".\"project_id\"",
        "issue_cycle__cycle_id__in" => "\"issue_cycle\".\"cycle_id\"",
        "issue_module__module_id__in" => "\"issue_module\".\"module_id\"",
        "issue_subscribers__subscriber_id__in" => "\"issue_subscribers\".\"subscriber_id\"",
        _ => return None,
    })
}

/// Columns for string `__in` predicates.
fn strings_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "state__group__in" => "\"states\".\"group\"",
        "estimate_point__in" => "\"issues\".\"estimate_point_id\"",
        "priority__in" => "\"issues\".\"priority\"",
        "issue_intake__status__in" => "\"issue_intake\".\"status\"",
        _ => return None,
    })
}

/// `(column, operator)` for `Day` predicates (`__gte` / `__lte`).
fn day_comparison(name: &str) -> Option<(&'static str, &'static str)> {
    let (term, operator) = name.rsplit_once("__")?;
    let column = match term {
        "created_at__date" => "\"issues\".\"created_at\"::date",
        "updated_at__date" => "\"issues\".\"updated_at\"::date",
        "completed_at__date" => "\"issues\".\"completed_at\"::date",
        "start_date" => "\"issues\".\"start_date\"",
        "target_date" => "\"issues\".\"target_date\"",
        _ => return None,
    };
    let operator = match operator {
        "gte" => ">=",
        "lte" => "<=",
        _ => return None,
    };
    Some((column, operator))
}

/// Parse a datetime param the way Django's `DateTimeField.get_prep_value`
/// does (naive values attach UTC). Garbage is a `ValidationError`.
fn parse_datetime_param(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(aware) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(aware.with_timezone(&chrono::Utc));
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M",
        "%m/%d/%y %H:%M:%S",
        "%m/%d/%y %H:%M",
        "%Y-%m-%d",
        "%m/%d/%Y",
        "%m/%d/%y",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return Some(naive.and_utc());
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(text, format) {
            return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
        }
    }
    None
}

/// Text predicates: `name__icontains`, bare `start_date`/`target_date`
/// exact lookups, explicit date bounds (`__gte`/`__lte` on a `__date`
/// term or plain date term), and the single-value `__contains` form.
fn filter_text_sql(binds: &mut Binds, name: &str, text: &str) -> Result<String, String> {
    if name == "name__icontains" {
        // Django `icontains`: LIKE with `\`, `%`, `_` escaped.
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let holder = binds.placeholder(BindVal::Text(format!("%{escaped}%")));
        return Ok(format!("\"issues\".\"name\" ILIKE {holder}"));
    }
    // Bare date terms are exact lookups (`filter(start_date="...")`).
    if name == "start_date" || name == "target_date" {
        let column = if name == "start_date" {
            "\"issues\".\"start_date\""
        } else {
            "\"issues\".\"target_date\""
        };
        if text.parse::<chrono::NaiveDate>().is_err() && parse_datetime_param(text).is_none() {
            return Err("analytic_export_task: bad date filter".to_owned());
        }
        let holder = binds.placeholder(BindVal::Text(text.to_owned()));
        return Ok(format!("{column} = {holder}::date"));
    }
    let Some((term, operator)) = name.rsplit_once("__") else {
        return Err("analytic_export_task: bad text filter".to_owned());
    };
    let operator = match operator {
        "gte" => ">=",
        "lte" => "<=",
        "contains" => "=",
        _ => return Err("analytic_export_task: bad text filter".to_owned()),
    };
    let column = match term {
        "created_at__date" => "\"issues\".\"created_at\"::date",
        "updated_at__date" => "\"issues\".\"updated_at\"::date",
        "completed_at__date" => "\"issues\".\"completed_at\"::date",
        "start_date" => "\"issues\".\"start_date\"",
        "target_date" => "\"issues\".\"target_date\"",
        _ => return Err("analytic_export_task: bad text filter".to_owned()),
    };
    // Explicit bounds must parse as dates: Django's `get_prep_value`
    // raises `ValidationError` on garbage.
    if text.parse::<chrono::NaiveDate>().is_err() && parse_datetime_param(text).is_none() {
        return Err("analytic_export_task: bad date filter".to_owned());
    }
    if operator == "=" {
        // Single-value form on a date term: Django's `contains`
        // lookup, i.e. LIKE with metacharacters escaped.
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let holder = binds.placeholder(BindVal::Text(format!("%{escaped}%")));
        return Ok(format!("{column}::text LIKE {holder}"));
    }
    let holder = binds.placeholder(BindVal::Text(text.to_owned()));
    Ok(format!("{column} {operator} {holder}::date"))
}

/// Compile one `issue_filters(data, "POST")` predicate to SQL text
/// (placeholders allocated from `binds`). Mirrors the GET chart
/// handler's `filter_sql` predicate-for-predicate, with the POST-only
/// shape: the POST compiler stores raw strings, so `Strings` under a
/// UUID column parse-or-fail exactly like Django's
/// `UUIDField.get_prep_value` (garbage is a `ValidationError`, the task
/// dies). Unknown names are a `FieldError` in Python — an error here.
pub fn filter_sql(binds: &mut Binds, name: &str, value: &FilterValue) -> Result<String, String> {
    let bad = || "analytic_export_task: bad filter".to_owned();
    if let Some(path) = name.strip_suffix("__isnull") {
        let column = isnull_column(path).ok_or_else(bad)?;
        let flag = match value {
            FilterValue::Flag(flag) => *flag,
            _ => return Err(bad()),
        };
        return Ok(if flag {
            format!("{column} IS NULL")
        } else {
            format!("{column} IS NOT NULL")
        });
    }
    match value {
        FilterValue::Uuids(ids) => {
            let column = uuid_in_column(name).ok_or_else(bad)?;
            if ids.is_empty() {
                return Ok("FALSE".to_owned());
            }
            let mut holders = Vec::with_capacity(ids.len());
            for id in ids {
                holders.push(binds.placeholder(BindVal::Uid(*id)));
            }
            Ok(format!("{column} IN ({})", holders.join(",")))
        }
        FilterValue::Strings(items) => {
            // POST stores raw strings under UUID columns too: parse-or-fail.
            if let Some(column) = uuid_in_column(name) {
                if items.is_empty() {
                    return Ok("FALSE".to_owned());
                }
                let mut holders = Vec::with_capacity(items.len());
                for item in items {
                    let id: uuid::Uuid = item.parse().map_err(|_| bad())?;
                    holders.push(binds.placeholder(BindVal::Uid(id)));
                }
                return Ok(format!("{column} IN ({})", holders.join(",")));
            }
            let column = strings_in_column(name).ok_or_else(bad)?;
            if items.is_empty() {
                return Ok("FALSE".to_owned());
            }
            if name == "issue_intake__status__in" {
                let mut holders = Vec::with_capacity(items.len());
                for item in items {
                    let number: i32 = item.parse().map_err(|_| bad())?;
                    holders.push(binds.placeholder(BindVal::Int(number)));
                }
                return Ok(format!("{column} IN ({})", holders.join(",")));
            }
            if name == "estimate_point__in" {
                // UUID PKs compared to strings: Django coerces via
                // `UUIDField.get_prep_value`; garbage is a ValidationError.
                let mut holders = Vec::with_capacity(items.len());
                for item in items {
                    let id: uuid::Uuid = item.parse().map_err(|_| bad())?;
                    holders.push(binds.placeholder(BindVal::Uid(id)));
                }
                return Ok(format!("{column} IN ({})", holders.join(",")));
            }
            let mut holders = Vec::with_capacity(items.len());
            for item in items {
                holders.push(binds.placeholder(BindVal::Text(item.clone())));
            }
            Ok(format!("{column} IN ({})", holders.join(",")))
        }
        FilterValue::Text(text) => filter_text_sql(binds, name, text),
        FilterValue::Flag(_) => Err(bad()),
        FilterValue::Day(day) => {
            let (column, operator) = day_comparison(name).ok_or_else(bad)?;
            let holder = binds.placeholder(BindVal::Text(day.to_string()));
            Ok(format!("{column} {operator} {holder}::date"))
        }
        FilterValue::Null => {
            let column = isnull_column(name).ok_or_else(bad)?;
            Ok(format!("{column} IS NULL"))
        }
    }
}

/// Compile the task's `issue_filters(data, "POST")` predicates plus the
/// scope into one `{filters}` fragment and its binds. `manager` selects
/// the `IssueManager` scope (every analytics read) vs the plain
/// soft-delete scope (the label-details bug branch,
/// `analytic_plot_export.py:128-140`, which reads through plain
/// `Issue.objects`).
pub fn scope_and_filters_post(
    params: &HashMap<String, PostVal>,
    manager: bool,
) -> Result<(String, Binds), String> {
    let today = chrono::Utc::now().date_naive();
    let compiled = issue_filters_post(params, "", today)
        .map_err(|error| format!("analytic_export_task: bad date filter ({error})"))?;
    compile_scope(&compiled, manager)
}

/// Compile an already-compiled [`IssueFilter`] plus the scope into one
/// `{filters}` fragment and its binds (the pure half of
/// [`scope_and_filters_post`).
pub fn compile_scope(filter: &IssueFilter, manager: bool) -> Result<(String, Binds), String> {
    let mut binds = Binds::default();
    let mut parts: Vec<String> = Vec::new();
    parts.push(if manager {
        format!("({})", q::ISSUE_OBJECTS_SCOPE)
    } else {
        "(\"issues\".\"deleted_at\" IS NULL)".to_owned()
    });
    for (name, value) in filter.predicates() {
        parts.push(format!("({})", filter_sql(&mut binds, name, value)?));
    }
    Ok((parts.join(" AND "), binds))
}

/// The legs the Q-01 builders do not emit: relation joins for whatever the
/// fragment references, plus the manager scope's states leg (Django's
/// exclude join — always `LEFT OUTER`; NULL-state rows still drop out of
/// the `!=` test, same as Django). Legs the statement already joined are
/// skipped (the state/cycle/module detail builders join their own tables).
pub fn scope_joins(statement: &str, fragment: &str, manager: bool) -> String {
    let mut joins = relation_joins(fragment);
    let wants_states = manager || fragment.contains("\"states\".");
    if wants_states && !statement.contains("JOIN \"states\"") {
        joins.push_str(
            " LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")",
        );
    }
    joins
}

/// Splice scope joins into a builder's statement before the scope's own
/// `WHERE ("workspaces".` clause, which every Q-01 builder carries exactly
/// once (inside the plot subquery where `"issues"` lives, or at top level
/// for the detail lookups).
pub fn scoped_sql(statement: &str, joins: &str) -> String {
    if joins.is_empty() {
        return statement.to_owned();
    }
    match statement.find(" WHERE (\"workspaces\".") {
        Some(index) => {
            let (head, tail) = statement.split_at(index);
            format!("{head}{joins}{tail}")
        }
        None => statement.to_owned(),
    }
}

/// Wrap a plot statement so `dimension`/`segment` decode uniformly as text
/// (`str(dimension)` in the regroup): the builders emit native column
/// types (uuid, numeric, text) that differ per axis.
pub fn wrapped_plot_sql(inner: &str, value_column: &str, segment: Option<&str>) -> String {
    let segment_select = segment
        .filter(|segment| !segment.is_empty())
        .map(|_| ", \"segment\"::text AS \"segment\"")
        .unwrap_or("");
    format!(
        "SELECT \"dimension\"::text AS \"dimension\"{segment_select}, \"{value_column}\" \
         FROM ({inner}) AS wrapped ORDER BY 1{order_segment}",
        order_segment = if segment.filter(|segment| !segment.is_empty()).is_some() {
            ", 2"
        } else {
            ""
        },
    )
}

/// Build the scoped plot statement for one export: the Q-01b/Q-01c
/// builder over the `{filters}` fragment, plus the scope legs. `None`
/// (unknown axis) is the `build_graph_plot` `ValueError` path.
pub fn plot_statement(
    x_axis: &str,
    y_axis: &str,
    segment: Option<&str>,
    fragment: &str,
) -> Result<String, String> {
    let built = if y_axis == "issue_count" {
        q::base_plot_count_sql(x_axis, segment, fragment)
    } else {
        q::base_plot_estimate_sql(x_axis, segment, fragment)
    };
    let statement = built.ok_or_else(|| "analytic_export_task: bad axis".to_owned())?;
    let joins = scope_joins(&statement, fragment, true);
    Ok(scoped_sql(&statement, &joins))
}

/// Build one scoped detail statement (`manager` selects the plain
/// soft-delete scope for the label bug branch).
pub fn detail_statement(axis: &str, fragment: &str, manager: bool) -> Result<String, String> {
    use pidash_types::tasks_cleanup::exports_dto::{
        ASSIGNEE_ID, CYCLE_ID, LABEL_ID, MODULE_ID, STATE_ID,
    };
    let built = if axis == STATE_ID {
        q::base_state_details_sql(fragment)
    } else if axis == LABEL_ID {
        q::base_label_details_sql(fragment)
    } else if axis == ASSIGNEE_ID {
        q::base_assignee_details_sql(fragment)
    } else if axis == CYCLE_ID {
        q::base_cycle_details_sql(fragment)
    } else if axis == MODULE_ID {
        q::base_module_details_sql(fragment)
    } else {
        return Err("analytic_export_task: bad detail axis".to_owned());
    };
    let joins = scope_joins(&built, fragment, manager);
    Ok(scoped_sql(&built, &joins))
}

/// Bind `$1` (workspace slug) plus the filter binds onto a typed fetch.
#[allow(clippy::type_complexity)]
fn bind_all_as<'q, O>(
    query: sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments>,
    slug: &'q str,
    binds: &'q [BindVal],
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, O, sqlx::postgres::PgArguments> {
    let mut query = query.bind(slug);
    for value in binds {
        query = match value {
            BindVal::Text(text) => query.bind(text),
            BindVal::Uid(id) => query.bind(id),
            BindVal::Int(number) => query.bind(number),
        };
    }
    query
}

async fn fetch_count_plot(
    pool: &PgPool,
    slug: &str,
    sql: &str,
    binds: &Binds,
    segment: Option<&str>,
) -> Result<Vec<(Option<String>, Option<String>, i64)>, String> {
    let wrapped = wrapped_plot_sql(sql, "count", segment);
    if segment.filter(|segment| !segment.is_empty()).is_some() {
        let rows: Vec<(Option<String>, Option<String>, i64)> =
            bind_all_as(sqlx::query_as(&wrapped), slug, binds.values())
                .fetch_all(pool)
                .await
                .map_err(|error| format!("analytic_export_task: plot query failed ({error})"))?;
        Ok(rows)
    } else {
        let rows: Vec<(Option<String>, i64)> =
            bind_all_as(sqlx::query_as(&wrapped), slug, binds.values())
                .fetch_all(pool)
                .await
                .map_err(|error| format!("analytic_export_task: plot query failed ({error})"))?;
        Ok(rows
            .into_iter()
            .map(|(dimension, count)| (dimension, None, count))
            .collect())
    }
}

async fn fetch_estimate_plot(
    pool: &PgPool,
    slug: &str,
    sql: &str,
    binds: &Binds,
    segment: Option<&str>,
) -> Result<Vec<(Option<String>, Option<String>, Option<f64>)>, String> {
    let wrapped = wrapped_plot_sql(sql, "estimate", segment);
    if segment.filter(|segment| !segment.is_empty()).is_some() {
        let rows: Vec<(Option<String>, Option<String>, Option<f64>)> =
            bind_all_as(sqlx::query_as(&wrapped), slug, binds.values())
                .fetch_all(pool)
                .await
                .map_err(|error| format!("analytic_export_task: plot query failed ({error})"))?;
        Ok(rows)
    } else {
        let rows: Vec<(Option<String>, Option<f64>)> =
            bind_all_as(sqlx::query_as(&wrapped), slug, binds.values())
                .fetch_all(pool)
                .await
                .map_err(|error| format!("analytic_export_task: plot query failed ({error})"))?;
        Ok(rows
            .into_iter()
            .map(|(dimension, estimate)| (dimension, None, estimate))
            .collect())
    }
}

/// Python `str()` over an optional detail value: `None` renders
/// `"None"` (the `f"{first} {last}"` interpolations and the `str(id)`
/// comparisons both see it).
fn detail_text(value: Option<String>) -> String {
    value.unwrap_or_else(|| "None".to_owned())
}

fn detail_uuid(value: Option<uuid::Uuid>) -> String {
    value
        .map(|id| id.to_string())
        .unwrap_or_else(|| "None".to_owned())
}

async fn fetch_state_details(
    pool: &PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<HashMap<String, String>>, String> {
    let sql = detail_statement(exports_dto::STATE_ID, fragment, true)?;
    let rows: Vec<(Option<uuid::Uuid>, Option<String>, Option<String>)> =
        bind_all_as(sqlx::query_as(&sql), slug, binds.values())
            .fetch_all(pool)
            .await
            .map_err(|error| format!("analytic_export_task: detail query failed ({error})"))?;
    Ok(rows
        .into_iter()
        .map(|(id, name, color)| {
            HashMap::from([
                ("state_id".to_owned(), detail_uuid(id)),
                ("state__name".to_owned(), detail_text(name)),
                ("state__color".to_owned(), detail_text(color)),
            ])
        })
        .collect())
}

async fn fetch_label_details(
    pool: &PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<HashMap<String, String>>, String> {
    let sql = detail_statement(exports_dto::LABEL_ID, fragment, false)?;
    let rows: Vec<(uuid::Uuid, Option<String>, Option<String>)> =
        bind_all_as(sqlx::query_as(&sql), slug, binds.values())
            .fetch_all(pool)
            .await
            .map_err(|error| format!("analytic_export_task: detail query failed ({error})"))?;
    Ok(rows
        .into_iter()
        .map(|(id, color, name)| {
            HashMap::from([
                ("labels__id".to_owned(), id.to_string()),
                ("labels__color".to_owned(), detail_text(color)),
                ("labels__name".to_owned(), detail_text(name)),
            ])
        })
        .collect())
}

async fn fetch_assignee_details(
    pool: &PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<HashMap<String, String>>, String> {
    let sql = detail_statement(exports_dto::ASSIGNEE_ID, fragment, true)?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        uuid::Uuid,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = bind_all_as(sqlx::query_as(&sql), slug, binds.values())
        .fetch_all(pool)
        .await
        .map_err(|error| format!("analytic_export_task: detail query failed ({error})"))?;
    Ok(rows
        .into_iter()
        .map(|(id, avatar_url, display_name, first_name, last_name)| {
            HashMap::from([
                ("assignees__id".to_owned(), id.to_string()),
                ("assignees__avatar_url".to_owned(), detail_text(avatar_url)),
                (
                    "assignees__display_name".to_owned(),
                    detail_text(display_name),
                ),
                ("assignees__first_name".to_owned(), detail_text(first_name)),
                ("assignees__last_name".to_owned(), detail_text(last_name)),
            ])
        })
        .collect())
}

async fn fetch_cycle_details(
    pool: &PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<HashMap<String, String>>, String> {
    let sql = detail_statement(exports_dto::CYCLE_ID, fragment, true)?;
    let rows: Vec<(uuid::Uuid, Option<String>)> =
        bind_all_as(sqlx::query_as(&sql), slug, binds.values())
            .fetch_all(pool)
            .await
            .map_err(|error| format!("analytic_export_task: detail query failed ({error})"))?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| {
            HashMap::from([
                ("issue_cycle__cycle_id".to_owned(), id.to_string()),
                ("issue_cycle__cycle__name".to_owned(), detail_text(name)),
            ])
        })
        .collect())
}

async fn fetch_module_details(
    pool: &PgPool,
    slug: &str,
    fragment: &str,
    binds: &Binds,
) -> Result<Vec<HashMap<String, String>>, String> {
    let sql = detail_statement(exports_dto::MODULE_ID, fragment, true)?;
    let rows: Vec<(uuid::Uuid, Option<String>)> =
        bind_all_as(sqlx::query_as(&sql), slug, binds.values())
            .fetch_all(pool)
            .await
            .map_err(|error| format!("analytic_export_task: detail query failed ({error})"))?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| {
            HashMap::from([
                ("issue_module__module_id".to_owned(), id.to_string()),
                ("issue_module__module__name".to_owned(), detail_text(name)),
            ])
        })
        .collect())
}

/// Group count-plot rows into the segmented `distribution` shape
/// (`build_graph_plot:117-120` regroup + [`q::sort_data_keys`] order).
/// A NULL dimension decodes as `""` (the builders exclude NULL
/// dimensions, so this is unreachable); a NULL segment decodes as `""`
/// too (Python keeps `None`, which `csv.writer` writes as `""` and
/// which never matches a detail id — `""` behaves identically).
pub fn assemble_segmented_counts(
    rows: &[(Option<String>, Option<String>, i64)],
    temp_axis: &str,
) -> Vec<(String, Vec<ex::SegmentPoint>)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<ex::SegmentPoint>> = HashMap::new();
    for (dimension, segment, count) in rows {
        let key = dimension.clone().unwrap_or_default();
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key.clone());
                Vec::new()
            })
            .push(ex::SegmentPoint {
                segment: segment.clone().unwrap_or_default(),
                value: Some(ex::Numeric::Int(*count)),
            });
    }
    let sorted = q::sort_data_keys(&order, temp_axis);
    sorted
        .into_iter()
        .map(|key| {
            let points = groups.remove(&key).unwrap_or_default();
            (key, points)
        })
        .collect()
}

/// Group estimate-plot rows into the segmented `distribution` shape. An
/// all-NULL `SUM` decodes as `None` (an empty CSV cell, like Python's
/// `x.get(key)` returning `None`).
pub fn assemble_segmented_estimates(
    rows: &[(Option<String>, Option<String>, Option<f64>)],
    temp_axis: &str,
) -> Vec<(String, Vec<ex::SegmentPoint>)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<ex::SegmentPoint>> = HashMap::new();
    for (dimension, segment, estimate) in rows {
        let key = dimension.clone().unwrap_or_default();
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key.clone());
                Vec::new()
            })
            .push(ex::SegmentPoint {
                segment: segment.clone().unwrap_or_default(),
                value: estimate.map(ex::Numeric::Float),
            });
    }
    let sorted = q::sort_data_keys(&order, temp_axis);
    sorted
        .into_iter()
        .map(|key| {
            let points = groups.remove(&key).unwrap_or_default();
            (key, points)
        })
        .collect()
}

/// Group plot rows into the non-segmented `distribution` shape: one
/// point per dimension (the `GROUP BY` emits exactly one row per
/// dimension; the first row wins, mirroring `data[0]`).
pub fn assemble_flat_counts(
    rows: &[(Option<String>, Option<String>, i64)],
    temp_axis: &str,
) -> Vec<(String, ex::FlatPoint)> {
    assemble_flat(
        rows.iter().map(|(dimension, _, count)| {
            (
                dimension.clone().unwrap_or_default(),
                ex::FlatPoint {
                    count: Some(ex::Numeric::Int(*count)),
                    estimate: None,
                },
            )
        }),
        temp_axis,
    )
}

/// Group estimate-plot rows into the non-segmented `distribution` shape.
pub fn assemble_flat_estimates(
    rows: &[(Option<String>, Option<String>, Option<f64>)],
    temp_axis: &str,
) -> Vec<(String, ex::FlatPoint)> {
    assemble_flat(
        rows.iter().map(|(dimension, _, estimate)| {
            (
                dimension.clone().unwrap_or_default(),
                ex::FlatPoint {
                    count: None,
                    estimate: estimate.map(ex::Numeric::Float),
                },
            )
        }),
        temp_axis,
    )
}

fn assemble_flat(
    points: impl Iterator<Item = (String, ex::FlatPoint)>,
    temp_axis: &str,
) -> Vec<(String, ex::FlatPoint)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, ex::FlatPoint> = HashMap::new();
    for (key, point) in points {
        if let std::collections::hash_map::Entry::Vacant(slot) = groups.entry(key.clone()) {
            order.push(key);
            slot.insert(point);
        }
    }
    let sorted = q::sort_data_keys(&order, temp_axis);
    sorted
        .into_iter()
        .map(|key| {
            let point = groups.remove(&key).unwrap_or(ex::FlatPoint {
                count: None,
                estimate: None,
            });
            (key, point)
        })
        .collect()
}

/// Detail slices for the row builders: only the axes in use are
/// populated, the rest are empty (`analytic_export_task:362-372`).
pub struct DetailSlices {
    pub assignee: Vec<HashMap<String, String>>,
    pub label: Vec<HashMap<String, String>>,
    pub state: Vec<HashMap<String, String>>,
    pub cycle: Vec<HashMap<String, String>>,
    pub module: Vec<HashMap<String, String>>,
}

async fn fetch_details(
    pool: &PgPool,
    slug: &str,
    x_axis: &str,
    segment: Option<&str>,
    params: &HashMap<String, PostVal>,
) -> Result<DetailSlices, String> {
    let needs = ex::detail_needs(Some(x_axis), segment);
    let mut slices = DetailSlices {
        assignee: Vec::new(),
        label: Vec::new(),
        state: Vec::new(),
        cycle: Vec::new(),
        module: Vec::new(),
    };
    if needs.assignee || needs.state || needs.cycle || needs.module {
        let (fragment, binds) = scope_and_filters_post(params, true)?;
        if needs.assignee {
            slices.assignee = fetch_assignee_details(pool, slug, &fragment, &binds).await?;
        }
        if needs.state {
            slices.state = fetch_state_details(pool, slug, &fragment, &binds).await?;
        }
        if needs.cycle {
            slices.cycle = fetch_cycle_details(pool, slug, &fragment, &binds).await?;
        }
        if needs.module {
            slices.module = fetch_module_details(pool, slug, &fragment, &binds).await?;
        }
    }
    if needs.label {
        // The label bug branch reads through plain `Issue.objects`
        // (`analytic_plot_export.py:128-140`).
        let (fragment, binds) = scope_and_filters_post(params, false)?;
        slices.label = fetch_label_details(pool, slug, &fragment, &binds).await?;
    }
    Ok(slices)
}

/// Run one bound export: filters → plot → details → CSV → mail. Any
/// error is the swallowed task exception (`log_exception` at the call
/// site); the mail send itself swallows internally and only reports
/// success/failure through the logs.
async fn run_analytic_export(pool: &PgPool, call: &AnalyticExportCall) -> Result<(), String> {
    let params = post_params(&call.data)?;
    let text_or_none = |key: &str| call.data.get(key).and_then(Value::as_str);
    let x_axis = text_or_none("x_axis").filter(|axis| q::is_valid_axis(axis));
    let y_axis = text_or_none("y_axis").filter(|axis| q::VALID_YAXIS.contains(axis));
    let segment = text_or_none("segment").filter(|seg| !seg.is_empty());
    let (Some(x_axis), Some(y_axis)) = (x_axis, y_axis) else {
        return Err("analytic_export_task: bad axes".to_owned());
    };
    if let Some(seg) = segment {
        if !q::is_valid_axis(seg) {
            return Err("analytic_export_task: bad segment".to_owned());
        }
    }
    let (fragment, binds) = scope_and_filters_post(&params, true)?;
    let statement = plot_statement(x_axis, y_axis, segment, &fragment)?;
    let slices = fetch_details(pool, &call.slug, x_axis, segment, &params).await?;
    let csv_rows: Vec<Vec<CsvCell>> = if ex::is_segmented(segment) {
        let seg = segment.unwrap_or_default();
        if y_axis == "issue_count" {
            let rows = fetch_count_plot(pool, &call.slug, &statement, &binds, segment).await?;
            let distribution = assemble_segmented_counts(&rows, x_axis);
            ex::generate_segmented_rows(
                &distribution,
                x_axis,
                y_axis,
                seg,
                &slices.assignee,
                &slices.label,
                &slices.state,
                &slices.cycle,
                &slices.module,
            )
        } else {
            let rows = fetch_estimate_plot(pool, &call.slug, &statement, &binds, segment).await?;
            let distribution = assemble_segmented_estimates(&rows, x_axis);
            ex::generate_segmented_rows(
                &distribution,
                x_axis,
                y_axis,
                seg,
                &slices.assignee,
                &slices.label,
                &slices.state,
                &slices.cycle,
                &slices.module,
            )
        }
    } else if y_axis == "issue_count" {
        let rows = fetch_count_plot(pool, &call.slug, &statement, &binds, None).await?;
        let distribution = assemble_flat_counts(&rows, x_axis);
        ex::generate_non_segmented_rows(
            &distribution,
            x_axis,
            y_axis,
            &slices.assignee,
            &slices.label,
            &slices.state,
            &slices.cycle,
            &slices.module,
        )
    } else {
        let rows = fetch_estimate_plot(pool, &call.slug, &statement, &binds, None).await?;
        let distribution = assemble_flat_estimates(&rows, x_axis);
        ex::generate_non_segmented_rows(
            &distribution,
            x_axis,
            y_axis,
            &slices.assignee,
            &slices.label,
            &slices.state,
            &slices.cycle,
            &slices.module,
        )
    };
    let csv_text = ex::write_csv_rows(&csv_rows);
    let attachment = exports_dto::analytics_attachment_name(&call.slug);
    mail_send::send_mail_with_attachment(
        pool,
        mail_send::ANALYTICS_EXPORT_TEMPLATE,
        ANALYTIC_EXPORT_SUBJECT,
        &HashMap::new(),
        &call.email,
        &attachment,
        &csv_text,
        "Email sent successfully.",
    )
    .await;
    Ok(())
}

/// Handler for [`ANALYTIC_EXPORT_TASK`]:
/// `analytic_export_task(email, data, slug)`.
pub fn analytic_export_handler(pool: PgPool) -> Handler {
    Arc::new(move |job: JobRow| {
        let pool = pool.clone();
        Box::pin(async move {
            let call = match bind_analytic_export_call(&job.args, &job.kwargs) {
                Ok(call) => call,
                Err(error) => return Err(error),
            };
            if let Err(error) = run_analytic_export(&pool, &call).await {
                tracing::error!(target: "pi_dash.exception", "{error}");
            }
            Ok(Verdict::Ack)
        })
    })
}

/// Register [`ANALYTIC_EXPORT_TASK`] on `registry` (bare
/// `@shared_task`: default ack-on-success — the handler always
/// acknowledges, mirroring the swallow-everything body).
pub fn register_analytic_task(registry: &mut crate::worker::Registry, pool: PgPool) {
    registry.register(ANALYTIC_EXPORT_TASK, analytic_export_handler(pool));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const UUID_A: &str = "123e4567-e89b-42d3-a456-426614174000";

    fn call_value() -> (Value, Value) {
        let mut kwargs = Map::new();
        kwargs.insert("email".to_owned(), json!("contract@example.com"));
        kwargs.insert(
            "data".to_owned(),
            json!({"x_axis": "priority", "y_axis": "issue_count"}),
        );
        kwargs.insert("slug".to_owned(), json!("acme"));
        (Value::Array(vec![]), Value::Object(kwargs))
    }

    #[test]
    fn task_name_matches_celery_wire() {
        assert_eq!(
            ANALYTIC_EXPORT_TASK,
            "pi_dash.bgtasks.analytic_plot_export.analytic_export_task"
        );
        assert_eq!(ANALYTIC_EXPORT_SUBJECT, "Your Export is ready");
    }

    #[test]
    fn bind_prefers_kwargs_then_positionals() {
        let (args, kwargs) = call_value();
        let bound = bind_analytic_export_call(&args, &kwargs).expect("binds");
        assert_eq!(bound.email, "contract@example.com");
        assert_eq!(bound.slug, "acme");
        assert_eq!(bound.data["x_axis"], json!("priority"));

        let args = Value::Array(vec![
            json!("a@x.io"),
            json!({"x_axis": "state_id"}),
            json!("s"),
        ]);
        let bound = bind_analytic_export_call(&args, &Value::Object(Map::new())).expect("binds");
        assert_eq!(bound.email, "a@x.io");

        // Missing and mistyped arguments fail to bind (the TypeError path).
        assert!(bind_analytic_export_call(&args, &Value::Null).is_err());
        assert!(bind_analytic_export_call(&Value::Null, &Value::Object(Map::new())).is_err());
        let mut bad = Map::new();
        bad.insert("email".to_owned(), Value::Null);
        assert!(bind_analytic_export_call(&Value::Array(vec![]), &Value::Object(bad)).is_err());
        // `data` keeps any JSON shape (post_params applies `in` semantics).
        let args = Value::Array(vec![json!("a@x.io"), json!([1, 2]), json!("s")]);
        assert!(bind_analytic_export_call(&args, &Value::Object(Map::new())).is_ok());
    }

    #[test]
    fn post_params_converts_realistic_payloads() {
        let data = json!({
            "x_axis": "priority",
            "y_axis": "issue_count",
            "priority": ["high", "low"],
            "state": UUID_A,
            "estimate_point": [],
            "labels": [],
        });
        let params = post_params(&data).expect("converts");
        assert_eq!(
            params.get("priority"),
            Some(&PostVal::List(vec!["high".to_owned(), "low".to_owned()]))
        );
        assert_eq!(params.get("state"), Some(&PostVal::Text(UUID_A.to_owned())));
        // Falsy values skip — except the guarded keys, whose
        // through-table guard applies on presence alone.
        assert!(!params.contains_key("estimate_point"));
        assert_eq!(params.get("labels"), Some(&PostVal::List(vec![])));
        // Non-filter keys never enter the map.
        assert!(!params.contains_key("x_axis"));
    }

    #[test]
    fn post_params_preserves_len_and_none_shapes() {
        // Truthy non-sized scalars are the `len()` TypeError.
        assert!(post_params(&json!({"priority": 5})).is_err());
        assert!(post_params(&json!({"priority": true})).is_err());
        // Falsy scalars skip silently.
        assert!(post_params(&json!({"priority": 0})).expect("ok").is_empty());
        assert!(post_params(&json!({"priority": false}))
            .expect("ok")
            .is_empty());
        assert!(post_params(&json!({"priority": null}))
            .expect("ok")
            .is_empty());
        // Nested elements always die in Django prep.
        assert!(post_params(&json!({"priority": [["high"]]})).is_err());
        // Dicts iterate their keys (`__in`); exact lookups die on a dict.
        let params = post_params(&json!({"priority": {"high": 1}})).expect("ok");
        assert_eq!(
            params.get("priority"),
            Some(&PostVal::List(vec!["high".to_owned()]))
        );
        assert!(post_params(&json!({"start_date": {"d": 1}})).is_err());
        // `name=[]` / `name=None` are the `icontains` crash; `""` skips.
        assert!(post_params(&json!({"name": []})).is_err());
        assert!(post_params(&json!({"name": null})).is_err());
        assert!(post_params(&json!({"name": 5})).is_err());
        assert!(post_params(&json!({"name": ""})).expect("ok").is_empty());
        // `type`/`sub_issue`/`start_target_date` never call `len()`.
        let params = post_params(&json!({"type": 5, "sub_issue": false})).expect("ok");
        assert_eq!(params.get("type"), Some(&PostVal::Text("5".to_owned())));
        assert_eq!(
            params.get("sub_issue"),
            Some(&PostVal::Text("False".to_owned()))
        );
        // Non-object data follows the `in` operator.
        assert!(post_params(&json!([1, 2])).expect("ok").is_empty());
        assert!(post_params(&json!(["priority"])).is_err());
        assert!(post_params(&json!("plain")).expect("ok").is_empty());
        assert!(post_params(&json!("has priority inside")).is_err());
        assert!(post_params(&json!(5)).is_err());
        assert!(post_params(&json!(null)).is_err());
    }

    #[test]
    fn filter_sql_compiles_post_shapes() {
        let mut binds = Binds::default();
        // POST stores raw strings under UUID columns: parse-or-fail.
        let sql = filter_sql(
            &mut binds,
            "state__in",
            &FilterValue::Strings(vec![UUID_A.to_owned()]),
        )
        .expect("compiles");
        assert_eq!(sql, "\"issues\".\"state_id\" IN ($2)");
        assert!(filter_sql(
            &mut Binds::default(),
            "state__in",
            &FilterValue::Strings(vec!["bogus".to_owned()]),
        )
        .is_err());
        // `logged_by` has no model field (Django FieldError).
        assert!(filter_sql(
            &mut Binds::default(),
            "logged_by__in",
            &FilterValue::Strings(vec![UUID_A.to_owned()]),
        )
        .is_err());
        // Intake statuses parse as integers.
        let sql = filter_sql(
            &mut Binds::default(),
            "issue_intake__status__in",
            &FilterValue::Strings(vec!["1".to_owned()]),
        )
        .expect("compiles");
        assert!(sql.contains("\"issue_intake\".\"status\" IN ($2)"));
        // Bare date terms are exact lookups; garbage dies.
        let sql = filter_sql(
            &mut Binds::default(),
            "start_date",
            &FilterValue::Text("2026-01-02".to_owned()),
        )
        .expect("compiles");
        assert_eq!(sql, "\"issues\".\"start_date\" = $2::date");
        assert!(filter_text_sql(&mut Binds::default(), "start_date", "2_weeks;before").is_err());
        // `name` lists die (Django `icontains` prep).
        assert!(filter_sql(
            &mut Binds::default(),
            "name__icontains",
            &FilterValue::Strings(vec!["a".to_owned()]),
        )
        .is_err());
    }

    #[test]
    fn scope_and_joins_mirror_get_path() {
        let params = post_params(&json!({"priority": ["high"]})).expect("ok");
        let (fragment, binds) = scope_and_filters_post(&params, true).expect("compiles");
        assert!(fragment.contains(q::ISSUE_OBJECTS_SCOPE));
        assert!(fragment.contains("\"issues\".\"priority\" IN ($2)"));
        assert_eq!(binds.values().len(), 1);
        // The label branch drops the manager scope (BUG port).
        let (label_fragment, _) = scope_and_filters_post(&params, false).expect("compiles");
        assert!(!label_fragment.contains("triage"));
        assert!(label_fragment.contains("\"issues\".\"deleted_at\" IS NULL"));
        // Scope legs: states for the manager scope, relation legs only
        // for referenced aliases.
        let statement = q::base_plot_count_sql("priority", None, &fragment).expect("built");
        let joins = scope_joins(&statement, &fragment, true);
        assert!(joins.contains("JOIN \"states\""));
        assert!(!joins.contains("issue_labels"));
        let scoped = scoped_sql(&statement, &joins);
        assert!(scoped.contains("JOIN \"states\" ON"));
        // `__isnull`-only aliases join LEFT OUTER.
        let params = post_params(&json!({"labels": []})).expect("ok");
        let (fragment, _) = scope_and_filters_post(&params, true).expect("compiles");
        assert!(fragment.contains("\"label_issue\".\"deleted_at\" IS NULL"));
        let joins = relation_joins(&fragment);
        assert!(joins.contains("LEFT OUTER JOIN \"issue_labels\""));
    }

    #[test]
    fn statements_come_from_shared_builders() {
        let fragment = "(\"issues\".\"deleted_at\" IS NULL)";
        let count = plot_statement("priority", "issue_count", None, fragment).expect("built");
        assert!(count.contains("COUNT(*)"));
        assert!(count.contains("ORDER BY \"dimension\" ASC"));
        let estimate = plot_statement("priority", "estimate", None, fragment).expect("built");
        assert!(estimate.contains("SUM(CAST"));
        let segmented =
            plot_statement("priority", "issue_count", Some("state_id"), fragment).expect("built");
        assert!(segmented.contains("AS \"segment\""));
        assert!(plot_statement("nope", "issue_count", None, fragment).is_err());
        let detail = detail_statement(exports_dto::STATE_ID, fragment, true).expect("built");
        assert!(detail.contains("DISTINCT ON"));
        assert!(detail_statement("nope", fragment, true).is_err());
        let wrapped = wrapped_plot_sql("SELECT 1", "count", Some("state_id"));
        assert!(wrapped.contains("\"dimension\"::text"));
        assert!(wrapped.contains("\"segment\"::text"));
    }

    #[test]
    fn assembly_sorts_and_groups_like_build_graph_plot() {
        // Priority order wins over row order; the sum folds per bucket.
        let rows = vec![
            (Some("low".to_owned()), Some("a".to_owned()), 2),
            (Some("high".to_owned()), Some("a".to_owned()), 1),
            (Some("high".to_owned()), Some("b".to_owned()), 3),
        ];
        let distribution = assemble_segmented_counts(&rows, "priority");
        let keys: Vec<&str> = distribution.iter().map(|(k, _)| k.as_str()).collect();
        // `low` sorts before `high` in plot priority order.
        assert_eq!(keys, vec!["low", "high"]);
        let csv = ex::write_csv_rows(&ex::generate_segmented_rows(
            &distribution,
            "priority",
            "issue_count",
            "state_id",
            &[],
            &[],
            &[],
            &[],
            &[],
        ));
        assert!(csv.starts_with("\"Priority\",\"Issue Count\",\"a\",\"b\"\r\n"));
        assert!(csv.contains("\"low\",\"2\",\"2\",\"0\"\r\n"));
        assert!(csv.contains("\"high\",\"4\",\"1\",\"3\"\r\n"));

        // Flat assembly keeps one point per dimension; NULL estimates
        // render empty.
        let rows = vec![
            (Some("b".to_owned()), None, Some(2.5)),
            (Some("a".to_owned()), None, None),
        ];
        let distribution = assemble_flat_estimates(&rows, "state__group");
        let keys: Vec<&str> = distribution.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["a", "b"]);
        let csv = ex::write_csv_rows(&ex::generate_non_segmented_rows(
            &distribution,
            "state__group",
            "estimate",
            &[],
            &[],
            &[],
            &[],
            &[],
        ));
        assert!(csv.starts_with("\"State Group\",\"Estimate\"\r\n"));
        assert!(csv.contains("\"a\",\"\"\r\n\"b\",\"2.5\"\r\n"));
    }
}
