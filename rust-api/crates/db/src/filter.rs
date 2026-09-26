//! Dynamic JSON filters compiled to sea-query conditions.
//!
//! Mirrors `ComplexFilterBackend`
//! (`pi_dash/utils/filters/filter_backend.py`), the kernel half. The tree
//! shape, structure rules and error cases are ported one for one; the
//! per-view half (concrete `FilterSet` declarations, custom methods) is
//! wired by F-07 on top of this module.
//!
//! Wire format (the `filters` query parameter, JSON):
//!
//! - `{"or": [...]}` / `{"and": [...]}`: non-empty lists of child objects
//!   (Django `Q() | ...` / `Q() & ...`). Operator keys are case-insensitive.
//! - `{"not": {...}}`: a single child object (`~Q()`).
//! - Anything else is a leaf: `{field__lookup: value}` pairs evaluated as
//!   one AND group, exactly like the backend handing the leaf dict to the
//!   view's `FilterSet`.
//!
//! Rules, as in `_validate_structure` / `_validate_leaf` /
//! `_validate_fields`:
//!
//! - One logical operator per object; operators never mix with field keys.
//! - Nesting depth is bounded (`DEFAULT_MAX_DEPTH = 5`, per view
//!   overridable via [`FilterTree::from_json_with_depth`]).
//! - Leaves are non-empty; values are scalars, null, or non-empty lists of
//!   scalars.
//! - Every leaf key must be declared in the view's allowlist (the
//!   `FilterSet.base_filters` keys, lookups included); without an
//!   allowlist filtering is rejected outright.
//!
//! Leaf keys are `field` (exact match) or `field__lookup` with one of:
//! `exact`, `in`, `gt`, `gte`, `lt`, `lte`, `contains`, `icontains`,
//! `startswith`, `istartswith`, `endswith`, `iendswith`, `iexact`,
//! `isnull`, `range`.
//! `icontains` / `istartswith` / `iexact` fold case with `LOWER()` on the
//! column, as Django does; LIKE metacharacters in the pattern are
//! backslash-escaped, also as Django does.

use std::collections::HashSet;

use sea_query::{Alias, Condition, Expr, Func, SimpleExpr};
use serde_json::Value;
use thiserror::Error as ThisError;

/// Default maximum nesting depth, mirroring `default_max_depth = 5`.
pub const DEFAULT_MAX_DEPTH: usize = 5;

/// A validated filter tree, ready to compile against a field allowlist.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterTree {
    And(Vec<FilterTree>),
    Or(Vec<FilterTree>),
    Not(Box<FilterTree>),
    Leaf(Vec<LeafCondition>),
}

/// One `field__lookup = value` predicate inside a leaf.
#[derive(Debug, Clone, PartialEq)]
pub struct LeafCondition {
    pub field: String,
    pub lookup: Lookup,
    pub value: Value,
}

/// Django field lookups supported by the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Exact,
    In,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    IContains,
    StartsWith,
    IStartsWith,
    EndsWith,
    IEndsWith,
    IExact,
    IsNull,
    Range,
}

impl Lookup {
    fn parse(key: &str) -> (String, Self) {
        match key.rsplit_once("__") {
            Some((field, suffix)) => {
                let lookup = match suffix {
                    "exact" => Lookup::Exact,
                    "in" => Lookup::In,
                    "gt" => Lookup::Gt,
                    "gte" => Lookup::Gte,
                    "lt" => Lookup::Lt,
                    "lte" => Lookup::Lte,
                    "contains" => Lookup::Contains,
                    "icontains" => Lookup::IContains,
                    "startswith" => Lookup::StartsWith,
                    "istartswith" => Lookup::IStartsWith,
                    "endswith" => Lookup::EndsWith,
                    "iendswith" => Lookup::IEndsWith,
                    "iexact" => Lookup::IExact,
                    "isnull" => Lookup::IsNull,
                    "range" => Lookup::Range,
                    _ => return (key.to_owned(), Lookup::Exact),
                };
                (field.to_owned(), lookup)
            }
            None => (key.to_owned(), Lookup::Exact),
        }
    }
}

