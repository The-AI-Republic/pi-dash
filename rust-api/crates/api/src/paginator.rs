//! Cursor paginator kernel for the Pi Dash Rust backend.
//!
//! Ports `pi_dash/utils/paginator.py` one for one: [`Cursor`] (the
//! `value:offset:is_prev` format), [`OffsetPaginator`] math,
//! [`GroupedOffsetPaginator`] / [`SubGroupedOffsetPaginator`] math and
//! grouping, and the [`PageResponse`] envelope [`BasePaginator.paginate`]
//! returns. Queryset execution stays with the domain handlers; this module
//! owns everything that is pure: parsing, window arithmetic, grouping, the
//! envelope, and the sea-query builders that pin the SQL semantics.
//!
//! Ported quirks (also listed in the PR):
//!
//! - `paginate` defaults an absent cursor to `f"{per_page}:0:0"`, while each
//!   paginator's own default is `Cursor(0, 0, 0)`. [`Cursor::default_for`]
//!   is the former.
//! - Grouped paginators stride by `cursor.value`, not by `limit`
//!   (`offset = page * cursor.value`); after a `per_page` change mid-walk
//!   pages shift. [`grouped_window`] keeps the stride.
//! - [`OffsetPaginator`] slices results to `limit`; the grouped paginators
//!   do not (their `CursorResult` wraps the whole window).
//! - The grouped non-m2m grouper guards with `in` and silently skips rows
//!   whose group value is not in `group_by_fields`. Only the sub-grouped
//!   plain grouper indexes cells directly, so only it raises `KeyError`
//!   (500) on an undeclared group. Here that is [`GroupError::UnknownGroup`].
//! - Group totals add `1 if count == 0 else count`, so an empty group counts
//!   as one. Sub-group totals do not (plain overwrite).
//! - m2m `group_ids` come from `list(set)` — nondeterministic across Python
//!   processes (`PYTHONHASHSEED`). The port sorts them: deterministic, and
//!   equal to one of Python's possible orders.
//! - `per_page <= 0` is accepted by the parser (Python `int()` takes it) and
//!   only fails later: `per_page = 0` divides by zero in `max_hits`
//!   ([`PageError::ZeroLimit`], a 500 like Python's `ZeroDivisionError`).

use sea_query::{
    Alias, Asterisk, Expr, Func, IntoColumnRef, NullOrdering, Order, OverStatement, Query,
    SelectStatement, SimpleExpr, WindowStatement,
};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error as ThisError;

/// `MAX_LIMIT = 1000`.
pub const MAX_LIMIT: i64 = 1000;

/// The cursor value: an int page-size, or a float when the raw text contains
/// a `.` (`float(bits[0]) if "." in bits[0] else int(bits[0])`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CursorValue {
    Int(i64),
    Float(f64),
}

impl CursorValue {
    /// Numeric comparison used by the `cursor.value != limit` back-slice rule.
    pub fn equals_limit(self, limit: i64) -> bool {
        match self {
            CursorValue::Int(v) => v == limit,
            CursorValue::Float(f) => f == limit as f64,
        }
    }
}

/// `Cursor(value, offset, is_prev, has_results)`. `offset` is the page
/// number; `value` is the page size the cursor was issued for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    pub value: CursorValue,
    pub offset: i64,
    pub is_prev: bool,
    pub has_results: Option<bool>,
}

impl Cursor {
    pub fn new(value: CursorValue, offset: i64, is_prev: bool, has_results: bool) -> Self {
        Self {
            value,
            offset,
            is_prev,
            has_results: Some(has_results),
        }
    }

    /// The `paginate` default for an absent cursor: `f"{per_page}:0:0"`.
    /// `has_results` is unset, so the cursor is falsy — as in Python, where
    /// `from_string` never sets it.
    pub fn default_for(per_page: i64) -> Self {
        Self {
            value: CursorValue::Int(per_page),
            offset: 0,
            is_prev: false,
            has_results: None,
        }
    }

    /// `bool(cursor)` is `bool(self.has_results)`: unset means falsy.
    pub fn has_results_or_false(self) -> bool {
        self.has_results.unwrap_or(false)
    }

    /// `Cursor.from_string`: exactly three `:`-separated parts.
    pub fn from_string(raw: &str) -> Result<Self, PageError> {
        let bits: Vec<&str> = raw.split(':').collect();
        if bits.len() != 3 {
            return Err(PageError::InvalidCursor);
        }
        let value = if bits[0].contains('.') {
            parse_py_float(bits[0]).map(CursorValue::Float)?
        } else {
            parse_py_int(bits[0]).map(CursorValue::Int)?
        };
        let offset = parse_py_int(bits[1])?;
        let is_prev = parse_py_int(bits[2])? != 0;
        Ok(Self {
            value,
            offset,
            is_prev,
            has_results: None,
        })
    }
}

/// Python `str(float)`: shortest round-trip digits, `".0"` for integral
/// values, and scientific notation with a signed two-digit exponent once
/// the decimal exponent reaches 16 or drops below -4 (`1e+16`, `1e-05`).
pub fn py_float_str(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "inf".to_owned()
        } else {
            "-inf".to_owned()
        };
    }
    let plain = format!("{value}");
    let unsigned = plain.trim_start_matches('-');
    let int_len = unsigned
        .split_once('.')
        .map_or(unsigned.len(), |(head, _)| head.len());
    if value.fract() == 0.0 && int_len <= 16 {
        // Integral values render "X.0" below the scientific threshold.
        return format!("{value:.1}");
    }
    let (negative, digits) = match plain.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, plain.as_str()),
    };
    let int_len = digits
        .split_once('.')
        .map_or(digits.len(), |(head, _)| head.len());
    // Leading fractional zeros for values below 1 ("0.00001" → 4).
    let frac_zeros = if int_len == 0 || (int_len == 1 && digits.starts_with('0')) {
        digits.split_once('.').map_or(0, |(_, tail)| {
            tail.chars().take_while(|c| *c == '0').count()
        })
    } else {
        0
    };
    let exponent: Option<i32> = if int_len > 16 {
        Some(int_len as i32 - 1)
    } else if frac_zeros >= 4 {
        Some(-(frac_zeros as i32 + 1))
    } else {
        None
    };
    match exponent {
        None => plain,
        Some(exp) => {
            let sig: String = digits.chars().filter(|c| *c != '.').collect();
            let sig = sig.trim_start_matches('0');
            let mantissa = if sig.len() > 1 {
                format!("{}.{}", &sig[..1], sig[1..].trim_end_matches('0'))
            } else {
                sig.to_owned()
            };
            let mantissa = mantissa.trim_end_matches('.').to_owned();
            let exp_text = if exp.unsigned_abs() < 10 {
                format!("{exp:+03}")
            } else {
                format!("{exp:+}")
            };
            format!("{}{mantissa}e{exp_text}", if negative { "-" } else { "" })
        }
    }
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value {
            CursorValue::Int(v) => write!(f, "{v}:{}:{}", self.offset, i32::from(self.is_prev)),
            CursorValue::Float(v) => write!(
                f,
                "{}:{}:{}",
                py_float_str(v),
                self.offset,
                i32::from(self.is_prev)
            ),
        }
    }
}

