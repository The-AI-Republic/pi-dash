#![forbid(unsafe_code)]

//! Response shapes for the issue-list family.
//!
//! Ports the `.values()` field lists, the `paginate()` envelope, and the
//! `user_timezone_converter` rule:
//! - `IssueViewSet.list` flat shape: the 26-key `.values(...)` list
//!   (base.py `list` → `on_results` path without group_by also funnels
//!   through `issue_on_results`, whose base list is the same 22 keys plus
//!   `state__group` and the three array annotations; the flat `.values()`
//!   list below is what the non-grouped branch renders — note it carries
//!   `state_id` but *not* `state__group`, while the grouped path adds
//!   `state__group`; the contract test asserts `"state__group" in row`
//!   on the default list because `issue_on_results` runs even when
//!   `group_by` is falsy (`on_results=lambda ... issue_on_results(
//!   group_by=False, ...)`), so the default shape is the on_results list).
//! - v2 `required_fields`: 26 keys plus `description_html` iff
//!   `description=true`.
//! - deleted-issues: a bare JSON id array.
//! - envelope: `BasePaginator.paginate` key order — `grouped_by`,
//!   `sub_grouped_by`, `total_count`, `next_cursor`, `prev_cursor`,
//!   `next_page_results`, `prev_page_results`, `count`, `total_pages`,
//!   `total_results`, `extra_stats`, `results`.
//! - datetimes: `created_at` / `updated_at` are shifted into the actor's
//!   `user_timezone` (pytz semantics: aware instant rendered in the named
//!   IANA zone) before DRF renders ISO-8601 with offset.

/// `IssueViewSet.list` flat `.values()` keys, in source order.
pub const LIST_VALUES_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "deleted_at",
];

/// `IssueListEndpoint.get` (flat `issues/list/`) `.values()` keys.
pub const FLAT_LIST_FIELDS: &[&str] = LIST_VALUES_FIELDS;

/// `issue_on_results` base keys before the array-annotation suffix.
pub const ON_RESULTS_BASE_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "sub_issues_count",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "attachment_count",
    "link_count",
    "is_draft",
    "archived_at",
    "state__group",
];

/// `issue_on_results` array-annotation suffix keys.
pub const ON_RESULTS_ARRAY_FIELDS: &[&str] = &["assignee_ids", "label_ids", "module_ids"];

/// The extra key the grouped path always carries.
pub const ON_RESULTS_STATE_GROUP_FIELD: &str = "state__group";

/// `IssueListDetailSerializer.to_representation` keys (the `issues-detail/`
/// path), in source order: like the on-results list minus `state__group`
/// (the serializer never emits it), with the three array suffixes at the
/// end. `fields=` is ignored by that serializer and `expand=` only appends
/// relation arrays, so with no `expand` this is the whole shape.
pub const DETAIL_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "is_draft",
    "archived_at",
    "cycle_id",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "sub_issues_count",
    "attachment_count",
    "link_count",
];

/// v2 `required_fields` without the `description_html` opt-in.
pub const V2_REQUIRED_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "state__group",
    "sort_order",
    "completed_at",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "is_draft",
    "archived_at",
    "module_ids",
    "label_ids",
    "assignee_ids",
    "link_count",
    "attachment_count",
    "sub_issues_count",
];

/// Datetime fields the list family shifts into the actor's timezone.
pub const DATETIME_FIELDS: &[&str] = &["created_at", "updated_at"];

/// `issue_group_values` static branches: `priority` and `state__group`.
/// (`STATE_GROUP_ORDER` without triage, same tuple the filter kernel
/// shares: backlog … cancelled.)
pub const PRIORITY_VALUES: &[&str] = &["low", "medium", "high", "urgent", "none"];

pub const STATE_GROUP_VALUES: &[&str] = &[
    "backlog",
    "unstarted",
    "started",
    "review",
    "test",
    "completed",
    "cancelled",
];

/// The `group_by == sub_group_by` 400 body.
pub fn group_mismatch_body() -> String {
    r#"{"error":"Group by and sub group by cannot have same parameters"}"#.to_owned()
}

/// The missing-`issues` 400 body on the flat endpoint.
pub fn issues_required_body() -> String {
    r#"{"error":"Issues are required"}"#.to_owned()
}

/// The deleted-issues body: a bare JSON array of id strings.
pub fn deleted_ids_body(ids: &[String]) -> String {
    let mut out = String::from("[");
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&id.replace('\\', "\\\\").replace('"', "\\\""));
        out.push('"');
    }
    out.push(']');
    out
}

/// v2 field list with the `description=true` opt-in applied.
pub fn v2_fields(description: bool) -> Vec<&'static str> {
    let mut fields = V2_REQUIRED_FIELDS.to_vec();
    if description {
        fields.push("description_html");
    }
    fields
}

