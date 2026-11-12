#![forbid(unsafe_code)]

//! api-v1 work-items serializers/queries/handlers surface (D-18, stage 5).
//!
//! Ports `apps/api/pi_dash/api/views/{issue,github_pr,git_code_review,
//! page}.py` + `api/serializers/{issue,page}.py` for the services layer,
//! bottom-up:
//!
//! * [`shape_issue`] — `IssueSerializer` core
//!   (`api/serializers/issue.py:54-108` helpers + `:109-496`, PIDASHCONV-660,
//!   this issue; owns this `mod.rs` as the first D-18 serializer to merge).
//! * [`shape_expand_search`] — expand + issue-link + search shapes
//!   (`api/serializers/issue.py:1033-1118,1139-1203`, PIDASHCONV-665).
//! * [`queries_core`] — work-item list/detail query builders
//!   (`api/views/issue.py` get_querysets + `utils/issue_filters.py:485-654`
//!   + list ordering + paginate surface, PIDASHCONV-668).
//! * [`queries_sub`] — subresource read query builders
//!   (`api/views/issue.py` label/link/comment/activity/attachment/relation/
//!   workpad querysets + `github_pr.py`/`git_code_review.py`, PIDASHCONV-669).
//! * [`shape_relations`] — relation shapes (`IssueRelationResponse` /
//!   `Create` / `Remove` / `Show`, `RelatedIssue`;
//!   `api/serializers/issue.py:729-906`, PIDASHCONV-663).
//! * [`shape_pages`] — page serializers (`api/serializers/page.py:27-112`,
//!   PIDASHCONV-666).
//! * [`shape_social`] — comment/attachment/activity shapes
//!   (`api/serializers/issue.py:907-1032` + `:1119-1138`, PIDASHCONV-664).
//! * [`columns`] — models layer: F18-05 column-verification record +
//!   nullable-column consts (PIDASHCONV-667; D-18 owns no tables).
//! * [`tasks`] — task call-site publishers (`issue_activity` /
//!   `model_activity` / link-title crawl / page transaction+version /
//!   asset-metadata emits plus the attachment S3 offline inputs;
//!   `api/views/issue.py` `.delay` sites, `api/views/page.py:156-170`,
//!   `settings/storage.py`, PIDASHCONV-672).
//! * [`queries_search`] — search + page read queries (legacy/advanced
//!   search SQL + params + result assembly, page visibility queryset +
//!   parent validation + fetch-or-error + detail select + archive reads;
//!   `api/views/issue.py:2654-2917`, `search/issue.py:128-198`,
//!   `api/views/page.py:173-232,480-553`, PIDASHCONV-670).
//! * [`shape_links`] — link shapes (`IssueLinkCreate` / `Update` / Show,
//!   `GithubPullRequestLink`, `GitCodeReviewLink`;
//!   `api/serializers/issue.py:580-728`, PIDASHCONV-662).
//! * [`shape_labels`] — issue/label lite + workpad shapes
//!   (`api/serializers/issue.py:497-579` + `:1061-1073`, PIDASHCONV-661).
//! * Sibling issues extend this file with their own `pub mod shape_*;` /
//!   `queries_*` lines (PIDASHCONV-661…663,665…672); on rebase keep both sides,
//!   never fork a helper.
//!
//! Shared kernels (used by every shape in this module):
//!
//! * [`filter_fields`] — `api/serializers/base.py:32-70`
//!   (`BaseSerializer._filter_fields`, the `?fields=` filter).
//! * [`python_number_str`] — Python `str()` of a JSON number (every
//!   `CharField` `str()` coercion plus the `IntegerField` number arm,
//!   PIDASHCONV-758/766).
//!
//! Wiring note: the crate root declares `pub mod v1_work_items;` (seam for
//! this issue's new files); every file under this module is new.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

pub mod columns;
pub mod queries_core;
pub mod queries_search;
pub mod queries_sub;
pub mod shape_expand_search;
pub mod shape_issue;
pub mod shape_labels;
pub mod shape_links;
pub mod shape_pages;
pub mod shape_relations;
pub mod shape_social;
pub mod tasks;

/// One entry of a DRF `fields=` argument (`base.py:19-30,41-60`): either a
/// plain field name or a `{name: sub-fields}` dict entry. Query-string
/// callers (`views/base.py:213-215`: comma-split) only ever produce plain
/// names; dicts arrive only via direct construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldSpec {
    /// A plain field name (`isinstance(item, str)`, `base.py:55-56`).
    Include(String),
    /// A `{name: [...]}` dict entry: Python recurses into the sub-list
    /// (`base.py:44-49`) and raises before the key could join `allowed`.
    Nested(String, Vec<FieldSpec>),
}

/// Failure modes of [`filter_fields`], mirroring the Python raises.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilterError {
    /// `TypeError` parity (`base.py:41-49`): Python recurses into
    /// list-valued nested entries with a `Field` where the `fields` list
    /// belongs, and iterating a `Field` raises — before the allowed-list
    /// update is reached. Non-list dict values never recurse (plain
    /// includes); views pass comma-split strings, so this arm is live
    /// only for direct construction.
    #[error("nested fields= entry always raises TypeError in Python (base.py:41): {0}")]
    NestedNotSupported(String),
}

