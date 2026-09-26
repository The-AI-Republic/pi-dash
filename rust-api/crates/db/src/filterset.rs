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
    /// A boolean equality on one column.
    Flag {
        table: &'static str,
        column: &'static str,
    },
    /// A date equality on one column.
    Date {
        table: &'static str,
        column: &'static str,
    },
    /// A two-element date range on one column.
    DateRange {
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
        FilterKind::Date {
            table: "issue",
            column: "created_at"
        }
    ),
    decl!(
        "created_at__exact",
        FilterKind::Date {
            table: "issue",
            column: "created_at"
        },
        alias_of = "created_at"
    ),
    decl!(
        "created_at__range",
        FilterKind::DateRange {
            table: "issue",
            column: "created_at"
        }
    ),
    decl!(
        "updated_at",
        FilterKind::Date {
            table: "issue",
            column: "updated_at"
        }
    ),
    decl!(
        "updated_at__exact",
        FilterKind::Date {
            table: "issue",
            column: "updated_at"
        },
        alias_of = "updated_at"
    ),
    decl!(
        "updated_at__range",
        FilterKind::DateRange {
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
/// `None` → `""`, booleans → `"True"` / `"False"`, numbers render plainly,
/// lists repeat the key. Every compiler below takes the stringified form,
/// so validation matches the filterset's (e.g. `UUIDField("")` cleans to
/// `None`, i.e. `IS NULL` — a JSON `null` never errors).
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

fn parse_uuid_field(field: &str, text: &str) -> Result<Option<uuid::Uuid>, FilterError> {
    if text.is_empty() {
        return Ok(None);
    }
    uuid::Uuid::parse_str(text)
        .map(Some)
        .map_err(|_| FilterError::InvalidLookupValue(field.to_owned()))
}

/// An `__in` list: items stringify like the `QueryDict` round-trip. Empty
/// items clean to `None` (`IN (NULL)`), which never matches, so they are
/// dropped — result-identical. A list left empty matches nothing, like
/// Python's `IN (NULL)`.
fn uuid_list(field: &str, value: &Value) -> Result<Vec<uuid::Uuid>, FilterError> {
    let texts = match value {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                stringify_scalar(item)
                    .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        single => vec![stringify_scalar(single)
            .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))?],
    };
    texts
        .iter()
        .map(|text| parse_uuid_field(field, text))
        .collect::<Result<Vec<_>, _>>()
        .map(|ids| ids.into_iter().flatten().collect())
}

fn text_list(field: &str, value: &Value) -> Result<Vec<String>, FilterError> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                stringify_scalar(item)
                    .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))
            })
            .collect(),
        single => Ok(vec![stringify_scalar(single)
            .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))?]),
    }
}

/// `filter_is_archived` on the stringified value: truthy spellings →
/// `archived_at IS NOT NULL`, falsy spellings → `IS NULL`, anything else →
/// no filter (`Q()`). Note `"1.0"` is not truthy: the backend stringifies
/// through `QueryDict` first, so only the exact spellings count.
pub fn archived_condition(value: &Value) -> SimpleExpr {
    const TRUTHY: &[&str] = &["true", "True", "1"];
    const FALSY: &[&str] = &["false", "False", "0"];
    let col = Expr::col((Alias::new("issue"), Alias::new("archived_at")));
    let text = stringify_scalar(value).unwrap_or_default();
    if TRUTHY.contains(&text.as_str()) {
        col.is_not_null()
    } else if FALSY.contains(&text.as_str()) {
        col.is_null()
    } else {
        // `Q()`: no filter.
        Expr::cust("1 = 1")
    }
}

/// `BooleanFilter` on the stringified value. `""` (a JSON `null`) cleans to
/// `False`. Anything outside the known spellings is rejected; Django's form
/// field would coerce an unknown non-empty string to `True`, a garbage-input
/// divergence documented here rather than copied.
fn parse_flag(field: &str, value: &Value) -> Result<bool, FilterError> {
    let text =
        stringify_scalar(value).ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))?;
    match text.as_str() {
        "true" | "True" | "1" => Ok(true),
        "false" | "False" | "0" | "" => Ok(false),
        _ => Err(FilterError::InvalidLookupValue(field.to_owned())),
    }
}