/// `issue_on_results` selected keys for a group/sub-group combination.
/// Group keys in `FIELD_MAPPER` (`labels__id`, `assignees__id`,
/// `issue_module__module_id`) replace their array-annotation counterpart
/// with the raw group key; every other group key is appended unchanged.
/// (`FIELD_MAPPER` maps `label_ids`→`labels__id`,
/// `assignee_ids`→`assignees__id`, `module_ids`→`issue_module__module_id`.)
pub fn on_results_fields(group_by: Option<&str>, sub_group_by: Option<&str>) -> Vec<String> {
    const MAPPER: &[(&str, &str)] = &[
        ("labels__id", "label_ids"),
        ("assignees__id", "assignee_ids"),
        ("issue_module__module_id", "module_ids"),
    ];
    let mut arrays: Vec<String> = ON_RESULTS_ARRAY_FIELDS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let mut extra: Vec<String> = Vec::new();
    for group in [group_by, sub_group_by].into_iter().flatten() {
        match MAPPER.iter().find(|(raw, _)| *raw == group) {
            Some((_, array)) => {
                arrays.retain(|name| name != array);
                extra.push(group.to_owned());
            }
            None => extra.push(group.to_owned()),
        }
    }
    let mut fields: Vec<String> = ON_RESULTS_BASE_FIELDS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    fields.extend(arrays);
    fields.extend(extra);
    fields
}

/// Assemble the `paginate()` envelope in Django's key order.
/// All cursor/count values arrive pre-rendered; `results` is spliced raw.
#[allow(clippy::too_many_arguments)]
pub fn envelope(
    grouped_by: Option<&str>,
    sub_grouped_by: Option<&str>,
    total_count: i64,
    next_cursor: &str,
    prev_cursor: &str,
    next_page_results: bool,
    prev_page_results: bool,
    count: usize,
    total_pages: i64,
    total_results: i64,
    results_json: &str,
) -> String {
    let grouped = grouped_by
        .map(|value| format!("\"{value}\""))
        .unwrap_or_else(|| "null".to_owned());
    let sub_grouped = sub_grouped_by
        .map(|value| format!("\"{value}\""))
        .unwrap_or_else(|| "null".to_owned());
    format!(
        "{{\"grouped_by\":{grouped},\"sub_grouped_by\":{sub_grouped},\
        \"total_count\":{total_count},\"next_cursor\":\"{next_cursor}\",\
        \"prev_cursor\":\"{prev_cursor}\",\
        \"next_page_results\":{next_page_results},\"prev_page_results\":{prev_page_results},\
        \"count\":{count},\"total_pages\":{total_pages},\"total_results\":{total_results},\
        \"extra_stats\":null,\"results\":{results_json}}}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_key_order_matches_paginate() {
        let body = envelope(None, None, 3, "3:1:0", "3:0:1", true, false, 3, 1, 3, "[]");
        assert!(body.starts_with(r#"{"grouped_by":null,"sub_grouped_by":null,"total_count":3,"#));
        let keys = [
            "grouped_by",
            "sub_grouped_by",
            "total_count",
            "next_cursor",
            "prev_cursor",
            "next_page_results",
            "prev_page_results",
            "count",
            "total_pages",
            "total_results",
            "extra_stats",
            "results",
        ];
        let mut last = 0;
        for key in keys {
            let at = body.find(&format!("\"{key}\":")).expect(key);
            assert!(at > last, "{key} out of order");
            last = at;
        }
    }

    #[test]
    fn default_on_results_carries_state_group_and_arrays() {
        let fields = on_results_fields(None, None);
        assert!(fields.contains(&"state__group".to_owned()));
        assert!(fields.contains(&"assignee_ids".to_owned()));
        assert!(fields.contains(&"label_ids".to_owned()));
        assert!(fields.contains(&"module_ids".to_owned()));
        assert!(!fields.contains(&"deleted_at".to_owned()));
    }

    #[test]
    fn group_key_replaces_its_array_counterpart() {
        let fields = on_results_fields(Some("labels__id"), Some("priority"));
        assert!(fields.contains(&"labels__id".to_owned()));
        assert!(!fields.contains(&"label_ids".to_owned()));
        assert!(fields.contains(&"assignee_ids".to_owned()));
        assert!(fields.contains(&"priority".to_owned()));
    }

    #[test]
    fn detail_shape_is_list_detail_serializer_order() {
        // Source order of IssueListDetailSerializer.to_representation:
        // base keys, then cycle/arrays/counts; no state__group.
        assert_eq!(DETAIL_FIELDS.len(), 25);
        assert!(!DETAIL_FIELDS.contains(&"state__group"));
        assert!(!DETAIL_FIELDS.contains(&"deleted_at"));
        let tail = &DETAIL_FIELDS[DETAIL_FIELDS.len() - 7..];
        assert_eq!(
            tail,
            [
                "cycle_id",
                "module_ids",
                "label_ids",
                "assignee_ids",
                "sub_issues_count",
                "attachment_count",
                "link_count",
            ]
        );
    }

    #[test]
    fn v2_description_opt_in_appends_html() {
        assert!(!v2_fields(false).contains(&"description_html"));
        let fields = v2_fields(true);
        assert_eq!(fields.last(), Some(&"description_html"));
        assert_eq!(fields.len(), V2_REQUIRED_FIELDS.len() + 1);
    }

    #[test]
    fn deleted_ids_body_is_bare_array() {
        assert_eq!(deleted_ids_body(&[]), "[]");
        assert_eq!(
            deleted_ids_body(&["a".to_owned(), "b".to_owned()]),
            r#"["a","b"]"#
        );
    }
}
