//! Intake-list `issue_filters` compiler (D-02, stage 4).
//!
//! Ports `pi_dash/utils/issue_filters.py::issue_filters(query_params,
//! "GET")` (`space/views/intake.py:64`) to SQL text plus bound values.
//! Only the `method="GET"` branches are ported: the intake list always
//! passes `"GET"`, so the `else` branches are dead on this path.
//!
//! Shapes were transcribed from Django's compiled SQL (captured per key
//! against the live ORM; see the workpad): the WHERE conjuncts below are
//! Django's verbatim, with `%s` literals replaced by `$N` binds starting
//! at the caller's `first_param` (`$1..$3` are the list core's intake /
//! workspace / project ids). M2M fan-out is preserved with `INNER JOIN`s
//! (never `EXISTS`, which would de-duplicate rows Django returns twice).
//!
//! Ported quirks (translate, don't redesign):
//!
//! * `updated_at` filters `created_at` (`filter_updated_at` passes
//!   `date_term="created_at__date"`).
//! * `type` scopes `state__group__in` only when the key is present
//!   (`issue_filters.py:462-466`); absent adds nothing, so triage-state
//!   rows stay listed. Present-but-unknown (incl. `"all"`) means the seven
//!   non-triage groups (`STATE_GROUP_ORDER`, triage excluded).
//! * `sub_issue` defaults to `"false"` when absent, so sub-issues are
//!   excluded unless `sub_issue=true`.
//! * `None` (capital N) means SQL NULL; lowercase `"null"` items are
//!   dropped silently; `""` anywhere in a GET list disables that key's
//!   `IN` (but never the `None`-triggered `IS NULL` or the unconditional
//!   through-table deleted guards, which are added first).
//! * Unknown keys are ignored; invalid UUIDs are dropped per key — except
//!   `estimate_point`, where Django's UUID-field validation raises (500),
//!   and `logged_by`, which names no `Issue` field at all (500).
//! * `parent`/`labels` `None`+`IN` combos are unsatisfiable by construction
//!   (`col IN (...) AND col IS NULL`); they are emitted as written.
//! * The redundant parent self-join Django adds for `parent=None,+IN` is
//!   NOT emitted: with the unsatisfiable conjuncts the join cannot change
//!   the (empty) row set.

use std::collections::HashSet;

use chrono::NaiveDate;

use super::{query_last, QueryMap};

/// One bound filter value, in placeholder order.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    Uuid(uuid::Uuid),
    Text(String),
    Int(i32),
}

/// Compiled `issue_filters` output: JOIN fragments for the list FROM
/// clause, the `AND (...)` WHERE tail, and the `$N` values in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compiled {
    pub joins: String,
    pub where_extra: String,
    pub values: Vec<FilterValue>,
}

/// `issue_filters` failure: Django raises (`ValidationError` for bad
/// `estimate_point` UUIDs, `FieldError` for `logged_by`, `ValueError` for
/// bad DSL durations) and `handle_exception` answers the 500 envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterError;

pub const ACTIVE_STATE_GROUPS: &[&str] = &["unstarted", "started", "review", "test"];
pub const ALL_STATE_GROUPS: &[&str] = &[
    "backlog",
    "unstarted",
    "started",
    "review",
    "test",
    "completed",
    "cancelled",
];