/// Python `int()`: unbounded, so magnitudes past `i64` saturate instead of
/// erroring (a huge cursor offset reads an empty page, exactly like slicing
/// past the end in Python). Surrounding whitespace and `_` separators are
/// accepted, like `int()`.
fn parse_py_int(raw: &str) -> Result<i64, PageError> {
    match raw.trim().replace('_', "").parse::<i128>() {
        Ok(value) => Ok(value.clamp(i64::MIN as i128, i64::MAX as i128) as i64),
        Err(_) => Err(PageError::InvalidCursor),
    }
}

/// Python `float()`: surrounding whitespace and `_` separators are accepted.
fn parse_py_float(raw: &str) -> Result<f64, PageError> {
    raw.trim()
        .replace('_', "")
        .parse::<f64>()
        .map_err(|_| PageError::InvalidCursor)
}

/// Why pagination input was rejected. [`PageError::detail`] is the exact
/// `ParseError` detail the views raise for the 400-class variants
/// ([`PageError::InvalidPerPage`], [`PageError::PerPageTooLarge`],
/// [`PageError::InvalidCursor`], [`PageError::OffsetTooLarge`],
/// [`PageError::NegativeOffset`]). The remaining variants mirror Python
/// exceptions the views do not catch, so the handlers answer 500 for them:
/// [`PageError::ZeroLimit`] is `ZeroDivisionError` from `max_hits`,
/// [`PageError::NegativeSlice`] is the `ValueError` Django raises when a
/// lazy queryset is sliced with a negative bound, [`PageError::NonFiniteCursor`]
/// is the `ValueError`/`OverflowError` from `int()` field prep on a
/// non-finite cursor stride, and [`PageError::MissingOrderKey`] is the
/// `TypeError` from `F(*None)` in the grouped window builders.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum PageError {
    #[error("Invalid per_page parameter.")]
    InvalidPerPage,
    #[error("Invalid per_page value. Cannot exceed {0}.")]
    PerPageTooLarge(i64),
    #[error("Invalid cursor parameter.")]
    InvalidCursor,
    #[error("Error in parsing")]
    OffsetTooLarge,
    #[error("Error in parsing")]
    NegativeOffset,
    #[error("Error in parsing")]
    ZeroLimit,
    #[error("negative slicing over a lazy queryset is not supported")]
    NegativeSlice,
    #[error("cursor value is not a finite number")]
    NonFiniteCursor,
    #[error("grouped pagination requires an order key")]
    MissingOrderKey,
}

impl PageError {
    /// The exact `ParseError(detail=...)` string the Python views raise.
    pub fn detail(&self) -> String {
        self.to_string()
    }
}

