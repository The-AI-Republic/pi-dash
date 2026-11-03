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

/// Longest digit run `int()` accepts: `sys.get_int_max_str_digits()` is 4300
/// on the backend (Python 3.12, no override anywhere in the repo), so a
/// longer digit run raises `ValueError` exactly like a misspelling. Only
/// digit characters count — sign, `_` separators and surrounding whitespace
/// do not (all probed).
const MAX_INT_DIGITS: usize = 4300;

/// Starts of the Unicode decimal-digit runs `int()` accepts: every code point
/// `c` with `start <= c < start + 10` reads as digit `c - start`. Generated
/// from CPython `unicodedata.decimal` (Unicode 15.0.0, the tables backend
/// Python 3.12 ships): each run verified a contiguous 0-9 block. (Mirrors the
/// `paginator` kernel, which this crate cannot import.)
const DECIMAL_DIGIT_RUN_STARTS: &[u32] = &[
    0x00030, // U+0030..U+0039 DIGIT
    0x00660, // U+0660..U+0669 ARABIC-INDIC DIGIT
    0x006F0, // U+06F0..U+06F9 EXTENDED ARABIC-INDIC DIGIT
    0x007C0, // U+07C0..U+07C9 NKO DIGIT
    0x00966, // U+0966..U+096F DEVANAGARI DIGIT
    0x009E6, // U+09E6..U+09EF BENGALI DIGIT
    0x00A66, // U+0A66..U+0A6F GURMUKHI DIGIT
    0x00AE6, // U+0AE6..U+0AEF GUJARATI DIGIT
    0x00B66, // U+0B66..U+0B6F ORIYA DIGIT
    0x00BE6, // U+0BE6..U+0BEF TAMIL DIGIT
    0x00C66, // U+0C66..U+0C6F TELUGU DIGIT
    0x00CE6, // U+0CE6..U+0CEF KANNADA DIGIT
    0x00D66, // U+0D66..U+0D6F MALAYALAM DIGIT
    0x00DE6, // U+0DE6..U+0DEF SINHALA LITH DIGIT
    0x00E50, // U+0E50..U+0E59 THAI DIGIT
    0x00ED0, // U+0ED0..U+0ED9 LAO DIGIT
    0x00F20, // U+0F20..U+0F29 TIBETAN DIGIT
    0x01040, // U+1040..U+1049 MYANMAR DIGIT
    0x01090, // U+1090..U+1099 MYANMAR SHAN DIGIT
    0x017E0, // U+17E0..U+17E9 KHMER DIGIT
    0x01810, // U+1810..U+1819 MONGOLIAN DIGIT
    0x01946, // U+1946..U+194F LIMBU DIGIT
    0x019D0, // U+19D0..U+19D9 NEW TAI LUE DIGIT
    0x01A80, // U+1A80..U+1A89 TAI THAM HORA DIGIT
    0x01A90, // U+1A90..U+1A99 TAI THAM THAM DIGIT
    0x01B50, // U+1B50..U+1B59 BALINESE DIGIT
    0x01BB0, // U+1BB0..U+1BB9 SUNDANESE DIGIT
    0x01C40, // U+1C40..U+1C49 LEPCHA DIGIT
    0x01C50, // U+1C50..U+1C59 OL CHIKI DIGIT
    0x0A620, // U+A620..U+A629 VAI DIGIT
    0x0A8D0, // U+A8D0..U+A8D9 SAURASHTRA DIGIT
    0x0A900, // U+A900..U+A909 KAYAH LI DIGIT
    0x0A9D0, // U+A9D0..U+A9D9 JAVANESE DIGIT
    0x0A9F0, // U+A9F0..U+A9F9 MYANMAR TAI LAING DIGIT
    0x0AA50, // U+AA50..U+AA59 CHAM DIGIT
    0x0ABF0, // U+ABF0..U+ABF9 MEETEI MAYEK DIGIT
    0x0FF10, // U+FF10..U+FF19 FULLWIDTH DIGIT
    0x104A0, // U+104A0..U+104A9 OSMANYA DIGIT
    0x10D30, // U+10D30..U+10D39 HANIFI ROHINGYA DIGIT
    0x11066, // U+11066..U+1106F BRAHMI DIGIT
    0x110F0, // U+110F0..U+110F9 SORA SOMPENG DIGIT
    0x11136, // U+11136..U+1113F CHAKMA DIGIT
    0x111D0, // U+111D0..U+111D9 SHARADA DIGIT
    0x112F0, // U+112F0..U+112F9 KHUDAWADI DIGIT
    0x11450, // U+11450..U+11459 NEWA DIGIT
    0x114D0, // U+114D0..U+114D9 TIRHUTA DIGIT
    0x11650, // U+11650..U+11659 MODI DIGIT
    0x116C0, // U+116C0..U+116C9 TAKRI DIGIT
    0x11730, // U+11730..U+11739 AHOM DIGIT
    0x118E0, // U+118E0..U+118E9 WARANG CITI DIGIT
    0x11950, // U+11950..U+11959 DIVES AKURU DIGIT
    0x11C50, // U+11C50..U+11C59 BHAIKSUKI DIGIT
    0x11D50, // U+11D50..U+11D59 MASARAM GONDI DIGIT
    0x11DA0, // U+11DA0..U+11DA9 GUNJALA GONDI DIGIT
    0x11F50, // U+11F50..U+11F59 KAWI DIGIT
    0x16A60, // U+16A60..U+16A69 MRO DIGIT
    0x16AC0, // U+16AC0..U+16AC9 TANGSA DIGIT
    0x16B50, // U+16B50..U+16B59 PAHAWH HMONG DIGIT
    0x1D7CE, // U+1D7CE..U+1D7D7 MATHEMATICAL BOLD DIGIT
    0x1D7D8, // U+1D7D8..U+1D7E1 MATHEMATICAL DOUBLE-STRUCK DIGIT
    0x1D7E2, // U+1D7E2..U+1D7EB MATHEMATICAL SANS-SERIF DIGIT
    0x1D7EC, // U+1D7EC..U+1D7F5 MATHEMATICAL SANS-SERIF BOLD DIGIT
    0x1D7F6, // U+1D7F6..U+1D7FF MATHEMATICAL MONOSPACE DIGIT
    0x1E140, // U+1E140..U+1E149 NYIAKENG PUACHUE HMONG DIGIT
    0x1E2F0, // U+1E2F0..U+1E2F9 WANCHO DIGIT
    0x1E4F0, // U+1E4F0..U+1E4F9 NAG MUNDARI DIGIT
    0x1E950, // U+1E950..U+1E959 ADLAM DIGIT
    0x1FBF0, // U+1FBF0..U+1FBF9 SEGMENTED DIGIT
];