/// Why a filter was rejected. Each variant mirrors a `DRFValidationError`
/// `code` from the Python backend.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum FilterError {
    #[error("invalid JSON for filter input")]
    InvalidJson,
    #[error("filter input must be a JSON object")]
    InvalidNode,
    #[error("filter objects must not be empty")]
    EmptyNode,
    #[error("filter nesting is too deep (max {0})")]
    MaxDepthExceeded(usize),
    #[error("a filter object cannot contain multiple logical operators")]
    MultipleOperators,
    #[error("cannot mix a logical operator with field keys")]
    MixedOperatorAndFields,
    #[error("logical operator children must be non-empty lists of objects")]
    InvalidOperatorChildren,
    #[error("'not' must wrap a single filter object")]
    InvalidNotChild,
    #[error("logical operators cannot appear in a leaf filter object")]
    OperatorInLeaf,
    #[error("list filter values must not be empty")]
    EmptyListValue,
    #[error("filter values must be scalars, null, or lists of scalars")]
    InvalidValue,
    #[error("filtering is not enabled for this endpoint (missing allowlist)")]
    FilteringNotEnabled,
    #[error("filtering on field '{0}' is not allowed")]
    InvalidField(String),
    #[error("invalid value for lookup on field '{0}'")]
    InvalidLookupValue(String),
    /// A `__range` filter whose whole value is empty (`""`/`null`). Python's
    /// form stays valid with `[]` and only fails at SQL-compile time
    /// (`ValueError`), which the views do not catch — a 500, unlike
    /// [`FilterError::InvalidLookupValue`]. The handlers answer 500 for this.
    #[error("range filter on field '{0}' has no bounds")]
    EmptyRangeBounds(String),
}

impl FilterTree {
    /// Parse the `filters` query parameter. Mirrors
    /// `_normalize_filter_data`: the input is a JSON string or an
    /// already-decoded value; malformed JSON is an error.
    pub fn parse_param(raw: &str) -> Result<Self, FilterError> {
        let value: Value = serde_json::from_str(raw).map_err(|_| FilterError::InvalidJson)?;
        Self::from_json(&value)
    }

    pub fn from_json(value: &Value) -> Result<Self, FilterError> {
        Self::from_json_with_depth(value, DEFAULT_MAX_DEPTH)
    }

    pub fn from_json_with_depth(value: &Value, max_depth: usize) -> Result<Self, FilterError> {
        parse_node(value, max_depth, 1)
    }

    /// Compile the tree to a sea-query [`Condition`], checking every leaf
    /// key against `allowed` (the view's declared filter names, `__lookup`
    /// suffixes included). Mirrors `_validate_fields` + `_evaluate_node`.
    pub fn build_condition(&self, allowed: &HashSet<String>) -> Result<Condition, FilterError> {
        if allowed.is_empty() {
            return Err(FilterError::FilteringNotEnabled);
        }
        build_condition(self, allowed)
    }
}

fn parse_node(value: &Value, max_depth: usize, depth: usize) -> Result<FilterTree, FilterError> {
    if depth > max_depth {
        return Err(FilterError::MaxDepthExceeded(max_depth));
    }
    let obj = value.as_object().ok_or(FilterError::InvalidNode)?;
    if obj.is_empty() {
        return Err(FilterError::EmptyNode);
    }
    let ops: Vec<(&String, &Value)> = obj
        .iter()
        .filter(|(k, _)| {
            let lower = k.to_lowercase();
            lower == "or" || lower == "and" || lower == "not"
        })
        .collect();
    if ops.len() > 1 {
        return Err(FilterError::MultipleOperators);
    }
    if let Some((_, child)) = ops.first() {
        if obj.len() != 1 {
            return Err(FilterError::MixedOperatorAndFields);
        }
        let op = ops[0].0.to_lowercase();
        return match op.as_str() {
            "or" | "and" => {
                let children = child
                    .as_array()
                    .ok_or(FilterError::InvalidOperatorChildren)?;
                if children.is_empty() {
                    return Err(FilterError::InvalidOperatorChildren);
                }
                let mut nodes = Vec::with_capacity(children.len());
                for item in children {
                    if !item.is_object() {
                        return Err(FilterError::InvalidOperatorChildren);
                    }
                    nodes.push(parse_node(item, max_depth, depth + 1)?);
                }
                Ok(if op == "or" {
                    FilterTree::Or(nodes)
                } else {
                    FilterTree::And(nodes)
                })
            }
            _ => {
                if !child.is_object() {
                    return Err(FilterError::InvalidNotChild);
                }
                Ok(FilterTree::Not(Box::new(parse_node(
                    child,
                    max_depth,
                    depth + 1,
                )?)))
            }
        };
    }
    parse_leaf(obj)
}

