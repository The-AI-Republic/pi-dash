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
//! * Sibling issues extend this file with their own `pub mod shape_*;` /
//!   `queries_*` lines (PIDASHCONV-661…663,665…672); on rebase keep both sides,
//!   never fork a helper.
//!
//! Shared kernels (used by every shape in this module):
//!
//! * [`filter_fields`] — `api/serializers/base.py:32-70`
//!   (`BaseSerializer._filter_fields`, the `?fields=` filter).
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
}