/// The decimal value of `c` when CPython reads it as a digit in `int()`,
/// else `None`. Covers ASCII `0-9` plus every Unicode decimal run (`١٠`,
/// `１０`, `१०`, ...); other numerics (`²`, `½`, `Ⅻ`) are `None`.
fn decimal_digit_value(c: char) -> Option<u32> {
    if c.is_ascii_digit() {
        return Some(c as u32 - '0' as u32);
    }
    let cp = c as u32;
    DECIMAL_DIGIT_RUN_STARTS
        .iter()
        .find(|start| cp >= **start && cp - **start < 10)
        .map(|start| cp - *start)
}

/// A successfully spelled `int()`: the exact value when it fits `i128`, else
/// its overflow class. Huge positives trip the ceiling; huge negatives fall
/// through to the `i64` clamp, like any negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PyInt {
    Small(i128),
    HugePositive,
    HugeNegative,
}

/// CPython `int(text)`: surrounding `White_Space` stripped (exactly Rust
/// `trim()`), one optional ASCII sign, PEP-515 `_` separators strictly
/// between digits, ASCII + Unicode decimal digits, at most [`MAX_INT_DIGITS`]
/// digit characters. Any `ValueError` spelling is `None`.
fn parse_py_int_value(raw: &str) -> Option<PyInt> {
    let trimmed = raw.trim();
    let (negative, body) = match trimmed.strip_prefix(['+', '-']) {
        Some(rest) => (trimmed.starts_with('-'), rest),
        None => (false, trimmed),
    };
    if body.is_empty() {
        return None;
    }
    let chars: Vec<char> = body.chars().collect();
    let mut digits: Vec<u32> = Vec::with_capacity(chars.len());
    let mut prev_is_digit = false;
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '_' {
            // PEP 515: `_` only between two digits (`1_0` ok; `1_`, `_1`,
            // `1__0` raise). Either neighbor may be a Unicode decimal digit.
            let next_is_digit = chars
                .get(index + 1)
                .is_some_and(|next| decimal_digit_value(*next).is_some());
            if !prev_is_digit || !next_is_digit {
                return None;
            }
            prev_is_digit = false;
            continue;
        }
        let digit = decimal_digit_value(*ch)?;
        digits.push(digit);
        prev_is_digit = true;
    }
    if digits.is_empty() || digits.len() > MAX_INT_DIGITS {
        return None;
    }
    let mut magnitude: i128 = 0;
    for digit in digits {
        match magnitude
            .checked_mul(10)
            .and_then(|scaled| scaled.checked_add(digit as i128))
        {
            Some(value) => magnitude = value,
            None => {
                return Some(if negative {
                    PyInt::HugeNegative
                } else {
                    PyInt::HugePositive
                });
            }
        }
    }
    Some(PyInt::Small(if negative { -magnitude } else { magnitude }))
}