fn parse_leaf(obj: &serde_json::Map<String, Value>) -> Result<FilterTree, FilterError> {
    let mut conditions = Vec::with_capacity(obj.len());
    for (key, value) in obj {
        let lower = key.to_lowercase();
        if lower == "or" || lower == "and" || lower == "not" {
            return Err(FilterError::OperatorInLeaf);
        }
        check_value(value)?;
        let (field, lookup) = Lookup::parse(key);
        conditions.push(LeafCondition {
            field,
            lookup,
            value: value.clone(),
        });
    }
    Ok(FilterTree::Leaf(conditions))
}

fn check_value(value: &Value) -> Result<(), FilterError> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
        Value::Array(items) => {
            if items.is_empty() {
                return Err(FilterError::EmptyListValue);
            }
            for item in items {
                match item {
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
                    _ => return Err(FilterError::InvalidValue),
                }
            }
            Ok(())
        }
        _ => Err(FilterError::InvalidValue),
    }
}

fn build_condition(tree: &FilterTree, allowed: &HashSet<String>) -> Result<Condition, FilterError> {
    match tree {
        FilterTree::And(children) => {
            let mut cond = Condition::all();
            for child in children {
                cond = cond.add(build_condition(child, allowed)?);
            }
            Ok(cond)
        }
        FilterTree::Or(children) => {
            let mut cond = Condition::any();
            for child in children {
                cond = cond.add(build_condition(child, allowed)?);
            }
            Ok(cond)
        }
        FilterTree::Not(child) => Ok(build_condition(child, allowed)?.not()),
        FilterTree::Leaf(conditions) => {
            let mut cond = Condition::all();
            for leaf in conditions {
                cond = cond.add(build_leaf(leaf, allowed)?);
            }
            Ok(cond)
        }
    }
}

fn lookup_key(leaf: &LeafCondition) -> String {
    match leaf.lookup {
        Lookup::Exact => leaf.field.clone(),
        _ => {
            let suffix = match leaf.lookup {
                Lookup::Exact => unreachable!(),
                Lookup::In => "in",
                Lookup::Gt => "gt",
                Lookup::Gte => "gte",
                Lookup::Lt => "lt",
                Lookup::Lte => "lte",
                Lookup::Contains => "contains",
                Lookup::IContains => "icontains",
                Lookup::StartsWith => "startswith",
                Lookup::IStartsWith => "istartswith",
                Lookup::EndsWith => "endswith",
                Lookup::IEndsWith => "iendswith",
                Lookup::IExact => "iexact",
                Lookup::IsNull => "isnull",
                Lookup::Range => "range",
            };
            format!("{}__{suffix}", leaf.field)
        }
    }
}

