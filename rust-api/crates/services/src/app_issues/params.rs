#![forbid(unsafe_code)]

//! Query-param parsing for the issue-list family with Django's exact error
//! bodies.
//!
//! Ports the parameter reads in `IssueViewSet.list`,
//! `IssueListEndpoint.get`, `IssuePaginatedViewSet.list`,
//! `DeletedIssuesListViewSet.get`, and `BasePaginator.get_per_page` /
//! `paginate` (`app/views/base.py`, `utils/paginator.py`):
//! - `per_page`: `int(GET["per_page"] or 1000)`; a non-integer raises
//!   `ParseError("Invalid per_page parameter.")`; above the max (1000)
//!   raises `ParseError("Invalid per_page value. Cannot exceed 1000.")`.
//!   Both render as `{"detail": ...}` with status 400.
//! - `cursor`: `Cursor.from_string(GET["cursor"] or f"{per_page}:0:0")`; a
//!   `ValueError` raises `ParseError("Invalid cursor parameter.")`.
//! - `group_by` / `sub_group_by`: `GET.get(name, False)` — absent means
//!   "no grouping", and the values flow into the grouper verbatim.
//! - `order_by`: `GET.get("order_by", "-created_at")`.
//! - `updated_at__gt`: `GET.get("updated_at__gt", None)`.
//! - `issues`: `GET.get("issues", False)`; missing/empty is a 400
//!   `{"error": "Issues are required"}` (flat endpoint only).
//! - `fields` / `expand`: comma-split, empties dropped; empty means `None`.
//! - `description`: `GET.get("description", "false")`, true when
//!   lowercased it equals `"true"` (v2 only).

use std::collections::HashMap;

/// A 400 the parameter parsing raises, with the exact DRF body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamError {
    /// `detail` (DRF `ParseError`) or `error` (view-inline) key.
    pub key: &'static str,
    /// The human message.
    pub message: String,
}

impl ParamError {
    /// `ParseError(detail=...)` renders `{"detail": detail}` at 400.
    pub fn detail(message: impl Into<String>) -> Self {
        Self {
            key: "detail",
            message: message.into(),
        }
    }

    /// View-inline `Response({"error": ...}, 400)`.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            key: "error",
            message: message.into(),
        }
    }

    /// The exact JSON body Django renders (compact separators, no spaces).
    pub fn body(&self) -> String {
        let escaped = self.message.replace('\\', "\\\\").replace('"', "\\\"");
        format!("{{\"{}\":\"{}\"}}", self.key, escaped)
    }
}

/// Parsed query params shared by the four list-family handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListParams {
    /// `per_page`, default 1000, max 1000.
    pub per_page: i64,
    /// Raw cursor string (parsed by the F-07 paginator kernel).
    pub cursor_raw: String,
    /// `group_by` value, or `None` when absent.
    pub group_by: Option<String>,
    /// `sub_group_by` value, or `None` when absent.
    pub sub_group_by: Option<String>,
    /// `order_by`, default `-created_at`.
    pub order_by: String,
    /// `updated_at__gt`, or `None` when absent.
    pub updated_at_gt: Option<String>,
    /// Comma-split `issues` ids (flat endpoint; `None` means absent).
    pub issue_ids: Option<Vec<String>>,
    /// Comma-split `fields`, empties dropped; `None` when empty/absent.
    pub fields: Option<Vec<String>>,
    /// Comma-split `expand`, empties dropped; `None` when empty/absent.
    pub expand: Option<Vec<String>>,
    /// v2 `description=true` (case-insensitive `"true"`).
    pub description: bool,
}

/// How strictly a list-family path parses its query params. Only the
/// main `issues/` list validates `per_page` and the group mismatch:
/// the flat, v2, deleted and detail paths never read those params in
/// Python, so a strict parse would 400 where Django 200s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Flat `issues/list/` behavior: a missing/empty `issues` param is a
    /// 400, checked *before* anything else (Python returns
    /// `{"error": "Issues are required"}` without touching `per_page`).
    pub require_issues: bool,
    /// Validate `per_page` (`ParseError` on garbage/over-max). Flat, v2,
    /// deleted and detail paths skip it.
    pub strict_per_page: bool,
}