/// `BasePaginator.get_per_page`: non-integers and over-max values raise.
/// Python's `int()` reads at most [`MAX_INT_DIGITS`] digits (`ValueError`
/// past that, like any misspelling); below the cap it is unbounded, so a
/// huge magnitude parses fine and then trips the ceiling instead of failing
/// to parse.
pub fn parse_per_page(raw: Option<&str>) -> Result<i64, ParamError> {
    const MAX: i64 = ListParams::DEFAULT_PER_PAGE;
    let Some(text) = raw else {
        return Ok(ListParams::DEFAULT_PER_PAGE);
    };
    let ceiling = || ParamError::detail(format!("Invalid per_page value. Cannot exceed {MAX}."));
    match parse_py_int_value(text) {
        None => Err(ParamError::detail("Invalid per_page parameter.")),
        Some(PyInt::HugePositive) => Err(ceiling()),
        Some(PyInt::HugeNegative) => Ok(i64::MIN),
        Some(PyInt::Small(per_page)) => {
            if per_page > MAX as i128 {
                return Err(ceiling());
            }
            Ok(per_page.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
        }
    }
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
    fn per_page_spelling_matches_cpython_int() {
        // PEP 515: `_` only strictly between digits (mirrors the `paginator`
        // kernel, which this crate cannot import).
        assert_eq!(parse_per_page(Some("1_0")), Ok(10));
        for bad in ["1_", "_1", "1__0", "100_"] {
            assert_eq!(
                parse_per_page(Some(bad)).unwrap_err().body(),
                r#"{"detail":"Invalid per_page parameter."}"#,
                "{bad}"
            );
        }
        // Unicode decimal digits read as their values.
        assert_eq!(parse_per_page(Some("١٠")), Ok(10));
        assert_eq!(parse_per_page(Some("１２")), Ok(12));
    }

    #[test]
    fn per_page_digit_cap_matches_backend_limit() {
        // 39..4300 digits parse, then trip the ceiling; past the backend's
        // 4300-digit `int()` cap the spelling itself raises.
        for huge in ["9".repeat(39), "9".repeat(100), "9".repeat(4300)] {
            assert_eq!(
                parse_per_page(Some(&huge)).unwrap_err().body(),
                r#"{"detail":"Invalid per_page value. Cannot exceed 1000."}"#,
                "{} digits",
                huge.len()
            );
        }
        for capped in ["9".repeat(4301), "9".repeat(5000)] {
            assert_eq!(
                parse_per_page(Some(&capped)).unwrap_err().body(),
                r#"{"detail":"Invalid per_page parameter."}"#,
                "{} digits",
                capped.len()
            );
        }
        assert_eq!(
            parse_per_page(Some(&format!("-{}", "9".repeat(100)))),
            Ok(i64::MIN)
        );
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