fn build_leaf(leaf: &LeafCondition, allowed: &HashSet<String>) -> Result<SimpleExpr, FilterError> {
    if !allowed.contains(&lookup_key(leaf)) {
        return Err(FilterError::InvalidField(lookup_key(leaf)));
    }
    let col = Alias::new(leaf.field.clone());
    let invalid = || FilterError::InvalidLookupValue(leaf.field.clone());
    match leaf.lookup {
        Lookup::Exact => match &leaf.value {
            Value::Null => Ok(Expr::col(col).is_null()),
            Value::Bool(b) => Ok(Expr::col(col).eq(*b)),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Expr::col(col).eq(i))
                } else if let Some(u) = n.as_u64() {
                    Ok(Expr::col(col).eq(u))
                } else {
                    n.as_f64().map(|f| Expr::col(col).eq(f)).ok_or_else(invalid)
                }
            }
            Value::String(s) => Ok(Expr::col(col).eq(s.clone())),
            _ => Err(invalid()),
        },
        Lookup::In => match &leaf.value {
            Value::Array(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(json_scalar(item).ok_or_else(invalid)?);
                }
                Ok(Expr::col(col).is_in(values))
            }
            single => Ok(Expr::col(col).is_in(vec![json_scalar(single).ok_or_else(invalid)?])),
        },
        Lookup::Gt => Ok(Expr::col(col).gt(json_scalar(&leaf.value).ok_or_else(invalid)?)),
        Lookup::Gte => Ok(Expr::col(col).gte(json_scalar(&leaf.value).ok_or_else(invalid)?)),
        Lookup::Lt => Ok(Expr::col(col).lt(json_scalar(&leaf.value).ok_or_else(invalid)?)),
        Lookup::Lte => Ok(Expr::col(col).lte(json_scalar(&leaf.value).ok_or_else(invalid)?)),
        Lookup::Contains => {
            let pattern = string_value(&leaf.value).ok_or_else(invalid)?;
            Ok(Expr::col(col).like(format!("%{}%", escape_like(&pattern))))
        }
        Lookup::IContains => {
            let pattern = string_value(&leaf.value).ok_or_else(invalid)?;
            Ok(lower_col(&col).like(format!("%{}%", escape_like(&pattern.to_lowercase()))))
        }
        Lookup::StartsWith => {
            let pattern = string_value(&leaf.value).ok_or_else(invalid)?;
            Ok(Expr::col(col).like(format!("{}%", escape_like(&pattern))))
        }
        Lookup::IStartsWith => {
            let pattern = string_value(&leaf.value).ok_or_else(invalid)?;
            Ok(lower_col(&col).like(format!("{}%", escape_like(&pattern.to_lowercase()))))
        }
        Lookup::EndsWith => {
            let pattern = string_value(&leaf.value).ok_or_else(invalid)?;
            Ok(Expr::col(col).like(format!("%{}", escape_like(&pattern))))
        }
        Lookup::IEndsWith => {
            let pattern = string_value(&leaf.value).ok_or_else(invalid)?;
            Ok(lower_col(&col).like(format!("%{}", escape_like(&pattern.to_lowercase()))))
        }
        Lookup::IExact => {
            let pattern = string_value(&leaf.value).ok_or_else(invalid)?;
            Ok(lower_col(&col).eq(pattern.to_lowercase()))
        }
        Lookup::IsNull => match &leaf.value {
            Value::Null => Ok(Expr::col(col).is_null()),
            Value::Bool(true) => Ok(Expr::col(col).is_null()),
            Value::Bool(false) => Ok(Expr::col(col).is_not_null()),
            _ => Err(invalid()),
        },
        Lookup::Range => match &leaf.value {
            Value::Array(items) if items.len() == 2 => {
                let lo = json_scalar(&items[0]).ok_or_else(invalid)?;
                let hi = json_scalar(&items[1]).ok_or_else(invalid)?;
                Ok(Expr::col(col).between(lo, hi))
            }
            _ => Err(invalid()),
        },
    }
}

/// `LOWER("col")` as an expression, mirroring Django's `UPPER()`/`LOWER()`
/// wrapping for case-insensitive lookups. The pattern side is folded in
/// Rust, which compares equal for the same rows.
fn lower_col(col: &Alias) -> SimpleExpr {
    SimpleExpr::from(Func::lower(Expr::col(col.clone())))
}