/// `BasePaginator.get_per_page`: unparsable input is an error, the ceiling is
/// `max(max_per_page, default_per_page)`, and negatives pass through.
/// Python's `int()` is unbounded, so a huge magnitude parses fine and then
/// trips the ceiling (`PerPageTooLarge`), instead of failing to parse.
pub fn parse_per_page(
    raw: Option<&str>,
    default_per_page: i64,
    max_per_page: i64,
) -> Result<i64, PageError> {
    let Some(text) = raw else {
        return Ok(default_per_page);
    };
    let per_page = text
        .trim()
        .replace('_', "")
        .parse::<i128>()
        .map_err(|_| PageError::InvalidPerPage)?;
    let ceiling = max_per_page.max(default_per_page);
    if per_page > ceiling as i128 {
        return Err(PageError::PerPageTooLarge(ceiling));
    }
    Ok(per_page.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

/// `limit = min(limit, max_limit)`.
pub fn clamp_limit(limit: i64, max_limit: i64) -> i64 {
    limit.min(max_limit)
}

/// The `next` cursor: `Cursor(limit, page + 1, False, extra_rows > 0)`.
pub fn next_cursor(limit: i64, page: i64, has_more: bool) -> Cursor {
    Cursor::new(CursorValue::Int(limit), page + 1, false, has_more)
}

/// The `prev` cursor: `Cursor(limit, page - 1, True, page > 0)`.
pub fn prev_cursor(limit: i64, page: i64) -> Cursor {
    Cursor::new(CursorValue::Int(limit), page - 1, true, page > 0)
}

/// `max_hits = math.ceil(count / limit)`; zero limit is Python's
/// `ZeroDivisionError`. The division rounds toward positive infinity for
/// every sign combination, exactly like `math.ceil`.
pub fn max_hits(count: i64, limit: i64) -> Result<i64, PageError> {
    if limit == 0 {
        return Err(PageError::ZeroLimit);
    }
    let quot = count / limit;
    let rem = count % limit;
    if rem != 0 && (rem > 0) == (limit > 0) {
        Ok(quot + 1)
    } else {
        Ok(quot)
    }
}

/// Saturate an exact `i128` offset into `i64`. Python offsets are unbounded;
/// a saturated `MAX` reads an empty page (slicing past the end), exactly
/// like the true huge value would.
fn saturate_offset(value: i128) -> i64 {
    value.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// The fetch window `OffsetPaginator.get_result` reads: `[offset, stop)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OffsetWindow {
    pub page: i64,
    pub offset: i64,
    pub stop: i64,
}

pub fn offset_window(
    limit: i64,
    page: i64,
    cursor_value: CursorValue,
    is_prev: bool,
    max_offset: Option<i64>,
) -> Result<OffsetWindow, PageError> {
    let offset = saturate_offset(page as i128 * limit as i128);
    if let Some(max) = max_offset {
        if offset >= max {
            return Err(PageError::OffsetTooLarge);
        }
    }
    if offset < 0 {
        return Err(PageError::NegativeOffset);
    }
    let stop = saturate_offset(offset as i128 + limit as i128 + 1);
    if stop < 0 {
        // `queryset[offset:stop]` with a negative `stop`: Django raises
        // `ValueError` on the lazy queryset (only reachable with `limit < 0`,
        // since `offset >= 0` here).
        return Err(PageError::NegativeSlice);
    }
    if !cursor_value.equals_limit(limit) && is_prev {
        // `results[-(limit + 1):]` runs on the lazy queryset, whose negative
        // indexing raises `ValueError` — the backwards walk never returns rows.
        return Err(PageError::NegativeSlice);
    }
    Ok(OffsetWindow { page, offset, stop })
}

/// Trim `fetched` rows (the `[offset, stop)` slice the handler read) to the
/// `[:limit]` page. A negative `limit` is Django's `ValueError` again
/// (`results[:limit]` runs on the lazy queryset), so it errors.
pub fn apply_offset_window<T: Clone>(fetched: &[T], limit: i64) -> Result<Vec<T>, PageError> {
    if limit < 0 {
        return Err(PageError::NegativeSlice);
    }
    let n = (limit as usize).min(fetched.len());
    Ok(fetched[..n].to_vec())
}

/// The fetch window the grouped paginators read: strides by the cursor's own
/// value, with `(cursor.value or limit)` as the width. `0` and `0.0` are
/// falsy and fall back to `limit`; a fractional cursor value truncates toward
/// zero after the multiply (Django's `int()` field prep does the same), so
/// the server-issued integral cursors stay exact while hand-crafted fractions
/// behave like Python instead of erroring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupedWindow {
    pub page: i64,
    pub offset: i64,
    pub stop: i64,
}

/// `(cursor.value or limit)` as an exact integer stride when the value is
/// integral, or a float stride otherwise. `0`/`0.0` fall back to `limit`;
/// NaN and infinities are Django's `ValueError`/`OverflowError` from the
/// `int()` field prep.
enum Stride {
    Int(i128),
    Float(f64),
}

fn stride_or_limit(value: CursorValue, limit: i64) -> Result<Stride, PageError> {
    match value {
        CursorValue::Int(0) => Ok(Stride::Int(limit as i128)),
        CursorValue::Int(v) => Ok(Stride::Int(v as i128)),
        CursorValue::Float(0.0) => Ok(Stride::Int(limit as i128)),
        CursorValue::Float(f) if !f.is_finite() => Err(PageError::NonFiniteCursor),
        CursorValue::Float(f)
            if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 =>
        {
            Ok(Stride::Int(f as i128))
        }
        CursorValue::Float(f) => Ok(Stride::Float(f)),
    }
}

/// Truncate a float window bound toward zero (like Django's `int()` prep)
/// and saturate past-`i64` magnitudes to an empty-page offset. Overflow
/// saturation (finite inputs overflowing `f64`) matches Python's unbounded
/// ints; only a NaN bound — impossible from finite inputs — errors.
fn trunc_saturate(bound: f64) -> Result<i64, PageError> {
    if bound.is_nan() {
        return Err(PageError::NonFiniteCursor);
    }
    if bound >= i64::MAX as f64 {
        return Ok(i64::MAX);
    }
    if bound <= i64::MIN as f64 {
        return Ok(i64::MIN);
    }
    Ok(bound.trunc() as i64)
}

pub fn grouped_window(
    limit: i64,
    page: i64,
    cursor_value: CursorValue,
    max_offset: Option<i64>,
) -> Result<GroupedWindow, PageError> {
    let stride = stride_or_limit(cursor_value, limit)?;
    let offset = match stride {
        Stride::Int(stride) => saturate_offset(page as i128 * stride),
        Stride::Float(stride) => trunc_saturate(page as f64 * stride)?,
    };
    if let Some(max) = max_offset {
        if offset >= max {
            return Err(PageError::OffsetTooLarge);
        }
    }
    if offset < 0 {
        return Err(PageError::NegativeOffset);
    }
    let stop = match stride {
        Stride::Int(stride) => saturate_offset(offset as i128 + stride + 1),
        // Python adds the untruncated floats first and truncates once.
        Stride::Float(stride) => trunc_saturate(page as f64 * stride + stride + 1.0)?,
    };
    Ok(GroupedWindow { page, offset, stop })
}

/// Grouped `max_hits`: `0` for an empty window, else the ceil over the
/// largest group's filtered count.
pub fn grouped_max_hits(
    window_empty: bool,
    top_group_count: i64,
    limit: i64,
) -> Result<i64, PageError> {
    if window_empty {
        return Ok(0);
    }
    max_hits(top_group_count, limit)
}

/// The exact response `BasePaginator.paginate` returns, keys in order:
/// `grouped_by`, `sub_grouped_by`, `total_count`, `next_cursor`,
/// `prev_cursor`, `next_page_results`, `prev_page_results`, `count`,
/// `total_pages`, `total_results`, `extra_stats`, `results`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PageResponse<T: Serialize> {
    pub grouped_by: Option<String>,
    pub sub_grouped_by: Option<String>,
    pub total_count: i64,
    pub next_cursor: String,
    pub prev_cursor: String,
    pub next_page_results: bool,
    pub prev_page_results: bool,
    pub count: usize,
    pub total_pages: i64,
    pub total_results: i64,
    pub extra_stats: Option<Value>,
    pub results: T,
}

impl<T: Serialize> PageResponse<T> {
    pub fn to_json_value(&self) -> Value {
        serde_json::to_value(self).expect("PageResponse is serializable")
    }
}

// ---------------------------------------------------------------------------
// Grouping
// ---------------------------------------------------------------------------

/// `FIELD_MAPPER`: m2m group fields and the id-list key each row gains.
pub const FIELD_MAPPER: &[(&str, &str)] = &[
    ("labels__id", "label_ids"),
    ("assignees__id", "assignee_ids"),
    ("issue_module__module_id", "module_ids"),
];

/// Why grouping a window failed. Each variant is a Python exception the
/// views do not catch (`KeyError` / `AttributeError` → 500).
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum GroupError {
    #[error("row group is not a declared group: {0}")]
    UnknownGroup(String),
    #[error("row has no id")]
    MissingId,
    #[error("row has no group field: {0}")]
    MissingGroupField(String),
    #[error("group cells are malformed")]
    MalformedCells,
}

pub fn field_mapper_lookup(field: &str) -> Option<&'static str> {
    FIELD_MAPPER
        .iter()
        .find(|(key, _)| *key == field)
        .map(|(_, mapped)| *mapped)
}

/// Python `str(value)` for JSON scalars: `None` → `"None"`,
/// `True` → `"True"`, numbers render plainly, strings pass through.
pub fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// `__get_total_dict` (grouped): per-group totals where a zero count counts
/// as one (`1 if count == 0 else count`). Later duplicates accumulate.
pub fn total_dict(pairs: &[(String, i64)]) -> std::collections::HashMap<String, i64> {
    let mut totals = std::collections::HashMap::new();
    for (group, count) in pairs {
        let add = if *count == 0 { 1 } else { *count };
        *totals.entry(group.clone()).or_insert(0) += add;
    }
    totals
}