/// Compile `issue_filters(query_params, "GET")` (`issue_filters.py:431`).
/// `today` is the request date (`timezone.now().date()`); `first_param`
/// is the first free `$N` (`$1..$3` are the list core's). Handled keys
/// emit in `ISSUE_FILTER` dict order; anything else is ignored.
pub fn compile(
    query: &QueryMap,
    today: NaiveDate,
    first_param: usize,
) -> Result<Compiled, FilterError> {
    let mut out = Compiler {
        today,
        next_param: first_param,
        joins: String::new(),
        conjuncts: Vec::new(),
        values: Vec::new(),
        states_joined: false,
    };
    // ISSUE_FILTER order (issue_filters.py:434-460).
    out.key_state(query)?;
    out.key_state_group(query)?;
    out.key_estimate_point(query)?;
    out.key_priority(query)?;
    out.key_parent(query)?;
    out.key_labels(query)?;
    out.key_assignees(query)?;
    out.key_mentions(query)?;
    out.key_created_by(query)?;
    out.key_logged_by(query)?;
    out.key_name(query);
    out.key_created_at(query)?;
    out.key_updated_at(query)?;
    out.key_start_date(query)?;
    out.key_target_date(query)?;
    out.key_completed_at(query)?;
    out.key_type(query);
    out.key_project(query)?;
    out.key_cycle(query)?;
    out.key_module(query)?;
    out.key_intake_status(query, "intake_status")?;
    out.key_intake_status(query, "inbox_status")?;
    out.key_sub_issue(query);
    out.key_subscriber(query)?;
    out.key_start_target_date(query);
    Ok(Compiled {
        joins: out.joins,
        where_extra: out.conjuncts.join(" AND "),
        values: out.values,
    })
}

struct Compiler {
    today: NaiveDate,
    next_param: usize,
    joins: String,
    conjuncts: Vec<String>,
    values: Vec<FilterValue>,
    states_joined: bool,
}

impl Compiler {
    fn placeholder(&mut self, value: FilterValue) -> String {
        let index = self.next_param;
        self.next_param += 1;
        self.values.push(value);
        format!("${index}")
    }

    fn placeholders(&mut self, values: Vec<FilterValue>) -> String {
        values
            .into_iter()
            .map(|value| self.placeholder(value))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn and(&mut self, conjunct: String) {
        self.conjuncts.push(conjunct);
    }

    fn join_states(&mut self) {
        if !self.states_joined {
            self.states_joined = true;
            self.joins.push_str(
                " INNER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\")",
            );
        }
    }

    /// M2M through join: `INNER` when an `IN` list scopes it (Django join
    /// promotion keeps fan-out duplicates), else `LEFT OUTER` for the
    /// `IS NULL` / deleted-guard probes.
    fn join_through(&mut self, table: &str, column: &str, inner: bool) {
        let kind = if inner { "INNER" } else { "LEFT OUTER" };
        self.joins.push_str(&format!(
            " {kind} JOIN \"{table}\" ON (\"issues\".\"id\" = \"{table}\".\"{column}\")"
        ));
    }

    /// Split a GET list value: drop `"null"` items (never `"None"`).
    fn split_items(raw: &str) -> Vec<&str> {
        raw.split(',').filter(|item| *item != "null").collect()
    }

    /// `filter_valid_uuids` (`issue_filters.py:19-28`): invalid UUIDs are
    /// dropped silently. Returns the survivors in order.
    fn valid_uuids(items: &[&str]) -> Vec<uuid::Uuid> {
        items
            .iter()
            .filter_map(|item| item.parse::<uuid::Uuid>().ok())
            .collect()
    }

    // -- scalar lookups --------------------------------------------------

    fn key_state(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "state") {
            let items = Self::split_items(&raw);
            let ids = Self::valid_uuids(&items);
            if !ids.is_empty() && !items.contains(&"") {
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                self.and(format!("\"issues\".\"state_id\" IN ({list})"));
            }
        }
        Ok(())
    }

