//! django-filter column tables (OVL-04 part 7, PIDASHCONV-1542).
//!
//! The GET chart handler (`api::app_analytics`) and the POST export task
//! (`jobs::tasks_export::analytic`) carried byte-identical copies of the
//! three column tables below. Each has exactly one canonical
//! implementation here now; both modules import it.
//!
//! Lifted verbatim from `app_analytics::handlers_a` (identical to the
//! `tasks_export::analytic` copies, modulo the doc comments).
//!
//! Shapes that stay local: the `app_intake` trio (same columns, but the
//! predicate paths carry the `issue__` join prefix — different match
//! arms, different SQL); the `app_issues` trio (unquoted
//! `alias.column` refs for the `Binder` query — quoting is wire bytes);
//! all four `day_comparison` (intake/jobs static `::date` vs
//! issues/analytics timezone-truncated `String`, plus the intake term
//! prefixes); the four filter compilers (`compile_filter` /
//! `compile_predicate` / `compile_text_predicate`,
//! `compile_filter_tree`, `filter_sql` x2, `filter_text_sql` x2,
//! `legacy_text_sql` — different value types, binders, error types and,
//! for GET-vs-POST text, `ILIKE` vs `LIKE`); intake `parse_cursor` /
//! `list_page` (singletons; `parse_cursor` also differs from
//! `paginator::Cursor::from_string` in value parsing — `str::parse`
//! vs `parse_py_float`/`parse_py_int` — so it is not the same
//! function); provider `paginate` / `next_url` (GitHub/GitLab
//! pagination, out of this family — Part 2 decision).

/// Columns for `__isnull` predicates (unprefixed ORM paths, quoted refs).
pub fn isnull_column(path: &str) -> Option<&'static str> {
    Some(match path {
        "parent" => "\"issues\".\"parent_id\"",
        "labels" => "\"label_issue\".\"label_id\"",
        "assignees" => "\"issue_assignee\".\"assignee_id\"",
        "created_by" => "\"issues\".\"created_by_id\"",
        "issue_cycle__cycle_id" => "\"issue_cycle\".\"cycle_id\"",
        "issue_module__module_id" => "\"issue_module\".\"module_id\"",
        "label_issue__deleted_at" => "\"label_issue\".\"deleted_at\"",
        "issue_assignee__deleted_at" => "\"issue_assignee\".\"deleted_at\"",
        "issue_cycle__deleted_at" => "\"issue_cycle\".\"deleted_at\"",
        "issue_module__deleted_at" => "\"issue_module\".\"deleted_at\"",
        "issue_subscribers__deleted_at" => "\"issue_subscribers\".\"deleted_at\"",
        "target_date" => "\"issues\".\"target_date\"",
        "start_date" => "\"issues\".\"start_date\"",
        _ => return None,
    })
}

/// Columns for UUID `__in` predicates. `logged_by` has no model field:
/// Django raises `FieldError` (generic 500).
pub fn uuid_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "state__in" => "\"issues\".\"state_id\"",
        "parent__in" => "\"issues\".\"parent_id\"",
        "labels__in" => "\"label_issue\".\"label_id\"",
        "assignees__in" => "\"issue_assignee\".\"assignee_id\"",
        "issue_mention__mention__id__in" => "\"issue_mention\".\"mention_id\"",
        "created_by__in" => "\"issues\".\"created_by_id\"",
        "project__in" => "\"issues\".\"project_id\"",
        "issue_cycle__cycle_id__in" => "\"issue_cycle\".\"cycle_id\"",
        "issue_module__module_id__in" => "\"issue_module\".\"module_id\"",
        "issue_subscribers__subscriber_id__in" => "\"issue_subscribers\".\"subscriber_id\"",
        _ => return None,
    })
}

/// Columns for string `__in` predicates.
pub fn strings_in_column(name: &str) -> Option<&'static str> {
    Some(match name {
        "state__group__in" => "\"states\".\"group\"",
        "estimate_point__in" => "\"issues\".\"estimate_point_id\"",
        "priority__in" => "\"issues\".\"priority\"",
        "issue_intake__status__in" => "\"issue_intake\".\"status\"",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isnull_table_pins_every_arm() {
        let cases = [
            ("parent", "\"issues\".\"parent_id\""),
            ("labels", "\"label_issue\".\"label_id\""),
            ("assignees", "\"issue_assignee\".\"assignee_id\""),
            ("created_by", "\"issues\".\"created_by_id\""),
            ("issue_cycle__cycle_id", "\"issue_cycle\".\"cycle_id\""),
            ("issue_module__module_id", "\"issue_module\".\"module_id\""),
            ("label_issue__deleted_at", "\"label_issue\".\"deleted_at\""),
            (
                "issue_assignee__deleted_at",
                "\"issue_assignee\".\"deleted_at\"",
            ),
            ("issue_cycle__deleted_at", "\"issue_cycle\".\"deleted_at\""),
            (
                "issue_module__deleted_at",
                "\"issue_module\".\"deleted_at\"",
            ),
            (
                "issue_subscribers__deleted_at",
                "\"issue_subscribers\".\"deleted_at\"",
            ),
            ("target_date", "\"issues\".\"target_date\""),
            ("start_date", "\"issues\".\"start_date\""),
        ];
        for (path, column) in cases {
            assert_eq!(isnull_column(path), Some(column), "{path}");
        }
        assert_eq!(isnull_column("issue__parent"), None);
        assert_eq!(isnull_column("logged_by"), None);
        assert_eq!(isnull_column(""), None);
    }

    #[test]
    fn uuid_in_table_pins_every_arm() {
        let cases = [
            ("state__in", "\"issues\".\"state_id\""),
            ("parent__in", "\"issues\".\"parent_id\""),
            ("labels__in", "\"label_issue\".\"label_id\""),
            ("assignees__in", "\"issue_assignee\".\"assignee_id\""),
            (
                "issue_mention__mention__id__in",
                "\"issue_mention\".\"mention_id\"",
            ),
            ("created_by__in", "\"issues\".\"created_by_id\""),
            ("project__in", "\"issues\".\"project_id\""),
            ("issue_cycle__cycle_id__in", "\"issue_cycle\".\"cycle_id\""),
            (
                "issue_module__module_id__in",
                "\"issue_module\".\"module_id\"",
            ),
            (
                "issue_subscribers__subscriber_id__in",
                "\"issue_subscribers\".\"subscriber_id\"",
            ),
        ];
        for (name, column) in cases {
            assert_eq!(uuid_in_column(name), Some(column), "{name}");
        }
        assert_eq!(uuid_in_column("issue__state__in"), None);
        assert_eq!(uuid_in_column("logged_by__in"), None);
        assert_eq!(uuid_in_column(""), None);
    }

    #[test]
    fn strings_in_table_pins_every_arm() {
        let cases = [
            ("state__group__in", "\"states\".\"group\""),
            ("estimate_point__in", "\"issues\".\"estimate_point_id\""),
            ("priority__in", "\"issues\".\"priority\""),
            ("issue_intake__status__in", "\"issue_intake\".\"status\""),
        ];
        for (name, column) in cases {
            assert_eq!(strings_in_column(name), Some(column), "{name}");
        }
        assert_eq!(strings_in_column("priority"), None);
        assert_eq!(strings_in_column(""), None);
    }
}