/// `__get_field_dict` (grouped): every declared group starts with empty
/// results and its total (`total_group_dict.get(str(field), 0)`).
pub fn field_dict(groups: &[String], totals: &std::collections::HashMap<String, i64>) -> Value {
    let mut map = serde_json::Map::new();
    for group in groups {
        let mut entry = serde_json::Map::new();
        entry.insert("results".to_owned(), Value::Array(Vec::new()));
        entry.insert(
            "total_results".to_owned(),
            totals
                .get(group)
                .copied()
                .map_or(Value::from(0), Value::from),
        );
        map.insert(group.clone(), Value::Object(entry));
    }
    Value::Object(map)
}

type Row = serde_json::Map<String, Value>;

/// Direct `result[field]` indexing: a missing key is Python's `KeyError`.
fn row_group_value(row: &Row, field: &str) -> Result<String, GroupError> {
    row.get(field)
        .map(py_str)
        .ok_or_else(|| GroupError::MissingGroupField(field.to_owned()))
}

/// `result.get(field)` access: a missing key reads as `None` / `"None"`.
fn row_group_value_or_none(row: &Row, field: &str) -> String {
    row.get(field)
        .map(py_str)
        .unwrap_or_else(|| "None".to_owned())
}

fn cell_results(cell: Option<&mut Value>) -> Result<&mut Vec<Value>, GroupError> {
    cell.and_then(|cell| cell.get_mut("results"))
        .and_then(Value::as_array_mut)
        .ok_or(GroupError::MalformedCells)
}

/// `__query_grouper` (grouped, non-m2m): rows land in their declared group.
/// The field reads via `.get` (missing → `"None"`); a value outside every
/// declared group is silently skipped (`if group_value in processed_results`).
pub fn query_grouper(
    rows: &[Row],
    group_field: &str,
    groups: &[String],
    totals: &std::collections::HashMap<String, i64>,
) -> Result<Value, GroupError> {
    let mut processed = match field_dict(groups, totals) {
        Value::Object(map) => map,
        _ => return Err(GroupError::MalformedCells),
    };
    for row in rows {
        let group = row_group_value_or_none(row, group_field);
        if let Some(entry) = processed.get_mut(&group) {
            cell_results(Some(entry))?.push(Value::Object(row.clone()));
        }
    }
    Ok(Value::Object(processed))
}