    fn key_state_group(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "state_group") {
            let items = Self::split_items(&raw);
            if !items.is_empty() && !items.contains(&"") {
                self.join_states();
                let list = self.placeholders(
                    items
                        .into_iter()
                        .map(|item| FilterValue::Text(item.to_string()))
                        .collect(),
                );
                self.and(format!("\"states\".\"group\" IN ({list})"));
            }
        }
        Ok(())
    }

    fn key_estimate_point(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "estimate_point") {
            let items = Self::split_items(&raw);
            if !items.is_empty() && !items.contains(&"") {
                // No `filter_valid_uuids` here: Django's UUID-field
                // validation raises on the first bad value (500).
                let mut ids = Vec::with_capacity(items.len());
                for item in items {
                    ids.push(FilterValue::Uuid(item.parse().map_err(|_| FilterError)?));
                }
                let list = self.placeholders(ids);
                self.and(format!("\"issues\".\"estimate_point_id\" IN ({list})"));
            }
        }
        Ok(())
    }

    fn key_priority(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "priority") {
            let items = Self::split_items(&raw);
            if !items.is_empty() && !items.contains(&"") {
                let list = self.placeholders(
                    items
                        .into_iter()
                        .map(|item| FilterValue::Text(item.to_string()))
                        .collect(),
                );
                self.and(format!("\"issues\".\"priority\" IN ({list})"));
            }
        }
        Ok(())
    }

    fn key_parent(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "parent") {
            let items = Self::split_items(&raw);
            // The `None` arm is added BEFORE uuid validation; the
            // redundant self-join Django emits alongside is unsatisfiable
            // either way, so only the conjuncts are ported.
            let mut parts = Vec::new();
            let ids = Self::valid_uuids(&items);
            if !ids.is_empty() && !items.contains(&"") {
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                parts.push(format!("\"issues\".\"parent_id\" IN ({list})"));
            }
            if items.contains(&"None") {
                parts.push("\"issues\".\"parent_id\" IS NULL".to_string());
            }
            if !parts.is_empty() {
                self.and(parts.join(" AND "));
            }
        }
        Ok(())
    }

    /// Shared M2M shape (`labels`, `assignees`): optional `None` IS NULL,
    /// optional valid-UUID IN, unconditional through-table deleted guard.
    fn key_m2m(
        &mut self,
        query: &QueryMap,
        key: &str,
        through: &str,
        fk_column: &str,
        guard_alias: &str,
    ) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, key) {
            let items = Self::split_items(&raw);
            let ids = Self::valid_uuids(&items);
            let has_in = !ids.is_empty() && !items.contains(&"");
            let has_none = items.contains(&"None");
            self.join_through(through, "issue_id", has_in);
            let mut parts = Vec::new();
            if has_in {
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                parts.push(format!("\"{through}\".\"{fk_column}\" IN ({list})"));
            }
            if has_none {
                parts.push(format!("\"{through}\".\"{fk_column}\" IS NULL"));
            }
            // Unconditional guard (`label_issue__deleted_at__isnull` /
            // `issue_assignee__deleted_at__isnull`): emitted whenever the
            // key is present, even for empty values.
            parts.push(format!("\"{guard_alias}\".\"deleted_at\" IS NULL"));
            self.and(parts.join(" AND "));
        }
        Ok(())
    }

    fn key_labels(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        // Guard alias is the lookup path's table: the single
        // `issue_labels` join serves both.
        self.key_m2m(query, "labels", "issue_labels", "label_id", "issue_labels")
    }

    fn key_assignees(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        self.key_m2m(
            query,
            "assignees",
            "issue_assignees",
            "assignee_id",
            "issue_assignees",
        )
    }

    fn key_mentions(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "mentions") {
            let items = Self::split_items(&raw);
            let ids = Self::valid_uuids(&items);
            if !ids.is_empty() && !items.contains(&"") {
                self.join_through("issue_mentions", "issue_id", true);
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                self.and(format!("\"issue_mentions\".\"mention_id\" IN ({list})"));
            }
        }
        Ok(())
    }

    fn key_created_by(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "created_by") {
            let items = Self::split_items(&raw);
            let mut parts = Vec::new();
            let ids = Self::valid_uuids(&items);
            if !ids.is_empty() && !items.contains(&"") {
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                parts.push(format!("\"issues\".\"created_by_id\" IN ({list})"));
            }
            if items.contains(&"None") {
                parts.push("\"issues\".\"created_by_id\" IS NULL".to_string());
            }
            if !parts.is_empty() {
                self.and(parts.join(" AND "));
            }
        }
        Ok(())
    }

    fn key_logged_by(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "logged_by") {
            let items = Self::split_items(&raw);
            let ids = Self::valid_uuids(&items);
            let mut parts = Vec::new();
            if !ids.is_empty() && !items.contains(&"") {
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                parts.push(format!("\"issues\".\"logged_by_id\" IN ({list})"));
            }
            if items.contains(&"None") {
                parts.push("\"issues\".\"logged_by_id\" IS NULL".to_string());
            }
            // `logged_by` names no `Issue` field: any emitted conjunct
            // raises at the database (500), exactly like Django's
            // `FieldError`. `"issues"."logged_by_id"` is intentionally a
            // dangling reference.
            if !parts.is_empty() {
                self.and(parts.join(" AND "));
            }
        }
        Ok(())
    }

    fn key_name(&mut self, query: &QueryMap) {
        if let Some(raw) = query_last(query, "name") {
            if !raw.is_empty() {
                // `icontains`: unescaped `%value%`, case-folded both sides.
                let pattern = self.placeholder(FilterValue::Text(format!("%{raw}%")));
                self.and(format!(
                    "UPPER(\"issues\".\"name\"::text) LIKE UPPER({pattern})"
                ));
            }
        }
    }

    // -- date lookups ----------------------------------------------------

    /// One `date_filter` comma-item (`issue_filters.py:53-85`): `;`-split,
    /// `N_weeks`/`N_months` DSL, `after`-sensitive `__gte`/`__lte`, else
    /// `__contains`.
    fn date_item(
        &mut self,
        item: &str,
        date_term: &str,
        is_date_column: bool,
    ) -> Result<(), FilterError> {
        let parts: Vec<&str> = item.split(';').collect();
        if parts.len() >= 2 {
            if is_relative_spec(parts[0]) {
                if parts.len() == 3 {
                    let (digit, term) = parts[0].split_once('_').ok_or(FilterError)?;
                    let duration: i64 = digit.parse().map_err(|_| FilterError)?;
                    let days: i64 = if term == "months" {
                        duration.saturating_mul(30)
                    } else {
                        duration.saturating_mul(7)
                    };
                    // Python overflows to 500 on out-of-range dates
                    // (`OverflowError`); checked arithmetic mirrors it.
                    let shift = |base: NaiveDate, days: i64| {
                        if days >= 0 {
                            base.checked_add_days(chrono::Days::new(days as u64))
                        } else {
                            base.checked_sub_days(chrono::Days::new(days.unsigned_abs()))
                        }
                    };
                    let day = if parts[1] == "after" {
                        if parts[2] == "fromnow" {
                            shift(self.today, days)
                        } else {
                            shift(self.today, -days)
                        }
                    } else if parts[2] == "fromnow" {
                        shift(self.today, days)
                    } else {
                        shift(self.today, -days)
                    }
                    .ok_or(FilterError)?;
                    self.date_compare(date_term, is_date_column, ">=", &day.to_string());
                }
                // len != 3 with a relative head: nothing is added.
                return Ok(());
            }
            if parts.contains(&"after") {
                self.date_compare(date_term, is_date_column, ">=", parts[0]);
            } else {
                self.date_compare(date_term, is_date_column, "<=", parts[0]);
            }
            return Ok(());
        }
        self.date_contains(date_term, is_date_column, item);
        Ok(())
    }

    fn date_compare(&mut self, date_term: &str, is_date_column: bool, op: &str, value: &str) {
        let placeholder = self.placeholder(FilterValue::Text(value.to_string()));
        let column = date_column(date_term, is_date_column);
        self.and(format!("{column} {op} {placeholder}::date"));
    }

    fn date_contains(&mut self, date_term: &str, is_date_column: bool, value: &str) {
        // `__contains` on a date: case-sensitive `LIKE %value%`.
        let pattern = self.placeholder(FilterValue::Text(format!("%{value}%")));
        let column = date_column(date_term, is_date_column);
        self.and(format!("{column}::text LIKE {pattern}"));
    }

    fn date_list(
        &mut self,
        query: &QueryMap,
        key: &str,
        date_term: &str,
        is_date_column: bool,
    ) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, key) {
            let items: Vec<&str> = raw.split(',').collect();
            // No `"null"` filtering on date keys; `""` anywhere disables.
            if !items.is_empty() && !items.contains(&"") {
                for item in items {
                    self.date_item(item, date_term, is_date_column)?;
                }
            }
        }
        Ok(())
    }

    fn key_created_at(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        self.date_list(query, "created_at", "created_at__date", false)
    }

    fn key_updated_at(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        // PORTED BUG: filters `created_at`, not `updated_at`.
        self.date_list(query, "updated_at", "created_at__date", false)
    }

    fn key_start_date(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        self.date_list(query, "start_date", "start_date", true)
    }

    fn key_target_date(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        self.date_list(query, "target_date", "target_date", true)
    }

    fn key_completed_at(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        self.date_list(query, "completed_at", "completed_at__date", false)
    }

    // -- group / relation lookups ----------------------------------------

    fn key_type(&mut self, query: &QueryMap) {
        // Present-only (`issue_filters.py:462-466` runs the filter only
        // `if key in query_params`): an absent `type` adds no constraint,
        // so triage-state rows stay listed. Present (even `""` or unknown)
        // scopes the states join; only `backlog`/`active` narrow the
        // seven non-triage groups (`STATE_GROUP_ORDER`, triage excluded).
        if let Some(raw) = query_last(query, "type") {
            let groups: Vec<FilterValue> = if raw == "backlog" {
                vec![FilterValue::Text("backlog".to_string())]
            } else if raw == "active" {
                ACTIVE_STATE_GROUPS
                    .iter()
                    .map(|group| FilterValue::Text(group.to_string()))
                    .collect()
            } else {
                ALL_STATE_GROUPS
                    .iter()
                    .map(|group| FilterValue::Text(group.to_string()))
                    .collect()
            };
            self.join_states();
            let list = self.placeholders(groups);
            self.and(format!("\"states\".\"group\" IN ({list})"));
        }
    }

    fn key_project(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "project") {
            let items = Self::split_items(&raw);
            let ids = Self::valid_uuids(&items);
            if !ids.is_empty() && !items.contains(&"") {
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                self.and(format!("\"issues\".\"project_id\" IN ({list})"));
            }
        }
        Ok(())
    }

    /// Shared cycle/module shape: optional `None` IS NULL, optional
    /// valid-UUID IN, unconditional through-table deleted guard.
    fn key_cycle_module(
        &mut self,
        query: &QueryMap,
        key: &str,
        through: &str,
        fk_column: &str,
    ) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, key) {
            let items = Self::split_items(&raw);
            let ids = Self::valid_uuids(&items);
            let has_in = !ids.is_empty() && !items.contains(&"");
            let has_none = items.contains(&"None");
            self.join_through(through, "issue_id", has_in);
            let mut parts = Vec::new();
            if has_in {
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                parts.push(format!("\"{through}\".\"{fk_column}\" IN ({list})"));
            }
            if has_none {
                parts.push(format!("\"{through}\".\"{fk_column}\" IS NULL"));
            }
            parts.push(format!("\"{through}\".\"deleted_at\" IS NULL"));
            self.and(parts.join(" AND "));
        }
        Ok(())
    }

    fn key_cycle(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        self.key_cycle_module(query, "cycle", "cycle_issues", "cycle_id")
    }

    fn key_module(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        self.key_cycle_module(query, "module", "module_issues", "module_id")
    }

    fn key_intake_status(&mut self, query: &QueryMap, key: &str) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, key) {
            let items = Self::split_items(&raw);
            if !items.is_empty() && !items.contains(&"") {
                // Values stay strings in Python and coerce server-side;
                // the `$N::integer` cast reproduces the coercion — and its
                // failure (500) — for typed binds.
                let mut ints = Vec::with_capacity(items.len());
                for item in items {
                    let trimmed = item.trim();
                    ints.push(FilterValue::Int(trimmed.parse().map_err(|_| FilterError)?));
                }
                let list = self.placeholders(ints);
                // The list query aliases the intake join `"issue_intake"`
                // (fixture builder); Django's own alias differs, but the
                // predicate is the caller's (`intake_assets.rs` docs).
                self.and(format!("\"issue_intake\".\"status\" IN ({list})"));
            }
        }
        Ok(())
    }

    fn key_sub_issue(&mut self, query: &QueryMap) {
        // GET and POST branches are identical; absent defaults to "false".
        if query_last(query, "sub_issue").as_deref().unwrap_or("false") == "false" {
            self.and("\"issues\".\"parent_id\" IS NULL".to_string());
        }
    }

    fn key_subscriber(&mut self, query: &QueryMap) -> Result<(), FilterError> {
        if let Some(raw) = query_last(query, "subscriber") {
            let items = Self::split_items(&raw);
            let ids = Self::valid_uuids(&items);
            if !ids.is_empty() && !items.contains(&"") {
                self.join_through("issue_subscribers", "issue_id", true);
                let list = self.placeholders(ids.into_iter().map(FilterValue::Uuid).collect());
                self.and(format!(
                    "\"issue_subscribers\".\"deleted_at\" IS NULL AND \"issue_subscribers\".\"subscriber_id\" IN ({list})"
                ));
            } else {
                // Guard-only probe still constrains (deleted guard is
                // unconditional when the key is present).
                self.join_through("issue_subscribers", "issue_id", false);
                self.and("\"issue_subscribers\".\"deleted_at\" IS NULL".to_string());
            }
        }
        Ok(())
    }

    fn key_start_target_date(&mut self, query: &QueryMap) {
        if query_last(query, "start_target_date").as_deref() == Some("true") {
            self.and(
                "\"issues\".\"start_date\" IS NOT NULL AND \"issues\".\"target_date\" IS NOT NULL"
                    .to_string(),
            );
        }
    }
}