/// Django escapes LIKE metacharacters with a backslash in `contains` /
/// `startswith` patterns (`pattern_ops`).
fn escape_like(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    for ch in pattern.chars() {
        if ch == '\\' || ch == '%' || ch == '_' {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn string_value(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Null => None,
        _ => None,
    }
}

fn json_scalar(value: &Value) -> Option<sea_query::Value> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(sea_query::Value::Bool(Some(*b))),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(sea_query::Value::BigInt(Some(i)))
            } else if let Some(u) = n.as_u64() {
                Some(sea_query::Value::BigUnsigned(Some(u)))
            } else {
                n.as_f64().map(|f| sea_query::Value::Double(Some(f)))
            }
        }
        Value::String(s) => Some(sea_query::Value::String(Some(s.clone().into()))),
        _ => None,
    }
}

/// Declared filter names for one view, mirroring
/// `FilterSet.base_filters.keys()`. Keys carry their lookup suffix
/// (`sequence_id__gte`), exactly as the backend requires.
pub fn allowlist(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Render helper for tests and debugging: the compiled `WHERE` clause.
#[cfg(test)]
pub(crate) fn render(cond: &Condition) -> String {
    use sea_query::{PostgresQueryBuilder, Query};
    let mut select = Query::select();
    select.column(Alias::new("id"));
    select.from(Alias::new("t"));
    select.and_where(SimpleExpr::from(cond.clone()));
    select.to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn allowed() -> HashSet<String> {
        allowlist(&[
            "priority",
            "priority__in",
            "sequence_id__gte",
            "title__icontains",
            "title__contains",
            "name__istartswith",
            "name__iendswith",
            "name__iexact",
            "deleted_at__isnull",
            "start_date__range",
            "state",
        ])
    }

    fn parse(raw: &Value) -> FilterTree {
        FilterTree::from_json(raw).expect("valid filter")
    }

    #[test]
    fn empty_filter_string_is_invalid_json() {
        assert_eq!(
            FilterTree::parse_param("not json{{").unwrap_err(),
            FilterError::InvalidJson
        );
    }

    #[test]
    fn non_object_and_empty_object_rejected() {
        assert_eq!(
            FilterTree::from_json(&json!([1])).unwrap_err(),
            FilterError::InvalidNode
        );
        assert_eq!(
            FilterTree::from_json(&json!({})).unwrap_err(),
            FilterError::EmptyNode
        );
    }

    #[test]
    fn mixed_operators_and_mixed_fields_rejected() {
        let both = json!({"or": [{"state": "x"}], "and": [{"state": "y"}]});
        assert_eq!(
            FilterTree::from_json(&both).unwrap_err(),
            FilterError::MultipleOperators
        );
        let mixed = json!({"or": [{"state": "x"}], "state": "y"});
        assert_eq!(
            FilterTree::from_json(&mixed).unwrap_err(),
            FilterError::MixedOperatorAndFields
        );
    }

    #[test]
    fn operator_children_must_be_non_empty_object_lists() {
        assert_eq!(
            FilterTree::from_json(&json!({"or": []})).unwrap_err(),
            FilterError::InvalidOperatorChildren
        );
        assert_eq!(
            FilterTree::from_json(&json!({"or": [1]})).unwrap_err(),
            FilterError::InvalidOperatorChildren
        );
        assert_eq!(
            FilterTree::from_json(&json!({"not": [1]})).unwrap_err(),
            FilterError::InvalidNotChild
        );
    }

    #[test]
    fn operator_keys_are_case_insensitive() {
        let tree = parse(&json!({"OR": [{"state": "x"}]}));
        assert!(matches!(tree, FilterTree::Or(_)));
        let tree = parse(&json!({"Not": {"state": "x"}}));
        assert!(matches!(tree, FilterTree::Not(_)));
    }

    #[test]
    fn depth_limit_enforced() {
        let mut node = json!({"state": "x"});
        for _ in 0..DEFAULT_MAX_DEPTH {
            node = json!({"and": [node]});
        }
        assert_eq!(
            FilterTree::from_json(&node).unwrap_err(),
            FilterError::MaxDepthExceeded(DEFAULT_MAX_DEPTH)
        );
    }

    #[test]
    fn leaf_values_must_be_scalar_or_scalar_lists() {
        assert_eq!(
            FilterTree::from_json(&json!({"state": {"a": 1}})).unwrap_err(),
            FilterError::InvalidValue
        );
        assert_eq!(
            FilterTree::from_json(&json!({"state": []})).unwrap_err(),
            FilterError::EmptyListValue
        );
        assert_eq!(
            FilterTree::from_json(&json!({"state": [1, [2]]})).unwrap_err(),
            FilterError::InvalidValue
        );
        // An operator key always takes the operator path: mixed with a
        // field it is MixedOperatorAndFields, never a leaf. The leaf
        // guard mirrors `_validate_leaf`'s (likewise unreachable) check.
        assert_eq!(
            FilterTree::from_json(&json!({"state": "x", "or": [{"state": "y"}]})).unwrap_err(),
            FilterError::MixedOperatorAndFields
        );
    }

    #[test]
    fn unknown_fields_and_missing_allowlist_rejected() {
        let tree = parse(&json!({"nope": "x"}));
        assert_eq!(
            tree.build_condition(&allowed()).unwrap_err(),
            FilterError::InvalidField("nope".to_owned())
        );
        let tree = parse(&json!({"state": "x"}));
        assert_eq!(
            tree.build_condition(&HashSet::new()).unwrap_err(),
            FilterError::FilteringNotEnabled
        );
    }

    #[test]
    fn exact_null_becomes_is_null() {
        let tree = parse(&json!({"deleted_at__isnull": true}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        assert!(sql.contains(r#""deleted_at" IS NULL"#), "{sql}");
    }

    #[test]
    fn in_and_range_compile() {
        let tree = parse(&json!({"priority__in": ["high", "urgent"]}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        assert!(sql.contains(r#""priority" IN ('high', 'urgent')"#), "{sql}");

        let tree = parse(&json!({"start_date__range": ["2026-01-01", "2026-02-01"]}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        assert!(
            sql.contains(r#""start_date" BETWEEN '2026-01-01' AND '2026-02-01'"#),
            "{sql}"
        );
    }

    #[test]
    fn icontains_lowers_column_and_escapes_like_wildcards() {
        let tree = parse(&json!({"title__icontains": "100%_done\\x"}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        // Backslashes in the pattern make sea-query render a Postgres
        // escape-string literal; the matched value is unchanged.
        assert!(
            sql.contains(r#"LOWER("title") LIKE E'%100\\%\\_done\\\\x%'"#),
            "{sql}"
        );
    }

    #[test]
    fn istartswith_and_iexact_compile() {
        let tree = parse(&json!({"name__istartswith": "Ab"}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        assert!(sql.contains(r#"LOWER("name") LIKE 'ab%'"#), "{sql}");

        let tree = parse(&json!({"name__iexact": "Ab"}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        assert!(sql.contains(r#"LOWER("name") = 'ab'"#), "{sql}");

        let tree = parse(&json!({"title__contains": "bug"}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        assert!(sql.contains(r#""title" LIKE '%bug%'"#), "{sql}");

        let tree = parse(&json!({"name__iendswith": "Ab"}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        assert!(sql.contains(r#"LOWER("name") LIKE '%ab'"#), "{sql}");
    }

    #[test]
    fn boolean_tree_compiles_with_not() {
        let tree =
            parse(&json!({"and": [{"priority__in": ["high"]}, {"not": {"sequence_id__gte": 10}}]}));
        let sql = render(&tree.build_condition(&allowed()).expect("build"));
        assert!(sql.contains("NOT"), "{sql}");
        assert!(sql.contains(r#""sequence_id" >= 10"#), "{sql}");
    }

    #[test]
    fn unknown_lookup_suffix_falls_back_to_exact_and_fails_allowlist() {
        let tree = parse(&json!({"priority__bogus": "high"}));
        assert_eq!(
            tree.build_condition(&allowed()).unwrap_err(),
            FilterError::InvalidField("priority__bogus".to_owned())
        );
    }
}
