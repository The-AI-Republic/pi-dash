#![forbid(unsafe_code)]

//! Port of `order_issue_queryset` (`pi_dash/utils/order_queryset.py`).
//!
//! The function returns `(queryset, order_by_param)` where the second
//! element is the *rewritten* param the paginator echoes back as `order_by`.
//! All four branches are copied exactly, including the ported bugs:
//! - priority ordering annotates `priority_order` (a `Case` over
//!   `["urgent", "high", "medium", "low", "none"]`) and always appends
//!   `-created_at` as the tiebreak;
//! - the state branch tests `order_by_param in ["state__name",
//!   "state__group"]`, which is always true for the two values that reach
//!   it, so ascending `state__group` uses the *forward* order and the
//!   `[::-1]` alternative is dead — copied verbatim;
//! - the min-aggregation branch (`labels__name`, `assignees__first_name`,
//!   `issue_module__module__name`, ±) annotates `min_values` and orders by
//!   it with `-created_at` tiebreak;
//! - the default branch orders by the raw param, appending `-created_at`
//!   unless the param already mentions `created_at`.
//!
//! Output here is SQL text for the `ORDER BY` clause plus the rewritten
//! param; the caller splices the fragment into the sea-query statement.

/// Lifecycle-state order shared with the filter kernel
/// (`pi_dash/utils/constants.py STATE_GROUP_ORDER`, triage excluded
/// upstream — the tuple starts at `backlog`).
pub const STATE_ORDER: &[&str] = &[
    "backlog",
    "unstarted",
    "started",
    "review",
    "test",
    "completed",
    "cancelled",
];

/// Priority order (`PRIORITY_ORDER`).
pub const PRIORITY_ORDER: &[&str] = &["urgent", "high", "medium", "low", "none"];

/// Min-aggregation orderable relations and their join targets.
pub const MIN_ORDER_FIELDS: &[(&str, &str)] = &[
    ("labels__name", "labels"),
    ("assignees__first_name", "assignees"),
    ("issue_module__module__name", "modules"),
];

/// The resolved ordering: SQL fragment plus rewritten `order_by` param.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSpec {
    /// `ORDER BY` fragment (without the keywords).
    pub order_by_sql: String,
    /// The rewritten param the paginator echoes as `order_by`.
    pub out_param: String,
}

/// The priority `CASE` expression (`priority_order` annotation).
/// Both signs sort by it ascending — the ported quirk lives in
/// [`order_sql`], this is just the shared expression.
pub fn priority_case_sql() -> String {
    let mut cases = String::new();
    for (index, priority) in PRIORITY_ORDER.iter().enumerate() {
        cases.push_str(&format!("WHEN issue.priority = '{priority}' THEN {index} "));
    }
    format!("CASE {cases}END")
}

/// The state-group `CASE` expression (`state_order` annotation).
/// Always the forward order — the `[::-1]` branch is dead (ported bug).
/// `state_column` is the SQL for the issue's state group.
pub fn state_case_sql(state_column: &str) -> String {
    let mut cases = String::new();
    for (index, group) in STATE_ORDER.iter().enumerate() {
        cases.push_str(&format!("WHEN {state_column} = '{group}' THEN {index} "));
    }
    format!("CASE {cases}ELSE {} END", STATE_ORDER.len())
}