/// `AT TIME ZONE UTC` datetime expressions render `::date` for the
/// `__date` terms; plain date columns compare directly.
fn date_column(date_term: &str, is_date_column: bool) -> String {
    let base = match date_term {
        "created_at__date" => "\"issues\".\"created_at\" AT TIME ZONE UTC",
        "completed_at__date" => "\"issues\".\"completed_at\" AT TIME ZONE UTC",
        _ => date_term,
    };
    if is_date_column {
        format!("\"issues\".\"{base}\"")
    } else if date_term.ends_with("__date") {
        format!("({base})::date")
    } else {
        format!("\"issues\".\"{base}\"")
    }
}

/// The `N_weeks`/`N_months` head pattern (`issue_filters.py:14`).
fn is_relative_spec(head: &str) -> bool {
    let (digits, rest) = match head.split_once('_') {
        Some(pair) => pair,
        None => return false,
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    rest == "weeks" || rest == "months"
}

/// Keys Django ignores for empty values (`""` disables the `IN`, and
/// these keys carry no unconditional guard): used by tests.
#[allow(dead_code)]
pub fn tracked_keys() -> HashSet<&'static str> {
    [
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
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::OneOrMany;

    fn query(pairs: &[(&str, &str)]) -> QueryMap {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), OneOrMany::One(value.to_string())))
            .collect()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 28).expect("fixed date")
    }

    fn compile_all(pairs: &[(&str, &str)]) -> Compiled {
        compile(&query(pairs), today(), 4).expect("compiles")
    }

    #[test]
    fn empty_params_filter_only_sub_issues() {
        // No params: sub_issue defaults false (parent IS NULL); the absent
        // `type` key adds no states join and no values, so triage-state
        // rows stay listed (`issue_filters.py:462-466`).
        let compiled = compile_all(&[]);
        assert!(compiled
            .where_extra
            .contains("\"issues\".\"parent_id\" IS NULL"));
        assert!(!compiled.joins.contains("\"states\""));
        assert!(compiled.values.is_empty());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let compiled = compile_all(&[("status", "pending"), ("bogus", "x")]);
        assert!(!compiled.where_extra.contains("pending"));
        assert!(!compiled.where_extra.contains("bogus"));
        assert!(compiled.values.is_empty(), "no defaults emitted");
    }

    #[test]
    fn priority_list_compiles_to_in() {
        let compiled = compile_all(&[("priority", "high,urgent")]);
        assert!(compiled
            .where_extra
            .contains("\"issues\".\"priority\" IN ($4, $5)"));
        assert_eq!(compiled.values[0], FilterValue::Text("high".to_string()));
    }

    #[test]
    fn empty_item_disables_in_but_keeps_guards() {
        // "high," carries "" -> no IN ...
        let compiled = compile_all(&[("priority", "high,")]);
        assert!(!compiled.where_extra.contains("priority\" IN"));
        // ... while the labels guard survives an empty value.
        let compiled = compile_all(&[("labels", "")]);
        assert!(compiled.joins.contains("LEFT OUTER JOIN \"issue_labels\""));
        assert!(compiled
            .where_extra
            .contains("\"issue_labels\".\"deleted_at\" IS NULL"));
        assert!(!compiled.where_extra.contains("label_id\" IN"));
    }

    #[test]
    fn none_triggers_is_null_and_inner_on_in() {
        let compiled = compile_all(&[("labels", "None")]);
        assert!(compiled.joins.contains("LEFT OUTER JOIN \"issue_labels\""));
        assert!(compiled
            .where_extra
            .contains("\"issue_labels\".\"label_id\" IS NULL"));
        let uid = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let compiled = compile_all(&[("labels", &format!("None,{uid}"))]);
        assert!(compiled.joins.contains("INNER JOIN \"issue_labels\""));
        assert!(compiled.where_extra.contains("IS NULL"));
        assert!(compiled.where_extra.contains("IN ($"));
    }

    #[test]
    fn invalid_uuids_drop_silently_except_estimate_point() {
        let compiled = compile_all(&[("state", "not-a-uuid")]);
        assert!(!compiled.where_extra.contains("state_id"));
        assert!(compile(&query(&[("estimate_point", "abc")]), today(), 4).is_err());
    }

    #[test]
    fn logged_by_names_no_column_and_fails() {
        // Any emitted conjunct references the nonexistent column (500 at
        // the database, like Django's FieldError); fully-invalid values
        // drop the key entirely.
        let compiled = compile_all(&[("logged_by", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")]);
        assert!(compiled.where_extra.contains("logged_by_id"));
        let compiled = compile_all(&[("logged_by", "bogus")]);
        assert!(!compiled.where_extra.contains("logged_by"));
    }

    #[test]
    fn intake_status_coerces_to_int() {
        // No `type` key present, so the status binds take $4..$5.
        let compiled = compile_all(&[("intake_status", "-2,1")]);
        assert!(compiled
            .where_extra
            .contains("\"issue_intake\".\"status\" IN ($4, $5)"));
        assert_eq!(compiled.values[0], FilterValue::Int(-2));
        assert_eq!(compiled.values[1], FilterValue::Int(1));
        assert!(compile(&query(&[("intake_status", "abc")]), today(), 4).is_err());
    }

    #[test]
    fn relative_date_dsl_matches_python_arithmetic() {
        // 2_weeks;after;fromnow on 2026-09-28 -> 2026-10-12.
        let compiled = compile_all(&[("start_date", "2_weeks;after;fromnow")]);
        assert!(compiled
            .where_extra
            .contains("\"issues\".\"start_date\" >= $4::date"));
        assert_eq!(
            compiled.values[0],
            FilterValue::Text("2026-10-12".to_string())
        );
        // updated_at filters created_at (ported quirk).
        let compiled = compile_all(&[("updated_at", "2024-02-02")]);
        assert!(compiled
            .where_extra
            .contains("created_at\" AT TIME ZONE UTC)::date::text LIKE"));
        // No-semicolon target_date is a case-sensitive contains.
        let compiled = compile_all(&[("target_date", "2024-05-05")]);
        assert!(compiled
            .where_extra
            .contains("\"issues\".\"target_date\"::text LIKE"));
    }

    #[test]
    fn type_variants_map_groups() {
        let compiled = compile_all(&[("type", "backlog")]);
        assert!(compiled
            .values
            .contains(&FilterValue::Text("backlog".to_string())));
        assert_eq!(compiled.values.len(), 1);
        let compiled = compile_all(&[("type", "active")]);
        assert_eq!(compiled.values.len(), 4);
    }
}