/// Port of `BaseSerializer.__init__` + `_filter_fields`
/// (`api/serializers/base.py:19-30,32-70`).
///
/// `available` is the serializer's field list in wire order; `specs` is the
/// `fields=` argument (`None` or empty keeps everything — `base.py:29-30`,
/// `if fields:` is falsy for both; the views pass `None` when `?fields=` is
/// absent or empty, `views/base.py:213-215`). Returns the kept field names in
/// wire order (Python pops non-allowed keys in place, so survivors keep their
/// relative order, `base.py:62-70`).
///
/// Unknown names are silently ignored (`base.py:67-68` pops
/// `existing - allowed`, never the reverse): `fields=["id","nope"]` keeps
/// `id`, and `fields=["relations_summary"]` — not a serializer field —
/// keeps nothing while still gating the computed blocker keys (see
/// [`shape_issue`]).
pub fn filter_fields(
    available: &[&str],
    specs: Option<&[FieldSpec]>,
) -> Result<Vec<String>, FilterError> {
    let Some(specs) = specs else {
        return Ok(available.iter().map(|name| name.to_string()).collect());
    };
    if specs.is_empty() {
        return Ok(available.iter().map(|name| name.to_string()).collect());
    }
    // Nested-dict pass first: Python iterates `fields` and recurses per
    // dict entry (`base.py:41-49`) BEFORE building `allowed` (`base.py:51+`),
    // so a nested entry raises even when an earlier plain name was fine.
    for spec in specs {
        if let FieldSpec::Nested(key, _) = spec {
            return Err(FilterError::NestedNotSupported(key.clone()));
        }
    }
    let mut allowed: Vec<&str> = Vec::with_capacity(specs.len());
    for spec in specs {
        // The nested pass above returned already; only plain names remain.
        if let FieldSpec::Include(name) = spec {
            allowed.push(name.as_str());
        }
    }
    Ok(available
        .iter()
        .filter(|name| allowed.contains(name))
        .map(|name| name.to_string())
        .collect())
}

/// Python `str()` of a JSON number, shared by every `CharField` `str()`
/// coercion and the `IntegerField` number arm in this domain
/// (PIDASHCONV-758, PIDASHCONV-766): ints render decimally
/// (arbitrary precision kept), floats via [`python_float_repr`].
/// `Number::to_string` is not enough: under `arbitrary_precision` it
/// echoes the literal's layout (`1.5e+3`, `1e-7`, `0.00001`), and without
/// it `zmij` `Display` pads no 1-digit exponent and fixes `1e-5`.
/// Overflow float literals (`1e999`) spell `inf` like Python's `float()`.
pub fn python_number_str(number: &serde_json::Number) -> String {
    if let Some(int) = number.as_i64() {
        return int.to_string();
    }
    if let Some(int) = number.as_u64() {
        return int.to_string();
    }
    if let Some(float) = number.as_f64() {
        if number.is_f64() {
            return python_float_repr(float);
        }
    }
    // Past-`u64` ints and overflow floats only survive parsing under
    // `arbitrary_precision`, which preserves the literal.
    python_literal_str(&number.to_string())
}

/// Python `str()` of a preserved number literal (see [`python_number_str`]):
/// int grammar normalizes to digits (`-0…0` is `0`), float grammar
/// overflowed to `inf` like Python's `float()`.
fn python_literal_str(text: &str) -> String {
    if text.bytes().any(|b| matches!(b, b'.' | b'e' | b'E')) {
        return if text.starts_with('-') { "-inf" } else { "inf" }.to_owned();
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return "0".to_owned();
    }
    if negative {
        return format!("-{digits}");
    }
    digits.to_owned()
}