/// `__query_multi_grouper` (grouped, m2m): rows fan out to every group they
/// belong to, gain their `<mapped>_ids` key (`[]` when any group is `None`),
/// and repeat rows are appended once per group. Groups track only groups
/// seen in the window; a missing total renders `null`.
pub fn query_multi_grouper(
    rows: &[Row],
    group_field: &str,
    mapped_key: &str,
    totals: &std::collections::HashMap<String, i64>,
) -> Result<Value, GroupError> {
    use std::collections::{BTreeSet, HashMap};

    let mut membership: HashMap<String, BTreeSet<String>> = HashMap::new();
    for row in rows {
        let id = row.get("id").map(py_str).ok_or(GroupError::MissingId)?;
        let group = row_group_value(row, group_field)?;
        membership.entry(id).or_default().insert(group);
    }

    let mut processed: serde_json::Map<String, Value> = serde_json::Map::new();
    for row in rows {
        let id = row.get("id").map(py_str).ok_or(GroupError::MissingId)?;
        let group_ids: Vec<String> = membership
            .get(&id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut row = row.clone();
        if group_ids.iter().any(|g| g == "None") {
            row.insert(mapped_key.to_owned(), Value::Array(Vec::new()));
        } else {
            row.insert(
                mapped_key.to_owned(),
                Value::Array(group_ids.iter().map(|g| Value::from(g.clone())).collect()),
            );
        }
        for group_id in &group_ids {
            let entry = processed.entry(group_id.clone()).or_insert_with(|| {
                let mut entry = serde_json::Map::new();
                entry.insert("results".to_owned(), Value::Array(Vec::new()));
                entry.insert(
                    "total_results".to_owned(),
                    totals
                        .get(group_id)
                        .copied()
                        .map_or(Value::Null, Value::from),
                );
                Value::Object(entry)
            });
            let results = cell_results(Some(entry))?;
            let already = results
                .iter()
                .any(|existing| existing.get("id") == row.get("id"));
            if !already {
                results.push(Value::Object(row.clone()));
            }
        }
    }
    Ok(Value::Object(processed))
}

/// `process_results` for the grouped paginator: m2m group fields fan out,
/// plain fields bucket; an empty window is `{}`.
pub fn process_grouped_results(
    rows: &[Row],
    group_field: &str,
    groups: &[String],
    totals: &std::collections::HashMap<String, i64>,
) -> Result<Value, GroupError> {
    if rows.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    match field_mapper_lookup(group_field) {
        Some(mapped) => query_multi_grouper(rows, group_field, mapped, totals),
        None => query_grouper(rows, group_field, groups, totals),
    }
}

/// Sub-grouped totals: group totals keep the `1-if-zero` rule; sub-group
/// totals overwrite plainly.
pub fn sub_total_dicts(
    group_pairs: &[(String, i64)],
    sub_pairs: &[(String, String, i64)],
) -> (
    std::collections::HashMap<String, i64>,
    std::collections::HashMap<String, std::collections::HashMap<String, i64>>,
) {
    let groups = total_dict(group_pairs);
    let mut subs: std::collections::HashMap<String, std::collections::HashMap<String, i64>> =
        std::collections::HashMap::new();
    for (group, sub, count) in sub_pairs {
        subs.entry(group.clone())
            .or_default()
            .insert(sub.clone(), *count);
    }
    (groups, subs)
}

/// `__get_field_dict` (sub-grouped): for every declared group, every known
/// sub-group starts empty with its total. A group absent from the sub-totals
/// yields an empty cell (`total_sub_group_dict.get(group, [])`).
pub fn sub_field_dict(
    groups: &[String],
    totals: &std::collections::HashMap<String, i64>,
    sub_totals: &std::collections::HashMap<String, std::collections::HashMap<String, i64>>,
) -> Result<Value, GroupError> {
    let mut map = serde_json::Map::new();
    for group in groups {
        let mut results = serde_json::Map::new();
        if let Some(subs) = sub_totals.get(group) {
            for (sub, total) in subs {
                let mut entry = serde_json::Map::new();
                entry.insert("results".to_owned(), Value::Array(Vec::new()));
                entry.insert("total_results".to_owned(), Value::from(*total));
                results.insert(sub.clone(), Value::Object(entry));
            }
        }
        let mut entry = serde_json::Map::new();
        entry.insert("results".to_owned(), Value::Object(results));
        entry.insert(
            "total_results".to_owned(),
            totals
                .get(group)
                .copied()
                .map_or(Value::from(0), Value::from),
        );
        map.insert(group.clone(), Value::Object(entry));
    }
    Ok(Value::Object(map))
}

/// `__query_multi_grouper` (sub-grouped): rows land in their group/sub-group
/// cell when both are declared (anything else is silently skipped); m2m
/// sides gain their `<mapped>_ids` keys.
pub fn sub_query_multi_grouper(
    rows: &[Row],
    group_field: &str,
    sub_group_field: &str,
    mut processed: serde_json::Map<String, Value>,
) -> Result<Value, GroupError> {
    use std::collections::{BTreeSet, HashMap};

    let mut group_membership: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut sub_membership: HashMap<String, BTreeSet<String>> = HashMap::new();
    let group_mapped = field_mapper_lookup(group_field);
    let sub_mapped = field_mapper_lookup(sub_group_field);
    for row in rows {
        let id = row.get("id").map(py_str).ok_or(GroupError::MissingId)?;
        if group_mapped.is_some() {
            group_membership
                .entry(id.clone())
                .or_default()
                .insert(row_group_value(row, group_field)?);
        }
        if sub_mapped.is_some() {
            sub_membership
                .entry(id)
                .or_default()
                .insert(row_group_value(row, sub_group_field)?);
        }
    }

    for row in rows {
        // The placement loop reads via `.get` (missing → `"None"`); only
        // the membership loop above uses direct indexing.
        let group_value = row_group_value_or_none(row, group_field);
        let sub_value = row_group_value_or_none(row, sub_group_field);
        let id = row.get("id").map(py_str).ok_or(GroupError::MissingId)?;
        let cell = processed
            .get_mut(&group_value)
            .and_then(|g| g.get_mut("results"))
            .and_then(|r| r.get_mut(&sub_value));
        if let Some(cell) = cell {
            let mut row = row.clone();
            if let Some(mapped) = group_mapped {
                let ids: Vec<String> = group_membership
                    .get(&id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect();
                row.insert(
                    mapped.to_owned(),
                    if ids.iter().any(|g| g == "None") {
                        Value::Array(Vec::new())
                    } else {
                        Value::Array(ids.iter().map(|g| Value::from(g.clone())).collect())
                    },
                );
            }
            if let Some(mapped) = sub_mapped {
                let ids: Vec<String> = sub_membership
                    .get(&id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect();
                row.insert(
                    mapped.to_owned(),
                    if ids.iter().any(|g| g == "None") {
                        Value::Array(Vec::new())
                    } else {
                        Value::Array(ids.iter().map(|g| Value::from(g.clone())).collect())
                    },
                );
            }
            cell_results(Some(cell))?.push(Value::Object(row));
        }
    }
    Ok(Value::Object(processed))
}

/// `__query_grouper` (sub-grouped): fields read via `.get`, cells index
/// directly; an undeclared group or sub-group is Python's `KeyError`.
pub fn sub_query_grouper(
    rows: &[Row],
    group_field: &str,
    sub_group_field: &str,
    mut processed: serde_json::Map<String, Value>,
) -> Result<Value, GroupError> {
    for row in rows {
        let group_value = row_group_value_or_none(row, group_field);
        let sub_value = row_group_value_or_none(row, sub_group_field);
        let cell = processed
            .get_mut(&group_value)
            .and_then(|g| g.get_mut("results"))
            .and_then(|r| r.get_mut(&sub_value))
            .ok_or_else(|| GroupError::UnknownGroup(format!("{group_value}/{sub_value}")))?;
        cell_results(Some(cell))?.push(Value::Object(row.clone()));
    }
    Ok(Value::Object(processed))
}

/// `process_results` for the sub-grouped paginator.
pub fn process_sub_grouped_results(
    rows: &[Row],
    group_field: &str,
    sub_group_field: &str,
    processed: serde_json::Map<String, Value>,
) -> Result<Value, GroupError> {
    if rows.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    if field_mapper_lookup(group_field).is_some() || field_mapper_lookup(sub_group_field).is_some()
    {
        sub_query_multi_grouper(rows, group_field, sub_group_field, processed)
    } else {
        sub_query_grouper(rows, group_field, sub_group_field, processed)
    }
}

// ---------------------------------------------------------------------------
// SQL builders: the exact statement shapes the Python slicing compiles to.
// ---------------------------------------------------------------------------

/// The `ORDER BY` every paginator applies: the requested key (nulls last,
/// direction from the `-` prefix) and then `created_at` descending.
pub fn order_key(order_by: Option<&str>) -> (Option<String>, bool) {
    match order_by {
        None => (None, false),
        Some(key) => match key.strip_prefix('-') {
            Some(rest) => (Some(rest.to_owned()), true),
            None => (Some(key.to_owned()), false),
        },
    }
}

/// `queryset.order_by(key [DESC|ASC] NULLS LAST, -created_at)[offset:stop]`:
/// full rows, `LIMIT (stop - offset)` `OFFSET offset`. With `order_by=None`
/// Python skips ordering entirely (`if self.key:` is false), not even
/// `-created_at`.
pub fn offset_query(
    table: &str,
    order_by: Option<&str>,
    fetch: u64,
    offset: u64,
) -> SelectStatement {
    let (key, desc) = order_key(order_by);
    let mut query = Query::select();
    query.from(Alias::new(table)).column(Asterisk);
    if let Some(key) = key {
        query.order_by_with_nulls(
            Alias::new(key),
            if desc { Order::Desc } else { Order::Asc },
            NullOrdering::Last,
        );
        query.order_by(Alias::new("created_at"), Order::Desc);
    }
    query.limit(fetch).offset(offset);
    query
}

/// The `PARTITION BY <group[, sub-group]> ORDER BY ...` window the grouped
/// paginators annotate `ROW_NUMBER() OVER (...)` as `row_number`.
/// `partition` holds one (grouped) or two (sub-grouped) field names.
/// With `order_by=None` Python crashes unpacking the key (`F(*None)`), so
/// the builders error instead of emitting an unordered window.
pub fn partition_window(
    partition: &[&str],
    order_by: Option<&str>,
) -> Result<WindowStatement, PageError> {
    let (key, desc) = order_key(order_by);
    let Some(key) = key else {
        return Err(PageError::MissingOrderKey);
    };
    let mut window = WindowStatement::partition_by(Alias::new(partition[0]));
    for extra in &partition[1..] {
        window.add_partition_by(SimpleExpr::Column(Alias::new(*extra).into_column_ref()));
    }
    window.order_by_with_nulls(
        Alias::new(key),
        if desc { Order::Desc } else { Order::Asc },
        NullOrdering::Last,
    );
    window.order_by(Alias::new("created_at"), Order::Desc);
    Ok(window)
}

/// The grouped page: rows whose per-partition `row_number` falls in
/// `(offset, stop)`, ordered like the window. Django filters on the
/// annotation via a subquery; so does this builder.
pub fn grouped_page_query(
    table: &str,
    partition: &[&str],
    order_by: Option<&str>,
    offset: i64,
    stop: i64,
) -> Result<SelectStatement, PageError> {
    let (key, desc) = order_key(order_by);
    let Some(key) = key else {
        return Err(PageError::MissingOrderKey);
    };
    let mut inner = Query::select();
    inner
        .from(Alias::new(table))
        .column(Asterisk)
        .expr_window_as(
            Func::cust("ROW_NUMBER"),
            partition_window(partition, order_by)?,
            Alias::new("row_number"),
        );
    let mut outer = Query::select();
    outer
        .from_subquery(inner, Alias::new("paged"))
        .column(Asterisk)
        .and_where(Expr::col(Alias::new("row_number")).gt(offset))
        .and_where(Expr::col(Alias::new("row_number")).lt(stop));
    outer.order_by_with_nulls(
        Alias::new(key),
        if desc { Order::Desc } else { Order::Asc },
        NullOrdering::Last,
    );
    outer.order_by(Alias::new("created_at"), Order::Desc);
    Ok(outer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::PostgresQueryBuilder;

    fn sql(query: &SelectStatement) -> String {
        query.to_string(PostgresQueryBuilder)
    }

    // -- Cursor ------------------------------------------------------------

    #[test]
    fn cursor_display_matches_python_str() {
        assert_eq!(
            Cursor::new(CursorValue::Int(50), 0, false, true).to_string(),
            "50:0:0"
        );
        assert_eq!(
            Cursor::new(CursorValue::Int(50), 2, true, false).to_string(),
            "50:2:1"
        );
        assert_eq!(
            Cursor::new(CursorValue::Float(10.5), 0, false, false).to_string(),
            "10.5:0:0"
        );
        // Python str(10.0) is "10.0", not "10".
        assert_eq!(
            Cursor::new(CursorValue::Float(10.0), 1, false, true).to_string(),
            "10.0:1:0"
        );
    }

    #[test]
    fn float_formatting_matches_python_repr() {
        assert_eq!(py_float_str(10.0), "10.0");
        assert_eq!(py_float_str(10.5), "10.5");
        assert_eq!(py_float_str(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(py_float_str(1e16), "1e+16");
        assert_eq!(py_float_str(1e-5), "1e-05");
        assert_eq!(py_float_str(f64::INFINITY), "inf");
        assert_eq!(py_float_str(f64::NEG_INFINITY), "-inf");
        assert_eq!(py_float_str(f64::NAN), "nan");
    }

    #[test]
    fn negative_limit_is_a_server_error_like_python() {
        // Oracle: `results[:limit]` with a negative limit runs on the lazy
        // queryset, so Django raises `ValueError` — no slice-trim happens.
        let fetched: Vec<i64> = vec![1, 2, 3, 4, 5];
        assert_eq!(
            apply_offset_window(&fetched, -2),
            Err(PageError::NegativeSlice)
        );
        assert_eq!(apply_offset_window(&fetched, 2), Ok(vec![1, 2]));
        assert_eq!(apply_offset_window(&fetched, 99), Ok(vec![1, 2, 3, 4, 5]));
    }

    #[test]
    fn cursor_from_string_parses_and_round_trips() {
        let cursor = Cursor::from_string("50:3:1").unwrap();
        assert_eq!(cursor.value, CursorValue::Int(50));
        assert_eq!(cursor.offset, 3);
        assert!(cursor.is_prev);
        // from_string never sets has_results, so the cursor is falsy.
        assert!(!cursor.has_results_or_false());

        let float = Cursor::from_string("10.5:0:0").unwrap();
        assert_eq!(float.value, CursorValue::Float(10.5));

        // Oracle: `int()` never overflows, so a huge offset saturates to an
        // empty-page bound instead of rejecting the cursor.
        let huge = Cursor::from_string("99999999999999999999999:0:0").unwrap();
        assert_eq!(huge.value, CursorValue::Int(i64::MAX));
        let huge_neg = Cursor::from_string("-99999999999999999999999:0:0").unwrap();
        assert_eq!(huge_neg.value, CursorValue::Int(i64::MIN));

        for bad in ["50:0", "50:0:0:0", "x:0:0", "50:x:0", "50:0:x", ""] {
            assert_eq!(
                Cursor::from_string(bad),
                Err(PageError::InvalidCursor),
                "{bad}"
            );
        }
    }

    #[test]
    fn cursor_default_uses_per_page_as_value() {
        let cursor = Cursor::default_for(100);
        assert_eq!(cursor.to_string(), "100:0:0");
        assert!(!cursor.has_results_or_false());
    }

    // -- per_page ----------------------------------------------------------

    #[test]
    fn per_page_parsing_matches_get_per_page() {
        assert_eq!(parse_per_page(None, 100, 1000), Ok(100));
        assert_eq!(parse_per_page(Some("50"), 100, 1000), Ok(50));
        assert_eq!(
            parse_per_page(Some("x"), 100, 1000),
            Err(PageError::InvalidPerPage)
        );
        assert_eq!(
            parse_per_page(Some("2000"), 100, 1000),
            Err(PageError::PerPageTooLarge(1000))
        );
        // Ceiling is max(max_per_page, default_per_page).
        assert_eq!(parse_per_page(Some("1500"), 2000, 1000), Ok(1500));
        // Oracle: `int()` is unbounded, so a huge magnitude parses fine and
        // then trips the ceiling — it is `PerPageTooLarge`, not unparsable.
        assert_eq!(
            parse_per_page(Some("99999999999999999999999"), 100, 1000),
            Err(PageError::PerPageTooLarge(1000))
        );
        // Negatives pass the parser.
        assert_eq!(parse_per_page(Some("-5"), 100, 1000), Ok(-5));
        assert_eq!(
            PageError::PerPageTooLarge(1000).detail(),
            "Invalid per_page value. Cannot exceed 1000."
        );
        assert_eq!(
            PageError::InvalidPerPage.detail(),
            "Invalid per_page parameter."
        );
        assert_eq!(
            PageError::InvalidCursor.detail(),
            "Invalid cursor parameter."
        );
        assert_eq!(PageError::OffsetTooLarge.detail(), "Error in parsing");
    }

    // -- offset window -----------------------------------------------------

    #[test]
    fn offset_window_first_page() {
        let window = offset_window(50, 0, CursorValue::Int(50), false, None).unwrap();
        assert_eq!(
            window,
            OffsetWindow {
                page: 0,
                offset: 0,
                stop: 51,
            }
        );
        assert_eq!(next_cursor(50, 0, true).to_string(), "50:1:0");
        assert_eq!(prev_cursor(50, 0).to_string(), "50:-1:1");
        assert!(!prev_cursor(50, 0).has_results_or_false());
    }

    #[test]
    fn offset_window_back_walk_is_a_server_error() {
        // Oracle: `results[-(limit + 1):]` runs on the lazy queryset, whose
        // negative indexing raises `ValueError` — the walk never returns rows.
        assert_eq!(
            offset_window(50, 2, CursorValue::Int(25), true, None),
            Err(PageError::NegativeSlice)
        );
        assert_eq!(
            offset_window(50, 2, CursorValue::Float(25.0), true, None),
            Err(PageError::NegativeSlice)
        );
        // A matching value walks forward normally.
        let window = offset_window(50, 2, CursorValue::Int(50), true, None).unwrap();
        assert_eq!(window.offset, 100);
        assert_eq!(window.stop, 151);
        let fetched: Vec<i64> = (0..51).collect();
        assert_eq!(apply_offset_window(&fetched, 50).unwrap().len(), 50);
    }

    #[test]
    fn offset_window_errors() {
        assert_eq!(
            offset_window(10, 5, CursorValue::Int(10), false, Some(50)),
            Err(PageError::OffsetTooLarge)
        );
        assert_eq!(
            offset_window(10, -1, CursorValue::Int(10), false, None),
            Err(PageError::NegativeOffset)
        );
        // A negative stop (`limit < 0` with a non-negative offset) slices
        // with a negative bound: Django `ValueError`, not rows.
        assert_eq!(
            offset_window(-5, 0, CursorValue::Int(-5), false, None),
            Err(PageError::NegativeSlice)
        );
        assert_eq!(max_hits(95, 50), Ok(2));
        assert_eq!(max_hits(100, 50), Ok(2));
        assert_eq!(max_hits(0, 50), Ok(0));
        assert_eq!(max_hits(10, 0), Err(PageError::ZeroLimit));
    }

    #[test]
    fn offset_window_saturates_huge_products_to_an_empty_page() {
        // Oracle: Python ints never overflow; a huge offset slices past the
        // end and reads an empty 200 page.
        let window =
            offset_window(1000, i64::MAX, CursorValue::Int(i64::MAX), false, None).unwrap();
        assert_eq!(window.offset, i64::MAX);
        assert_eq!(window.stop, i64::MAX);
    }

    // -- grouped window ----------------------------------------------------

    #[test]
    fn grouped_window_strides_by_cursor_value() {
        // First page: cursor.value is 0, so width falls back to limit.
        let first = grouped_window(50, 0, CursorValue::Int(0), None).unwrap();
        assert_eq!(
            first,
            GroupedWindow {
                page: 0,
                offset: 0,
                stop: 51
            }
        );
        // Later pages stride by the cursor's own value, not the limit.
        let later = grouped_window(25, 2, CursorValue::Int(50), None).unwrap();
        assert_eq!(
            later,
            GroupedWindow {
                page: 2,
                offset: 100,
                stop: 151
            }
        );
        assert_eq!(grouped_max_hits(true, 999, 50), Ok(0));
        assert_eq!(grouped_max_hits(false, 95, 50), Ok(2));
    }

    #[test]
    fn grouped_window_float_stride_matches_python_or_semantics() {
        // Oracle: `(cursor.value or limit)` — `0.0` is falsy and falls back
        // to `limit`, exactly like `0`.
        let zero_float = grouped_window(50, 2, CursorValue::Float(0.0), None).unwrap();
        assert_eq!(zero_float.offset, 100);
        assert_eq!(zero_float.stop, 151);
        // Oracle: finite fractions truncate toward zero after the multiply
        // (Django's `int()` field prep), they do not error.
        let frac = grouped_window(50, 2, CursorValue::Float(10.5), None).unwrap();
        assert_eq!(frac.offset, 21);
        assert_eq!(frac.stop, 32);
        // Oracle: NaN poisons the multiply (`int()` raises `ValueError`) and
        // infinities overflow it (`OverflowError`) — both are 500s.
        for bad in [
            CursorValue::Float(f64::NAN),
            CursorValue::Float(f64::INFINITY),
            CursorValue::Float(f64::NEG_INFINITY),
        ] {
            assert_eq!(
                grouped_window(50, 0, bad, None),
                Err(PageError::NonFiniteCursor),
                "{bad:?}"
            );
        }
        // Oracle: a huge integral stride is a huge offset, i.e. an empty page.
        let huge = grouped_window(50, 1, CursorValue::Float(1e300), None).unwrap();
        assert_eq!(huge.offset, i64::MAX);
        assert_eq!(huge.stop, i64::MAX);
    }

    // -- envelope ----------------------------------------------------------

    #[test]
    fn envelope_has_all_twelve_keys_in_order() {
        let response = PageResponse {
            grouped_by: None,
            sub_grouped_by: None,
            total_count: 7,
            next_cursor: "50:1:0".to_owned(),
            prev_cursor: "50:-1:1".to_owned(),
            next_page_results: true,
            prev_page_results: false,
            count: 7,
            total_pages: 1,
            total_results: 7,
            extra_stats: None,
            results: vec![1, 2],
        };
        let rendered = serde_json::to_string(&response).unwrap();
        assert_eq!(
            rendered,
            r#"{"grouped_by":null,"sub_grouped_by":null,"total_count":7,"next_cursor":"50:1:0","prev_cursor":"50:-1:1","next_page_results":true,"prev_page_results":false,"count":7,"total_pages":1,"total_results":7,"extra_stats":null,"results":[1,2]}"#
        );
    }

    // -- grouping ----------------------------------------------------------

    fn row(pairs: &[(&str, Value)]) -> Row {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn plain_grouper_buckets_and_totals() {
        let rows = vec![
            row(&[
                ("id", Value::from("a")),
                ("state__group", Value::from("backlog")),
            ]),
            row(&[
                ("id", Value::from("b")),
                ("state__group", Value::from("started")),
            ]),
        ];
        let mut totals = std::collections::HashMap::new();
        totals.insert("backlog".to_owned(), 3);
        totals.insert("started".to_owned(), 1);
        let out = query_grouper(
            &rows,
            "state__group",
            &[
                "backlog".to_owned(),
                "started".to_owned(),
                "cancelled".to_owned(),
            ],
            &totals,
        )
        .unwrap();
        assert_eq!(out["backlog"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(out["backlog"]["total_results"], Value::from(3));
        assert_eq!(out["cancelled"]["results"].as_array().unwrap().len(), 0);
        // Missing totals default to 0 (field_dict rule).
        assert_eq!(out["cancelled"]["total_results"], Value::from(0));
    }

    #[test]
    fn plain_grouper_skips_undeclared_groups() {
        // `paginator.py` guards with `if group_value in processed_results`:
        // rows outside every declared group vanish silently (only the
        // sub-grouped plain grouper indexes cells directly and raises).
        let rows = vec![
            row(&[
                ("id", Value::from("a")),
                ("state__group", Value::from("zzz")),
            ]),
            row(&[
                ("id", Value::from("b")),
                ("state__group", Value::from("backlog")),
            ]),
        ];
        let out = query_grouper(
            &rows,
            "state__group",
            &["backlog".to_owned()],
            &std::collections::HashMap::new(),
        )
        .unwrap();
        assert!(out.get("zzz").is_none());
        assert_eq!(out["backlog"]["results"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn total_dict_zero_counts_as_one() {
        let totals = total_dict(&[("a".to_owned(), 0), ("b".to_owned(), 4)]);
        assert_eq!(totals["a"], 1);
        assert_eq!(totals["b"], 4);
    }

    #[test]
    fn multi_grouper_fans_out_and_sets_id_keys() {
        let rows = vec![
            row(&[("id", Value::from("a")), ("labels__id", Value::from("l1"))]),
            row(&[("id", Value::from("a")), ("labels__id", Value::from("l2"))]),
            row(&[("id", Value::from("b")), ("labels__id", Value::Null)]),
        ];
        let mut totals = std::collections::HashMap::new();
        totals.insert("l1".to_owned(), 1);
        totals.insert("l2".to_owned(), 1);
        let out = query_multi_grouper(&rows, "labels__id", "label_ids", &totals).unwrap();
        assert_eq!(out["l1"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(out["l2"]["results"].as_array().unwrap().len(), 1);
        // "None" group: the row gains an empty id list.
        assert_eq!(out["None"]["results"][0]["label_ids"], Value::Array(vec![]));
        // Sorted (deterministic) id lists.
        assert_eq!(
            out["l1"]["results"][0]["label_ids"],
            Value::Array(vec![Value::from("l1"), Value::from("l2")])
        );
        // A group with no total renders null.
        assert_eq!(out["None"]["total_results"], Value::Null);
    }

    #[test]
    fn grouped_process_results_empty_is_empty_object() {
        let out =
            process_grouped_results(&[], "state__group", &[], &std::collections::HashMap::new())
                .unwrap();
        assert_eq!(out, Value::Object(serde_json::Map::new()));
    }

    #[test]
    fn sub_field_dict_missing_group_is_an_empty_cell() {
        // `total_sub_group_dict.get(group, [])`: a group with no sub-totals
        // renders an empty results object and total 0, not an error.
        let out = sub_field_dict(
            &["g".to_owned()],
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
        )
        .unwrap();
        assert_eq!(out["g"]["results"], Value::Object(serde_json::Map::new()));
        assert_eq!(out["g"]["total_results"], Value::from(0));
    }

    #[test]
    fn sub_grouper_cells_rows() {
        let mut totals = std::collections::HashMap::new();
        totals.insert("g".to_owned(), 2);
        let mut sub_totals = std::collections::HashMap::new();
        sub_totals.insert("g".to_owned(), [("s".to_owned(), 2)].into_iter().collect());
        let processed = match sub_field_dict(&["g".to_owned()], &totals, &sub_totals).unwrap() {
            Value::Object(map) => map,
            _ => unreachable!(),
        };
        let rows = vec![row(&[
            ("id", Value::from("a")),
            ("state__group", Value::from("g")),
            ("priority", Value::from("s")),
        ])];
        let out =
            process_sub_grouped_results(&rows, "state__group", "priority", processed).unwrap();
        assert_eq!(
            out["g"]["results"]["s"]["results"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // The plain sub-grouper indexes cells directly: an undeclared
        // group is Python's KeyError.
        let rows = vec![row(&[
            ("id", Value::from("a")),
            ("state__group", Value::from("zzz")),
            ("priority", Value::from("s")),
        ])];
        let processed = match sub_field_dict(&["g".to_owned()], &totals, &sub_totals).unwrap() {
            Value::Object(map) => map,
            _ => unreachable!(),
        };
        let err =
            process_sub_grouped_results(&rows, "state__group", "priority", processed).unwrap_err();
        assert_eq!(err, GroupError::UnknownGroup("zzz/s".to_owned()));
        // The m2m sub-grouper instead guards with `in` and skips silently.
        let rows = vec![row(&[
            ("id", Value::from("a")),
            ("labels__id", Value::from("zzz")),
            ("priority", Value::from("s")),
        ])];
        let processed = match sub_field_dict(&["g".to_owned()], &totals, &sub_totals).unwrap() {
            Value::Object(map) => map,
            _ => unreachable!(),
        };
        let out = process_sub_grouped_results(&rows, "labels__id", "priority", processed).unwrap();
        assert_eq!(
            out["g"]["results"]["s"]["results"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    // -- SQL ---------------------------------------------------------------

    #[test]
    fn offset_sql_orders_nulls_last_and_slices() {
        assert_eq!(
            sql(&offset_query("issue", Some("-sequence_id"), 51, 100)),
            r#"SELECT * FROM "issue" ORDER BY "sequence_id" DESC NULLS LAST, "created_at" DESC LIMIT 51 OFFSET 100"#
        );
        // Oracle: `order_by=None` skips ordering entirely (`if self.key:`
        // is false) — not even `-created_at`.
        assert_eq!(
            sql(&offset_query("issue", None, 51, 0)),
            r#"SELECT * FROM "issue" LIMIT 51 OFFSET 0"#
        );
    }

    #[test]
    fn grouped_sql_partitions_and_bounds_rows() {
        let rendered =
            sql(
                &grouped_page_query("issue", &["state__group"], Some("-sequence_id"), 100, 151)
                    .unwrap(),
            );
        assert!(rendered.contains(r#"ROW_NUMBER() OVER ( PARTITION BY "state__group" ORDER BY "sequence_id" DESC NULLS LAST, "created_at" DESC ) AS "row_number""#), "{rendered}");
        assert!(
            rendered.contains(r#""row_number" > 100 AND "row_number" < 151"#),
            "{rendered}"
        );
        // Oracle: the grouped builders unpack the key (`F(*None)`), so
        // `order_by=None` is a `TypeError` — a 500, not an unordered window.
        assert_eq!(
            grouped_page_query("issue", &["state__group"], None, 100, 151),
            Err(PageError::MissingOrderKey)
        );
        assert_eq!(
            partition_window(&["state__group"], None),
            Err(PageError::MissingOrderKey)
        );
    }
}