impl ListParams {
    /// Default `per_page` / max `per_page` on every list-family path.
    pub const DEFAULT_PER_PAGE: i64 = 1000;

    /// Parse from a multi-value query map (`GET` query params).
    /// `require_issues` selects the flat `issues/list/` behavior where a
    /// missing `issues` param is a 400.
    pub fn parse(
        query: &HashMap<String, Vec<String>>,
        require_issues: bool,
    ) -> Result<Self, ParamError> {
        Self::parse_with(
            query,
            ParseOptions {
                require_issues,
                strict_per_page: true,
            },
        )
    }

    /// Parse with explicit per-path strictness (see [`ParseOptions`]).
    pub fn parse_with(
        query: &HashMap<String, Vec<String>>,
        options: ParseOptions,
    ) -> Result<Self, ParamError> {
        // Django's `QueryDict.get` returns the *last* value on repeats.
        let first = |name: &str| query.get(name).and_then(|values| values.last().cloned());
        // The flat `issues`-required check precedes `per_page` parsing:
        // Python answers `{"error": "Issues are required"}` however broken
        // `per_page` is.
        if options.require_issues && !options.strict_per_page {
            match first("issues") {
                Some(raw) if !raw.is_empty() => {}
                _ => return Err(ParamError::error("Issues are required")),
            }
        }
        let per_page = if options.strict_per_page {
            parse_per_page(first("per_page").as_deref())?
        } else {
            Self::DEFAULT_PER_PAGE
        };
        let cursor_raw = first("cursor").unwrap_or_else(|| format!("{per_page}:0:0"));
        // `GET.get(name, False)` + `if group_by:`: an empty `?group_by=`
        // is falsy, so it behaves as absent (flat branch, never collides).
        let group_by = first("group_by").filter(|value| !value.is_empty());
        let sub_group_by = first("sub_group_by").filter(|value| !value.is_empty());
        let order_by = first("order_by").unwrap_or_else(|| "-created_at".to_owned());
        let updated_at_gt = first("updated_at__gt");
        let issue_ids = match first("issues") {
            Some(raw) if !raw.is_empty() => Some(
                raw.split(',')
                    .filter(|part| !part.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ),
            _ => {
                if options.require_issues {
                    return Err(ParamError::error("Issues are required"));
                }
                None
            }
        };
        // `GET.get("issues", False)`: an `issues=` empty value is falsy, so
        // it takes the same 400 branch as a missing param.
        if options.require_issues && issue_ids.as_ref().map(Vec::len).unwrap_or(0) == 0 {
            return Err(ParamError::error("Issues are required"));
        }
        let fields = split_list(first("fields").as_deref());
        let expand = split_list(first("expand").as_deref());
        let description = first("description")
            .map(|raw| raw.to_lowercase() == "true")
            .unwrap_or(false);
        Ok(Self {
            per_page,
            cursor_raw,
            group_by,
            sub_group_by,
            order_by,
            updated_at_gt,
            issue_ids,
            fields,
            expand,
            description,
        })
    }

    /// The grouped/sub-grouped mismatch guard: equal non-absent values are
    /// a 400 `{"error": "Group by and sub group by cannot have same
    /// parameters"}`. Django checks this only when both are set (the outer
    /// `if group_by:` / `if sub_group_by:` nesting); absent values never
    /// collide.
    pub fn group_mismatch(&self) -> Option<ParamError> {
        match (&self.group_by, &self.sub_group_by) {
            (Some(group), Some(sub)) if group == sub => Some(ParamError::error(
                "Group by and sub group by cannot have same parameters",
            )),
            _ => None,
        }
    }

    /// True when the flat `.values()` shape applies (no fields/expand).
    pub fn is_flat_shape(&self) -> bool {
        self.fields.is_none() && self.expand.is_none()
    }
}

/// The group-mismatch guard on *raw* query values, for the main-list
/// validation order: Python checks the mismatch in-view *before*
/// `paginate` parses `per_page`/cursor, so
/// `?group_by=X&sub_group_by=X&per_page=lots` answers the mismatch 400,
/// not the `per_page` 400. Empty values are falsy (`if group_by:`) and
/// never collide.
pub fn raw_group_mismatch(
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
) -> Option<ParamError> {
    match (group_by, sub_group_by) {
        (Some(group), Some(sub)) if !group.is_empty() && !sub.is_empty() && group == sub => Some(
            ParamError::error("Group by and sub group by cannot have same parameters"),
        ),
        _ => None,
    }
}

/// `BasePaginator.get_per_page`: non-integers and over-max values raise.
/// Python's `int()` is unbounded (whitespace/underscores/signs allowed),
/// so a huge magnitude parses fine and then trips the ceiling instead of
/// failing to parse. Inputs that overflow even `i128` but read as an
/// integer take the ceiling branch too, exactly like `int()` would.
pub fn parse_per_page(raw: Option<&str>) -> Result<i64, ParamError> {
    const MAX: i64 = ListParams::DEFAULT_PER_PAGE;
    let Some(text) = raw else {
        return Ok(ListParams::DEFAULT_PER_PAGE);
    };
    let ceiling = || ParamError::detail(format!("Invalid per_page value. Cannot exceed {MAX}."));
    let digits = text.trim().replace('_', "");
    let per_page = match digits.parse::<i128>() {
        Ok(value) => value,
        Err(_) => {
            // `int()` accepts an optional sign plus digits (after the same
            // trim/underscore cleanup); anything else is not an integer.
            // Beyond `i128`, emulate unbounded-then-compare: a positive
            // magnitude necessarily trips the ceiling, a negative one
            // falls through to the `i64` clamp below.
            let body: &str = digits.strip_prefix(['+', '-']).unwrap_or(&digits);
            if !body.is_empty() && body.bytes().all(|byte| byte.is_ascii_digit()) {
                if digits.starts_with('-') {
                    return Ok(i64::MIN);
                }
                return Err(ceiling());
            }
            return Err(ParamError::detail("Invalid per_page parameter."));
        }
    };
    if per_page > MAX as i128 {
        return Err(ceiling());
    }
    Ok(per_page.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

/// Comma-split with empties dropped; empty/absent means `None`.
/// Mirrors the `fields` / `expand` properties on both base views.
pub fn split_list(raw: Option<&str>) -> Option<Vec<String>> {
    let items: Vec<String> = raw
        .unwrap_or("")
        .split(',')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(pairs: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
        let mut map = HashMap::new();
        for (key, value) in pairs {
            map.entry((*key).to_owned())
                .or_insert_with(Vec::new)
                .push((*value).to_owned());
        }
        map
    }

    #[test]
    fn defaults_match_django_gets() {
        let params = ListParams::parse(&query(&[]), false).expect("parse");
        assert_eq!(params.per_page, 1000);
        assert_eq!(params.cursor_raw, "1000:0:0");
        assert_eq!(params.group_by, None);
        assert_eq!(params.sub_group_by, None);
        assert_eq!(params.order_by, "-created_at");
        assert_eq!(params.updated_at_gt, None);
        assert_eq!(params.issue_ids, None);
        assert!(params.is_flat_shape());
        assert!(!params.description);
        assert_eq!(params.group_mismatch(), None);
    }

    #[test]
    fn per_page_non_integer_is_detail_400() {
        let err = ListParams::parse(&query(&[("per_page", "lots")]), false).unwrap_err();
        assert_eq!(err.key, "detail");
        assert_eq!(err.body(), r#"{"detail":"Invalid per_page parameter."}"#);
    }

    #[test]
    fn per_page_over_max_is_detail_400() {
        let err = ListParams::parse(&query(&[("per_page", "1001")]), false).unwrap_err();
        assert_eq!(
            err.body(),
            r#"{"detail":"Invalid per_page value. Cannot exceed 1000."}"#
        );
    }

    #[test]
    fn missing_issues_is_error_400_on_flat_endpoint() {
        let err = ListParams::parse(&query(&[]), true).unwrap_err();
        assert_eq!(err.body(), r#"{"error":"Issues are required"}"#);
        // Same endpoint without the requirement parses fine.
        let params = ListParams::parse(&query(&[]), false).expect("parse");
        assert_eq!(params.issue_ids, None);
    }

    #[test]
    fn empty_issues_value_is_also_required() {
        let err = ListParams::parse(&query(&[("issues", "")]), true).unwrap_err();
        assert_eq!(err.body(), r#"{"error":"Issues are required"}"#);
    }

    #[test]
    fn issues_split_drops_empties() {
        let params = ListParams::parse(&query(&[("issues", "a,,b,")]), true).expect("parse");
        assert_eq!(params.issue_ids, Some(vec!["a".to_owned(), "b".to_owned()]));
    }

    #[test]
    fn equal_group_values_mismatch() {
        let params = ListParams::parse(
            &query(&[("group_by", "state"), ("sub_group_by", "state")]),
            false,
        )
        .expect("parse");
        let err = params.group_mismatch().expect("mismatch");
        assert_eq!(
            err.body(),
            r#"{"error":"Group by and sub group by cannot have same parameters"}"#
        );
    }

    #[test]
    fn different_group_values_pass() {
        let params = ListParams::parse(
            &query(&[("group_by", "state"), ("sub_group_by", "priority")]),
            false,
        )
        .expect("parse");
        assert_eq!(params.group_mismatch(), None);
    }

    #[test]
    fn huge_integer_per_page_trips_ceiling_like_python_int() {
        // `int("9" * 40)` parses (unbounded) then exceeds the max.
        let err = ListParams::parse(&query(&[("per_page", &"9".repeat(40))]), false).unwrap_err();
        assert_eq!(
            err.body(),
            r#"{"detail":"Invalid per_page value. Cannot exceed 1000."}"#
        );
        // Non-integers still fail to parse.
        let err = ListParams::parse(&query(&[("per_page", "lots")]), false).unwrap_err();
        assert_eq!(err.body(), r#"{"detail":"Invalid per_page parameter."}"#);
    }

    #[test]
    fn lenient_parse_skips_per_page_but_keeps_issues_required() {
        let options = ParseOptions {
            require_issues: true,
            strict_per_page: false,
        };
        // Garbage per_page is ignored; the issues check runs first.
        let params = ListParams::parse_with(&query(&[("per_page", "lots")]), options);
        assert!(params.is_err());
        let params =
            ListParams::parse_with(&query(&[("per_page", "lots"), ("issues", "a")]), options)
                .expect("parse");
        assert_eq!(params.per_page, 1000);
        assert_eq!(params.issue_ids, Some(vec!["a".to_owned()]));
    }

    #[test]
    fn empty_group_values_behave_as_absent() {
        let params = ListParams::parse(&query(&[("group_by", "")]), false).expect("parse");
        assert_eq!(params.group_by, None);
        assert_eq!(params.group_mismatch(), None);
        assert_eq!(raw_group_mismatch(Some(""), Some("")), None);
        assert_eq!(
            raw_group_mismatch(Some("state"), Some("state")).map(|error| error.body()),
            Some(r#"{"error":"Group by and sub group by cannot have same parameters"}"#.to_owned())
        );
    }

    #[test]
    fn description_true_is_case_insensitive() {
        for raw in ["true", "True", "TRUE"] {
            let params = ListParams::parse(&query(&[("description", raw)]), false).expect("parse");
            assert!(params.description, "{raw}");
        }
        let params = ListParams::parse(&query(&[("description", "yes")]), false).expect("parse");
        assert!(!params.description);
    }

    #[test]
    fn fields_expand_split_and_drop_empties() {
        let params = ListParams::parse(&query(&[("fields", "id,,name"), ("expand", "")]), false)
            .expect("parse");
        assert_eq!(
            params.fields,
            Some(vec!["id".to_owned(), "name".to_owned()])
        );
        assert_eq!(params.expand, None);
        assert!(!params.is_flat_shape());
    }
}