/// Python `repr(float)` spelling for an `f64` (== `str(float)`):
/// shortest roundtrip digits (via serde), then CPython's layout rule —
/// scientific `d[.ddd]e±XX` (two-digit minimum exponent) when the
/// normalized decimal exponent is `< -4` or `>= 16`, else fixed with a
/// forced `.0` (`float_repr_style short`, `PyOS_double_to_string 'r'`).
/// `nan` is unreachable from parsed JSON but renders exactly anyway.
fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    let text = serde_json::to_string(&value).expect("finite float");
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.as_str()),
    };
    let (mantissa, exponent): (&str, i32) = match text.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, exponent.parse().expect("serde exponent")),
        None => (text, 0),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int, frac)) => (int, frac),
        None => (mantissa, ""),
    };
    let digits: String = format!("{int_part}{frac_part}");
    let point = int_part.len() as i32;
    let Some(first) = digits.find(|c| c != '0') else {
        return if negative { "-0.0" } else { "0.0" }.to_owned();
    };
    // Normalized exponent: d.ddd × 10^power.
    let power = point - first as i32 - 1 + exponent;
    let significant = &digits[first..];
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if !(-4..16).contains(&power) {
        out.push_str(&significant[..1]);
        let rest = significant[1..].trim_end_matches('0');
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push('e');
        if power < 0 {
            out.push('-');
            let digits = (-power).to_string();
            if digits.len() < 2 {
                out.push('0');
            }
            out.push_str(&digits);
        } else {
            out.push('+');
            let digits = power.to_string();
            if digits.len() < 2 {
                out.push('0');
            }
            out.push_str(&digits);
        }
        return out;
    }
    if power >= 0 {
        let int_len = power as usize + 1;
        if significant.len() >= int_len {
            out.push_str(&significant[..int_len]);
            let rest = significant[int_len..].trim_end_matches('0');
            out.push('.');
            if rest.is_empty() {
                out.push('0');
            } else {
                out.push_str(rest);
            }
        } else {
            out.push_str(significant);
            out.push_str(&"0".repeat(int_len - significant.len()));
            out.push_str(".0");
        }
        return out;
    }
    out.push_str("0.");
    out.push_str(&"0".repeat((-power - 1) as usize));
    out.push_str(significant.trim_end_matches('0'));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELDS: &[&str] = &["id", "name", "state"];

    fn includes(names: &[&str]) -> Vec<FieldSpec> {
        names
            .iter()
            .map(|name| FieldSpec::Include(name.to_string()))
            .collect()
    }

    #[test]
    fn none_and_empty_keep_everything_in_wire_order() {
        assert_eq!(
            filter_fields(FIELDS, None).expect("keeps all"),
            vec!["id", "name", "state"]
        );
        // `if fields:` is falsy for `[]` too (`base.py:29`).
        assert_eq!(
            filter_fields(FIELDS, Some(&[])).expect("keeps all"),
            vec!["id", "name", "state"]
        );
    }

    #[test]
    fn plain_subset_keeps_wire_order_and_ignores_unknowns() {
        // Reversed request order still yields wire order (pop-in-place).
        let specs = includes(&["state", "id", "nope"]);
        assert_eq!(
            filter_fields(FIELDS, Some(&specs)).expect("filters"),
            vec!["id", "state"]
        );
        // All-unknown keeps nothing (never raises).
        let specs = includes(&["relations_summary"]);
        assert!(filter_fields(FIELDS, Some(&specs))
            .expect("filters")
            .is_empty());
    }

    #[test]
    fn nested_entry_always_raises_before_allowance() {
        let specs = vec![
            FieldSpec::Include("id".to_string()),
            FieldSpec::Nested("state".to_string(), vec![]),
        ];
        assert_eq!(
            filter_fields(FIELDS, Some(&specs)),
            Err(FilterError::NestedNotSupported("state".to_string()))
        );
    }

    // The over-precise literals are deliberate: the same source text as
    // the CPython probe, so both sides parse the identical `f64`.
    #[allow(clippy::excessive_precision)]
    #[test]
    fn python_number_str_matches_cpython() {
        // `str(v)` for each JSON number, generated by CPython
        // (PIDASHCONV-758). Constructed values behave identically with
        // and without `arbitrary_precision`; the literal spellings pin
        // the parse shapes (`-0`/huge-int/overflow literals are covered
        // in the api crate, where the feature is always on).
        for (value, expected) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (12.0, "12.0"),
            (-2.5, "-2.5"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e100, "1e+100"),
            (-1e100, "-1e+100"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1.0 / 3.0, "0.3333333333333333"),
            (999999999999999.0, "999999999999999.0"),
            (1.5e-7, "1.5e-07"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (100.0, "100.0"),
            (0.5, "0.5"),
            (123.456, "123.456"),
            (1e21, "1e+21"),
            (123456789012345680.0, "1.2345678901234568e+17"),
        ] {
            let number = serde_json::Number::from_f64(value).expect("finite");
            assert_eq!(python_number_str(&number), expected, "{value}");
        }
        for (int, expected) in [
            (0i64, "0"),
            (-7i64, "-7"),
            (42i64, "42"),
            (i64::MIN, "-9223372036854775808"),
            (i64::MAX, "9223372036854775807"),
        ] {
            assert_eq!(python_number_str(&int.into()), expected);
        }
        assert_eq!(python_number_str(&u64::MAX.into()), "18446744073709551615");
        for (raw, expected) in [
            ("7", "7"),
            ("-7", "-7"),
            ("0", "0"),
            ("100.0", "100.0"),
            ("1e100", "1e+100"),
            ("1E100", "1e+100"),
            ("1.5e3", "1500.0"),
            ("1.50", "1.5"),
            ("0.00001", "1e-05"),
            ("1e-7", "1e-07"),
        ] {
            let value: serde_json::Value = serde_json::from_str(raw).expect("parses");
            assert_eq!(
                python_number_str(value.as_number().expect("number")),
                expected,
                "{raw}"
            );
        }
        assert_eq!(python_float_repr(f64::INFINITY), "inf");
        assert_eq!(python_float_repr(f64::NEG_INFINITY), "-inf");
        assert_eq!(python_float_repr(f64::NAN), "nan");
    }
}