/// A date/datetime the model field accepts, in the string form Postgres
/// compares. Mirrors the form-field parse: invalid strings are a filterset
/// error in Python, never a database error.
fn parse_date_text(field: &str, text: &str) -> Result<String, FilterError> {
    if text.is_empty() {
        return Ok(String::new());
    }
    let invalid = || FilterError::InvalidLookupValue(field.to_owned());
    if chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok() {
        return Ok(text.to_owned());
    }
    if chrono::DateTime::parse_from_rfc3339(text).is_ok() {
        return Ok(text.to_owned());
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S%.f",
    ] {
        if chrono::NaiveDateTime::parse_from_str(text, format).is_ok() {
            return Ok(text.to_owned());
        }
    }
    Err(invalid())
}

fn col(table: &'static str, column: &'static str) -> Expr {
    Expr::col((Alias::new(table), Alias::new(column)))
}

/// Compile one `{name: value}` leaf against the declaration table.
/// Mirrors `_validate_fields` (unknown names are rejected) plus
/// `build_combined_q` for a single filter.
pub fn compile_leaf(name: &str, value: &Value) -> Result<SimpleExpr, FilterError> {
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
                let ids = uuid_list(field, value)?;
                if ids.is_empty() {
                    // `IN (NULL)`: matches nothing.
                    Ok(Expr::cust("FALSE"))
                } else {
                    Ok(target.is_in(ids))
                }
            } else {
                let text = stringify_scalar(value)
                    .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))?;
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
                let ids = uuid_list(field, value)?;
                if ids.is_empty() {
                    Ok(Expr::cust("FALSE").and(guard))
                } else {
                    Ok(target.is_in(ids).and(guard))
                }
            } else {
                let text = stringify_scalar(value)
                    .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))?;
                match parse_uuid_field(field, &text)? {
                    Some(id) => Ok(target.eq(id).and(guard)),
                    None => Ok(target.is_null().and(guard)),
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
                Ok(target.is_in(text_list(field, value)?))
            } else {
                let text = stringify_scalar(value)
                    .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))?;
                Ok(target.eq(text))
            }
        }
        FilterKind::Flag { table, column } => Ok(col(table, column).eq(parse_flag(field, value)?)),
        FilterKind::Date { table, column } => {
            let text = stringify_scalar(value)
                .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))?;
            let bound = parse_date_text(field, &text)?;
            if bound.is_empty() {
                Ok(col(table, column).is_null())
            } else {
                Ok(col(table, column).eq(bound))
            }
        }
        FilterKind::DateRange { table, column } => match value {
            Value::Array(items) if items.len() == 2 => {
                let bounds = items
                    .iter()
                    .map(|item| {
                        let text = stringify_scalar(item)
                            .ok_or_else(|| FilterError::InvalidLookupValue(field.to_owned()))?;
                        parse_date_text(field, &text)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(col(table, column).between(bounds[0].clone(), bounds[1].clone()))
            }
            _ => Err(FilterError::InvalidLookupValue(field.to_owned())),
        },
        FilterKind::Archived => Ok(archived_condition(value)),
    }
}