/// Port of `order_issue_queryset(issue_queryset, order_by_param)`.
/// `state_column` is the SQL expression for the issue's state group
/// (e.g. `states."group"` after the state join); `min_column` maps a
/// relation key to its pre-aggregated `MIN(...)` select alias.
pub fn order_sql(
    order_by_param: &str,
    state_column: &str,
    min_column: impl Fn(&str) -> String,
) -> OrderSpec {
    if order_by_param == "priority" || order_by_param == "-priority" {
        let cases = priority_case_sql();
        let descending = order_by_param.starts_with('-');
        return OrderSpec {
            order_by_sql: format!("{cases}, created_at DESC"),
            out_param: if descending {
                "priority_order".to_owned()
            } else {
                "-priority_order".to_owned()
            },
        };
    }
    if order_by_param == "state__group" || order_by_param == "-state__group" {
        // Ported bug: the Python tests `in ["state__name", "state__group"]`,
        // always true here, so the `[::-1]` branch is dead and ascending
        // `state__group` keeps the forward order.
        let descending = order_by_param.starts_with('-');
        let cases = state_case_sql(state_column);
        return OrderSpec {
            order_by_sql: format!("{cases}, created_at DESC"),
            out_param: if descending {
                "-state_order".to_owned()
            } else {
                "state_order".to_owned()
            },
        };
    }
    let stripped = order_by_param.trim_start_matches('-');
    if MIN_ORDER_FIELDS.iter().any(|(name, _)| *name == stripped) {
        let descending = order_by_param.starts_with('-');
        let alias = min_column(stripped);
        return OrderSpec {
            order_by_sql: if descending {
                format!("{alias} DESC, created_at DESC")
            } else {
                format!("{alias}, created_at DESC")
            },
            out_param: if descending {
                "-min_values".to_owned()
            } else {
                "min_values".to_owned()
            },
        };
    }
    // Default branch: `created_at` in the param means no tiebreak append.
    let order_by_sql = if order_by_param.contains("created_at") {
        order_by_param.to_owned()
    } else {
        format!("{order_by_param}, created_at DESC")
    };
    OrderSpec {
        order_by_sql,
        out_param: order_by_param.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(param: &str) -> OrderSpec {
        order_sql(param, r#"states."group""#, |name| {
            format!("min_{}", name.replace("__", "_"))
        })
    }

    #[test]
    fn default_param_orders_by_created_at_desc() {
        let spec = order("-created_at");
        assert_eq!(spec.order_by_sql, "-created_at");
        assert_eq!(spec.out_param, "-created_at");
    }

    #[test]
    fn non_created_field_appends_tiebreak() {
        let spec = order("-updated_at");
        assert_eq!(spec.order_by_sql, "-updated_at, created_at DESC");
        assert_eq!(spec.out_param, "-updated_at");
    }

    #[test]
    fn created_at_substring_skips_tiebreak() {
        // `"created_at" in order_by_param` is a substring test.
        let spec = order("created_at");
        assert_eq!(spec.order_by_sql, "created_at");
    }

    #[test]
    fn priority_builds_case_with_rewritten_param() {
        let spec = order("-priority");
        assert!(spec
            .order_by_sql
            .contains("WHEN issue.priority = 'urgent' THEN 0"));
        assert!(spec.order_by_sql.ends_with("created_at DESC"));
        assert_eq!(spec.out_param, "priority_order");
        let spec = order("priority");
        assert_eq!(spec.out_param, "-priority_order");
    }

    #[test]
    fn priority_sql_is_ascending_for_both_signs_ported_quirk() {
        // Django orders `.order_by("priority_order", "-created_at")` on
        // both branches; only the echoed param differs.
        assert_eq!(
            order("-priority").order_by_sql,
            order("priority").order_by_sql
        );
    }

    #[test]
    fn state_group_ascending_keeps_forward_order_ported_bug() {
        // The `[::-1]` branch is dead: ascending uses STATE_ORDER as-is.
        let spec = order("state__group");
        assert!(spec
            .order_by_sql
            .contains("WHEN states.\"group\" = 'backlog' THEN 0"));
        assert!(spec.order_by_sql.contains("ELSE 7 END"));
        assert_eq!(spec.out_param, "state_order");
    }

    #[test]
    fn min_aggregation_branch_rewrites_param() {
        let spec = order("-assignees__first_name");
        assert_eq!(
            spec.order_by_sql,
            "min_assignees_first_name DESC, created_at DESC"
        );
        assert_eq!(spec.out_param, "-min_values");
    }
}