/// Port of `build_combined_q`: AND the conditions for the filters actually
/// provided, in the caller's order. Unknown names are rejected.
pub fn build_combined(provided: &[(String, Value)]) -> Result<Condition, FilterError> {
    let mut combined = Condition::all();
    for (name, value) in provided {
        combined = combined.add(compile_leaf(name, value)?);
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
        let rendered = select_where(compile_leaf("assignee_id", &Value::from(id)).unwrap());
        assert_eq!(
            rendered,
            format!(
                r#"SELECT 1 WHERE "issue_assignee"."assignee_id" = '{id}' AND "issue_assignee"."deleted_at" IS NULL"#
            )
        );
        let rendered = select_where(
            compile_leaf("label_id__in", &Value::Array(vec![Value::from(id)])).unwrap(),
        );
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
            compile_leaf("assignee_id", &Value::from("not-a-uuid")),
            Err(FilterError::InvalidLookupValue("assignee_id".to_owned()))
        );
        assert_eq!(
            compile_leaf("nope", &Value::from("x")),
            Err(FilterError::InvalidField("nope".to_owned()))
        );
    }

    #[test]
    fn archived_tri_state_matches_python() {
        let truthy = select_where(compile_leaf("is_archived", &Value::from("true")).unwrap());
        assert!(truthy.contains(r#""archived_at" IS NOT NULL"#), "{truthy}");
        let falsy = select_where(compile_leaf("is_archived", &Value::Bool(false)).unwrap());
        assert!(falsy.contains(r#""archived_at" IS NULL"#), "{falsy}");
        assert!(!falsy.contains("IS NOT NULL"), "{falsy}");
        // Unrecognized values filter nothing (Q()).
        let noop = select_where(compile_leaf("is_archived", &Value::from("maybe")).unwrap());
        assert!(noop.contains("1 = 1"), "{noop}");
        // "1.0" is not truthy: the backend stringifies first, so only the
        // exact spellings count.
        let one = select_where(compile_leaf("is_archived", &serde_json::json!(1.0)).unwrap());
        assert!(one.contains("1 = 1"), "{one}");
    }

    #[test]
    fn direct_fields_compile() {
        let rendered = select_where(
            compile_leaf(
                "state_group__in",
                &Value::Array(vec![Value::from("backlog")]),
            )
            .unwrap(),
        );
        assert!(
            rendered.contains(r#""state"."group" IN ('backlog')"#),
            "{rendered}"
        );
        let rendered = select_where(compile_leaf("priority", &Value::from("urgent")).unwrap());
        assert!(
            rendered.contains(r#""issue"."priority" = 'urgent'"#),
            "{rendered}"
        );
        let rendered = select_where(
            compile_leaf(
                "start_date__range",
                &Value::Array(vec![Value::from("2024-01-01"), Value::from("2024-02-01")]),
            )
            .unwrap(),
        );
        assert!(rendered.contains("BETWEEN"), "{rendered}");
        assert_eq!(
            compile_leaf("start_date__range", &Value::from("2024-01-01")),
            Err(FilterError::InvalidLookupValue(
                "start_date__range".to_owned()
            ))
        );
        let rendered = select_where(compile_leaf("is_draft", &Value::Bool(true)).unwrap());
        assert!(
            rendered.contains(r#""issue"."is_draft" = TRUE"#),
            "{rendered}"
        );
    }

    #[test]
    fn null_and_empty_string_follow_the_querydict_round_trip() {
        // UUIDField("") cleans to None: IS NULL, never an error.
        let rendered = select_where(compile_leaf("assignee_id", &Value::Null).unwrap());
        assert!(
            rendered.contains(r#""issue_assignee"."assignee_id" IS NULL"#),
            "{rendered}"
        );
        let rendered = select_where(compile_leaf("assignee_id", &Value::from("")).unwrap());
        assert!(rendered.contains("IS NULL"), "{rendered}");
        // CharField("") matches the empty string.
        let rendered = select_where(compile_leaf("priority", &Value::Null).unwrap());
        assert!(
            rendered.contains(r#""issue"."priority" = ''"#),
            "{rendered}"
        );
        // BooleanField("") cleans to False.
        let rendered = select_where(compile_leaf("is_draft", &Value::Null).unwrap());
        assert!(
            rendered.contains(r#""issue"."is_draft" = FALSE"#),
            "{rendered}"
        );
        // DateField("") cleans to None.
        let rendered = select_where(compile_leaf("start_date", &Value::Null).unwrap());
        assert!(
            rendered.contains(r#""issue"."start_date" IS NULL"#),
            "{rendered}"
        );
        // Unparseable dates are a filterset error, never a database error.
        assert_eq!(
            compile_leaf("start_date", &Value::from("yesterday")),
            Err(FilterError::InvalidLookupValue("start_date".to_owned()))
        );
        // Datetime strings pass through for the datetime columns.
        let rendered =
            select_where(compile_leaf("created_at", &Value::from("2024-01-02T03:04:05Z")).unwrap());
        assert!(rendered.contains("2024-01-02T03:04:05Z"), "{rendered}");
        // An __in list of only nulls matches nothing, like IN (NULL).
        let rendered = select_where(
            compile_leaf("assignee_id__in", &Value::Array(vec![Value::Null])).unwrap(),
        );
        assert!(rendered.contains("FALSE"), "{rendered}");
    }

    #[test]
    fn combined_ands_provided_filters_in_order() {
        let cond = build_combined(&[
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
        assert!(build_combined(&[("nope".to_owned(), Value::Null)]).is_err());
    }
}
