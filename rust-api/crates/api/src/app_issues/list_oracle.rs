#![forbid(unsafe_code)]

//! The text issue-list engine, kept only as the differential oracle for
//! [`super::list_engine`] (Batch L). Every builder below is the pre-Batch-L
//! production code, moved verbatim; the live test at the bottom seeds one
//! realistic dataset and asserts the sea-query engine returns byte-identical
//! JSON for every list variant. Delete this module once nothing needs the
//! comparison.

use std::collections::HashMap;

use pidash_services::app_issues::OrderSpec;
use sqlx::Row;

use super::reads_oracle::{archive_scope_where, ARCHIVE_JOINS};
use super::{
    bind_all, complex_filter, legacy_sql, query_last, render_condition, with_order_joins, Binder,
    Denial, FilterLayers, FilteredSet, Gate, OrderKey, QueryMap, INVALID_DETAIL_BODY_MSG,
};
use super::{min_order_subquery, order_key, parse_datetime_param};

/// Relation joins the list query can use: SQL table plus kernel alias.
const RELATION_JOINS: &[(&str, &str, &str)] = &[
    ("issue_assignees", "issue_assignee", "assignee_id"),
    ("cycle_issues", "issue_cycle", "cycle_id"),
    ("module_issues", "issue_module", "module_id"),
    ("issue_mentions", "issue_mention", "mention_id"),
    ("issue_labels", "label_issue", "label_id"),
    ("issue_subscribers", "issue_subscribers", "subscriber_id"),
];

/// True when every `"alias".` reference in the filter SQL is an
/// `IS NULL` / `IS NOT NULL` test (the `__isnull` predicates).
fn alias_nullable_only(where_sql: &str, alias: &str) -> bool {
    let marker = format!("\"{alias}\".");
    let mut rest = where_sql;
    while let Some(start) = rest.find(&marker) {
        let after = &rest[start + marker.len()..];
        let ident_len = after
            .chars()
            .take_while(|char| char.is_alphanumeric() || *char == '_' || *char == '"')
            .map(|char| char.len_utf8())
            .sum::<usize>();
        let tail = after[ident_len..].trim_start();
        if !(tail.starts_with("IS NULL") || tail.starts_with("IS NOT NULL")) {
            return false;
        }
        rest = &after[ident_len..];
    }
    true
}

/// M2M group filters (`GROUP_FILTER_MAPPER` in `utils/grouper.py`):
/// grouping by this raw field guards its join's `deleted_at` over the
/// LEFT join (linkless issues keep the NULL group key).
fn group_join_alias(group_by: Option<&str>, sub_group_by: Option<&str>, alias: &str) -> bool {
    let raw = match alias {
        "issue_assignee" => "assignees__id",
        "label_issue" => "labels__id",
        "issue_module" => "issue_module__module_id",
        _ => return false,
    };
    group_by == Some(raw) || sub_group_by == Some(raw)
}

/// Scalar subquery annotations shared by every list path
/// (`apply_annotations` + the grouper array annotations).
/// `unguarded_arrays` reproduces `IssueDetailEndpoint.apply_annotations` +
/// `IssueListDetailSerializer`'s prefetch reads: the reverse managers run
/// on the plain `objects` manager, so the array subqueries carry *no*
/// soft-delete guard and the module one neither joins `modules` nor checks
/// `scalar_counts` selects the empty-count rendering per path: Django
/// uses two annotation shapes for these three counts and they differ
/// exactly here (both verified with `str(qs.query)` on Django 4.2.30,
/// `USE_TZ`, `TIME_ZONE="UTC"`).
/// Scalar (`Func(F("id"), function="Count")`, not an Aggregate → no
/// `GROUP BY`): `(SELECT Count(U0."id") ...)` always returns one row, so
/// empty renders `0`. Used by `IssueListEndpoint.get`
/// (`app/views/issue/base.py:121-141`) and
/// `IssueDetailEndpoint.apply_annotations` (`base.py:999-1019`) — the
/// `flat_list` and `detail_list` paths (`scalar_counts = true`, plain
/// `COUNT`).
/// Grouped (`Subquery(...values().annotate(count=Count()).values("count"))`
/// → `GROUP BY`): zero related rows yield zero groups, so the scalar
/// subquery is `NULL` → JSON `null`. Used by
/// `IssueViewSet.apply_annotations` (`base.py:225-251`) and
/// `IssuePaginatedViewSet.get_queryset` (`base.py:839-866`) — the
/// `list_issues` (flat + grouped) and `v2_list` paths
/// (`scalar_counts = false`, `NULLIF(COUNT(*), 0)`).
/// Array guards mirror the managers Django queries through:
/// `IssueLabel`/`IssueAssignee`/`ModuleIssue.objects` are soft-deletion
/// managers (deleted rows excluded everywhere); the v2 assignee filter
/// adds the active-member join; the grouper and the v2 view add the
/// archived-module guard, while the detail endpoint's
/// `Prefetch(...objects.all())` carries the manager's deleted filter only
/// (`module_archived_guard = false` there). The extra `m.deleted_at`
/// guard on modules is a disclosed divergence (shared helper, all paths).
pub(crate) fn annotation_selects(
    arrays: bool,
    skip_array: Option<&str>,
    assignee_active_member: bool,
    module_archived_guard: bool,
    scalar_counts: bool,
) -> String {
    let count = if scalar_counts {
        "COUNT(*)"
    } else {
        "NULLIF(COUNT(*), 0)"
    };
    let mut selects = format!(
        r#"issue.id, issue.name, issue.state_id, issue.sort_order, issue.completed_at,
        issue.estimate_point_id AS estimate_point, issue.priority, issue.start_date,
        issue.target_date, issue.sequence_id, issue.project_id, issue.parent_id,
        (SELECT ci.cycle_id FROM cycle_issues ci
          WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1) AS cycle_id,
        (SELECT {count} FROM issue_links il
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS link_count,
        (SELECT {count} FROM file_assets fa
          WHERE fa.issue_id = issue.id AND fa.entity_type = 'ISSUE_ATTACHMENT'
            AND fa.deleted_at IS NULL) AS attachment_count,
        (SELECT {count} FROM issues c
           LEFT JOIN states cs ON cs.id = c.state_id AND cs.deleted_at IS NULL
           JOIN projects cp ON cp.id = c.project_id
          WHERE c.parent_id = issue.id AND c.deleted_at IS NULL
            AND (cs."group" IS NULL OR NOT (cs."group" = 'triage'))
            AND c.archived_at IS NULL AND cp.archived_at IS NULL AND c.is_draft = FALSE
        ) AS sub_issues_count,
        issue.created_at, issue.updated_at, issue.created_by_id AS created_by,
        issue.updated_by_id AS updated_by, issue.is_draft, issue.archived_at,
        issue.deleted_at, state."group" AS "state__group""#,
    );
    if arrays {
        if skip_array != Some("label_ids") {
            selects.push_str(
                r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT il.label_id), '{}'::uuid[])
           FROM issue_labels il
          WHERE il.issue_id = issue.id AND il.deleted_at IS NULL) AS label_ids"#,
            );
        }
        if skip_array != Some("assignee_ids") {
            if assignee_active_member {
                selects.push_str(
                    r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id), '{}'::uuid[])
           FROM issue_assignees ia
          WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL
            AND EXISTS (SELECT 1 FROM project_members pm
                         WHERE pm.member_id = ia.assignee_id
                           AND pm.is_active AND pm.deleted_at IS NULL)) AS assignee_ids"#,
                );
            } else {
                selects.push_str(
                    r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT ia.assignee_id), '{}'::uuid[])
           FROM issue_assignees ia
          WHERE ia.issue_id = issue.id AND ia.deleted_at IS NULL) AS assignee_ids"#,
                );
            }
        }
        if skip_array != Some("module_ids") {
            if module_archived_guard {
                selects.push_str(
                    r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT mi.module_id), '{}'::uuid[])
           FROM module_issues mi JOIN modules m ON m.id = mi.module_id
          WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL
            AND m.archived_at IS NULL AND m.deleted_at IS NULL) AS module_ids"#,
                );
            } else {
                selects.push_str(
                    r#",
        (SELECT COALESCE(ARRAY_AGG(DISTINCT mi.module_id), '{}'::uuid[])
           FROM module_issues mi
          WHERE mi.issue_id = issue.id AND mi.deleted_at IS NULL) AS module_ids"#,
                );
            }
        }
    }
    selects
}

/// `WHERE` preamble: tenant scope plus the `issue_objects` manager
/// (`SoftDeletionManager` + triage/archived/draft exclusions).
/// `detail` selects the `IssueDetailEndpoint` permission model: the
/// `Exists` membership subquery (passed separately via `extra`) replaces
/// the guest `created_by` scoping, so the preamble skips it. The flat
/// `issues/list/` endpoint never scopes guests either
/// (`IssueListEndpoint.get` has no role-5 check), so it passes
/// `guest_scope = false` too.
/// The triage exclusion keeps NULL-state rows: `state` is a nullable FK
/// and Django's `exclude(state__group=TRIAGE)` retains them via
/// `split_exclude`'s `IS NULL` disjunct; a bare `NOT (group = 'triage')`
/// over the left join would drop them (three-valued logic).
fn base_where(
    binder: &mut Binder,
    gate: &Gate,
    slug: &str,
    detail: bool,
    guest_scope: bool,
) -> String {
    let slug_holder = binder.bind_string(slug.to_owned());
    let project_holder = binder.bind_uuid(gate.project_id);
    let mut where_sql = format!(
        r#"workspaces.slug = {slug_holder} AND issue.project_id = {project_holder}
        AND issue.deleted_at IS NULL
        AND (state."group" IS NULL OR NOT (state."group" = 'triage'))
        AND issue.archived_at IS NULL AND project.archived_at IS NULL
        AND issue.is_draft = FALSE"#
    );
    if gate.guest_scoped && !detail && guest_scope {
        let user_holder = binder.bind_uuid(gate.user_id);
        where_sql.push_str(&format!(" AND issue.created_by_id = {user_holder}"));
    }
    where_sql
}

#[allow(clippy::too_many_arguments)]
pub fn filtered_set(
    gate: &Gate,
    slug: &str,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    extra: Option<(String, Vec<sea_query::Value>)>,
    detail: bool,
    layers: FilterLayers,
) -> Result<FilteredSet, Denial> {
    let mut binder = Binder::new();
    let preamble = base_where(&mut binder, gate, slug, detail, layers.guest_scope);
    let mut fragments: Vec<String> = vec![preamble];
    // Rich filters (`filters` JSON via ComplexFilterBackend + IssueFilterSet).
    let mut complex_sql = String::new();
    if layers.rich {
        if let Some(cond) = complex_filter(query, &gate.timezone)? {
            let (fragment, values) = render_condition(&cond);
            complex_sql = binder.splice(&fragment, values);
        }
    }
    // Legacy filters. Relative dates resolve against the UTC date, exactly
    // like `timezone.now().date()` in `issue_filters.py` (`now()` is always
    // UTC; the request tz activation only affects `localtime()`). The
    // `__date` truncations use the gate's actor zone instead (Django's
    // `TruncDate` under the `TimezoneMixin`-activated user zone).
    let mut legacy_sql_text = String::new();
    if layers.legacy {
        let flat: HashMap<String, String> = query
            .keys()
            .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
            .collect();
        let today = chrono::Utc::now().date_naive();
        let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
            .map_err(|_| Denial::ServerError)?;
        for (name, value) in legacy.predicates() {
            let fragment = legacy_sql(&mut binder, name, value, &gate.timezone)?;
            if legacy_sql_text.is_empty() {
                legacy_sql_text = fragment;
            } else {
                legacy_sql_text = format!("{legacy_sql_text} AND {fragment}");
            }
        }
    }
    if !complex_sql.is_empty() {
        fragments.push(complex_sql);
    }
    if !legacy_sql_text.is_empty() {
        fragments.push(legacy_sql_text);
    }
    if let Some((fragment, binds)) = extra {
        fragments.push(binder.splice(&fragment, binds));
    }
    // `GROUP_FILTER_MAPPER` (`utils/grouper.py:41-49`): a canonical m2m
    // group key — group or sub — filters its join's `deleted_at`.
    // Over the LEFT join that keeps linkless issues (NULL group key,
    // the "None" bucket) while rows whose only links are soft-deleted
    // drop out, exactly like Django.
    for key in [group_by, sub_group_by].into_iter().flatten() {
        let alias = match key {
            "labels__id" => "label_issue",
            "assignees__id" => "issue_assignee",
            "issue_module__module_id" => "issue_module",
            _ => continue,
        };
        fragments.push(format!("\"{alias}\".\"deleted_at\" IS NULL"));
    }
    let where_sql = fragments.join(" AND ");
    // INNER when referenced by a positive condition (Django's
    // `filter()` join); LEFT when every reference is an
    // `IS [NOT] NULL` test (Django's `__isnull` join, the m2m group
    // key included) or unreferenced.
    // Joined tables carry no soft-delete guard: `filter()` never applies a
    // related manager to a join, so `filter(labels__in=...)` matches
    // soft-deleted link rows and `labels__isnull=False` keeps issues whose
    // only links are deleted.
    let mut joins = String::new();
    let mut referenced = Vec::new();
    if layers.rich || layers.legacy {
        for (table, alias, key_column) in RELATION_JOINS {
            let marker = format!("\"{alias}\".");
            let mentioned = where_sql.contains(&marker);
            let nullable_only = mentioned && alias_nullable_only(&where_sql, alias);
            let used = mentioned || group_join_alias(group_by, sub_group_by, alias);
            let inner = used && !nullable_only;
            if used {
                referenced.push(*alias);
            }
            let kind = if inner { "INNER JOIN" } else { "LEFT JOIN" };
            joins.push_str(&format!(
                " {kind} {table} AS {alias} ON {alias}.issue_id = issue.id"
            ));
            let _ = key_column;
        }
        let intake_used = where_sql.contains("\"issue_intake\".");
        joins.push_str(&format!(
            " {} intake_issues AS issue_intake ON issue_intake.issue_id = issue.id",
            if intake_used {
                "INNER JOIN"
            } else {
                "LEFT JOIN"
            }
        ));
        if intake_used {
            referenced.push("issue_intake");
        }
    }
    let from_where = format!(
        r#"FROM issues AS issue
        JOIN projects AS project ON project.id = issue.project_id
        JOIN workspaces ON workspaces.id = issue.workspace_id
        LEFT JOIN states AS state ON state.id = issue.state_id AND state.deleted_at IS NULL
        {joins}
        WHERE {where_sql}"#
    );
    Ok(FilteredSet {
        from_where,
        values: binder.values(),
        referenced,
    })
}

/// The epic boundary as a WHERE fragment (`epics.py:65-66`): Django's
/// `filter(type__is_epic=True)` inner-joins `issue_types` with no
/// related-manager guard — the EXISTS below is that join (the
/// single-letter alias keeps the relation-join marker scan quiet).
const EPIC_ONLY_FRAGMENT: &str =
    "EXISTS(SELECT 1 FROM issue_types AS t WHERE t.id = issue.type_id AND t.is_epic)";

/// The SQL expression a group field partitions by.
fn group_expression(field: &str) -> Result<String, Denial> {
    Ok(match field {
        "labels__id" => "label_issue.label_id".to_owned(),
        "assignees__id" => "issue_assignee.assignee_id".to_owned(),
        "issue_module__module_id" => "issue_module.module_id".to_owned(),
        "state_id" => "issue.state_id".to_owned(),
        "priority" => "issue.priority".to_owned(),
        "cycle_id" => "(SELECT ci.cycle_id FROM cycle_issues ci WHERE ci.issue_id = issue.id AND ci.deleted_at IS NULL LIMIT 1)".to_owned(),
        "project_id" => "issue.project_id".to_owned(),
        "state__group" => "state.\"group\"".to_owned(),
        "target_date" => "issue.target_date".to_owned(),
        "start_date" => "issue.start_date".to_owned(),
        "created_by" => "issue.created_by_id".to_owned(),
        _ => return Err(Denial::ServerError),
    })
}

/// Extra select carrying the raw m2m group key per joined row.
fn group_member_select(group_by: &str) -> &'static str {
    match group_by {
        "labels__id" => ", label_issue.label_id AS \"labels__id\"",
        "assignees__id" => ", issue_assignee.assignee_id AS \"assignees__id\"",
        "issue_module__module_id" => ", issue_module.module_id AS \"issue_module__module_id\"",
        _ => "",
    }
}

/// `(group, filtered count)` pairs for the totals dict.
async fn group_total_pairs(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    group_expr: &str,
    count_filter: &str,
) -> Result<Vec<(String, i64)>, Denial> {
    let sql = format!(
        "SELECT COALESCE(({group_expr})::text, 'None') AS bucket, COUNT(DISTINCT issue.id) FILTER (WHERE {strip_and}) AS n {} GROUP BY 1",
        filtered.from_where,
        strip_and = count_filter.strip_prefix("AND ").unwrap_or(count_filter),
    );
    let query = bind_all(&sql, filtered.values.clone())?;
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::new();
    for row in rows {
        let bucket: String = row.try_get("bucket").map_err(|_| Denial::ServerError)?;
        let count: i64 = row.try_get("n").map_err(|_| Denial::ServerError)?;
        out.push((bucket, count));
    }
    Ok(out)
}

/// `(group, sub, count)` pairs for the nested sub-totals.
async fn sub_total_pairs(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    group_expr: &str,
    sub_expr: &str,
    count_filter: &str,
) -> Result<Vec<(String, String, i64)>, Denial> {
    let sql = format!(
        "SELECT COALESCE(({group_expr})::text, 'None') AS bucket, COALESCE(({sub_expr})::text, 'None') AS sub, COUNT(DISTINCT issue.id) FILTER (WHERE {strip_and}) AS n {} GROUP BY 1, 2",
        filtered.from_where,
        strip_and = count_filter.strip_prefix("AND ").unwrap_or(count_filter),
    );
    let query = bind_all(&sql, filtered.values.clone())?;
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::new();
    for row in rows {
        let bucket: String = row.try_get("bucket").map_err(|_| Denial::ServerError)?;
        let sub: String = row.try_get("sub").map_err(|_| Denial::ServerError)?;
        let count: i64 = row.try_get("n").map_err(|_| Denial::ServerError)?;
        out.push((bucket, sub, count));
    }
    Ok(out)
}

/// `ORDER BY` for the unpaginated flat endpoint, as an expression:
/// `(expr, descending, created_at tiebreak, order joins)`. Mirrors
/// [`super::list_engine::flat_order_key`]: the flat list selects the
/// expr as `__order_key` and orders by the alias so the statement is
/// valid under `DISTINCT` (Django `issues/list/` ends with
/// `.distinct()`).
fn flat_order_key_sql(
    order_spec: &OrderSpec,
    orig_param: &str,
) -> Result<(String, bool, bool, String), Denial> {
    let out = order_spec.out_param.as_str();
    if out == "priority_order" || out == "-priority_order" {
        let expr = pidash_services::app_issues::priority_case_sql();
        return Ok((expr, false, true, String::new()));
    }
    if out == "state_order" || out == "-state_order" {
        let expr = pidash_services::app_issues::state_case_sql(r#"state."group""#);
        return Ok((expr, false, true, String::new()));
    }
    if out == "min_values" || out == "-min_values" {
        let sub = min_order_subquery(orig_param.trim_start_matches('-'))?;
        return Ok((sub, out.starts_with('-'), true, String::new()));
    }
    // The flat `FROM` always carries the unfiltered `issue_cycle`
    // bridge join, which the cycle-name traversal reuses like Django.
    let key = order_key(out, orig_param, Some("issue_cycle"), false)?;
    Ok((
        key.expr.clone(),
        key.descending,
        !orig_param.contains("created_at"),
        key.joins.clone(),
    ))
}

/// The `IssueDetailEndpoint.get` permission subquery: the issue survives
/// when the actor holds an active project membership with a role above
/// guest, or an active guest membership on a project with
/// `guest_view_all_features`, or an active guest membership without it on
/// issues they created. The outer query already pins the issue's workspace
/// and project, so correlating the primary key (plus the project, belt and
/// braces) makes the subquery's own workspace/project filters redundant —
/// omitted, not weakened. Role and soft-delete semantics mirror
/// [`allow_project`].
fn permission_exists_fragment(binder: &mut Binder, gate: &Gate) -> String {
    let project_holder = binder.bind_uuid(gate.project_id);
    let user_holder = binder.bind_uuid(gate.user_id);
    let branch = |role_pred: &str, view_all: &str, own_only: bool| {
        let own = if own_only {
            format!(" AND _perm.created_by_id = {user_holder}")
        } else {
            String::new()
        };
        format!(
            "EXISTS (SELECT 1 FROM project_members AS _pm \
             JOIN projects AS _pp ON _pp.id = _pm.project_id \
             WHERE _pm.project_id = _perm.project_id \
             AND _pm.member_id = {user_holder} AND _pm.is_active \
             AND _pm.deleted_at IS NULL AND {role_pred} \
             AND _pp.guest_view_all_features = {view_all}{own})"
        )
    };
    format!(
        "EXISTS (SELECT 1 FROM issues AS _perm \
         WHERE _perm.id = issue.id AND _perm.project_id = {project_holder} \
         AND ({} OR {} OR {}))",
        branch("_pm.role > 5", "TRUE", false),
        branch("_pm.role = 5", "TRUE", false),
        branch("_pm.role = 5", "FALSE", true),
    )
}

/// `updated_at__gt` shared by list, v2 and deleted-issues. Garbage is a
/// `ValidationError` (400 invalid detail), not SQL text.
fn updated_at_gt(query: &QueryMap) -> Result<(Option<String>, Vec<sea_query::Value>), Denial> {
    let raw = query_last(query, "updated_at__gt");
    let Some(raw) = raw else {
        return Ok((None, Vec::new()));
    };
    let parsed = parse_datetime_param(&raw)
        .ok_or_else(|| Denial::BadError(INVALID_DETAIL_BODY_MSG.to_owned()))?;
    let mut binder = Binder::new();
    let holder = binder.bind_string(parsed.to_rfc3339());
    Ok((
        Some(format!("issue.updated_at > {holder}::timestamptz")),
        binder.values(),
    ))
}

/// The filtered archived set: [`archive_scope_where`] plus the
/// `show_sub_issues` top-level filter, then the same rich + legacy
/// stacks and relation joins as [`super::filtered_set`]. The `states`
/// join serves `state__group` grouping/ordering with Django's auto-join
/// semantics (no soft-delete guard); there is no `projects` join (the
/// archive scope never filters on it, and no filter leaf references it).
fn archive_filtered_set(
    gate: &Gate,
    slug: &str,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    show_sub_issues: bool,
) -> Result<FilteredSet, Denial> {
    let mut binder = Binder::new();
    let mut preamble = archive_scope_where(&mut binder, slug, gate.project_id);
    if !show_sub_issues {
        preamble.push_str(" AND issue.parent_id IS NULL");
    }
    let mut fragments: Vec<String> = vec![preamble];
    let mut complex_sql = String::new();
    if let Some(cond) = complex_filter(query, &gate.timezone)? {
        let (fragment, values) = render_condition(&cond);
        complex_sql = binder.splice(&fragment, values);
    }
    let flat: HashMap<String, String> = query
        .keys()
        .filter_map(|key| query_last(query, key).map(|last| (key.clone(), last)))
        .collect();
    let today = chrono::Utc::now().date_naive();
    let legacy = pidash_db::issue_filters::issue_filters_get(&flat, "", today)
        .map_err(|_| Denial::ServerError)?;
    let mut legacy_sql_text = String::new();
    for (name, value) in legacy.predicates() {
        // The archive view shares the issue viewset's `TimezoneMixin`:
        // `__date` terms truncate in the actor zone.
        let fragment = legacy_sql(&mut binder, name, value, &gate.timezone)?;
        if legacy_sql_text.is_empty() {
            legacy_sql_text = fragment;
        } else {
            legacy_sql_text = format!("{legacy_sql_text} AND {fragment}");
        }
    }
    if !complex_sql.is_empty() {
        fragments.push(complex_sql);
    }
    if !legacy_sql_text.is_empty() {
        fragments.push(legacy_sql_text);
    }
    // `GROUP_FILTER_MAPPER` (`utils/grouper.py:41-49`): a canonical m2m
    // group key — group or sub — filters its join's `deleted_at`.
    // Over the LEFT join that keeps linkless issues (NULL group key,
    // the "None" bucket) while rows whose only links are soft-deleted
    // drop out, exactly like Django.
    for key in [group_by, sub_group_by].into_iter().flatten() {
        let alias = match key {
            "labels__id" => "label_issue",
            "assignees__id" => "issue_assignee",
            "issue_module__module_id" => "issue_module",
            _ => continue,
        };
        fragments.push(format!("\"{alias}\".\"deleted_at\" IS NULL"));
    }
    let where_sql = fragments.join(" AND ");
    let mut joins = String::new();
    let mut referenced = Vec::new();
    for (table, alias, key_column) in RELATION_JOINS {
        let marker = format!("\"{alias}\".");
        let mentioned = where_sql.contains(&marker);
        let nullable_only = mentioned && alias_nullable_only(&where_sql, alias);
        let used = mentioned || group_join_alias(group_by, sub_group_by, alias);
        let inner = used && !nullable_only;
        if used {
            referenced.push(*alias);
        }
        let kind = if inner { "INNER JOIN" } else { "LEFT JOIN" };
        joins.push_str(&format!(
            " {kind} {table} AS {alias} ON {alias}.issue_id = issue.id"
        ));
        let _ = key_column;
    }
    let intake_used = where_sql.contains("\"issue_intake\".");
    joins.push_str(&format!(
        " {} intake_issues AS issue_intake ON issue_intake.issue_id = issue.id",
        if intake_used {
            "INNER JOIN"
        } else {
            "LEFT JOIN"
        }
    ));
    if intake_used {
        referenced.push("issue_intake");
    }
    let from_where = format!(
        "{ARCHIVE_JOINS} \
        LEFT JOIN states AS state ON state.id = issue.state_id \
        {joins} \
        WHERE {where_sql}"
    );
    Ok(FilteredSet {
        from_where,
        values: binder.values(),
        referenced,
    })
}

// ---- the old statement texts ---------------------------------------------------

/// The old flat paginated page.
pub(crate) fn flat_page_sql(
    filtered: &FilteredSet,
    selects: &str,
    key: &OrderKey,
    limit: i64,
    offset: i64,
) -> String {
    let direction = if key.descending { "DESC" } else { "ASC" };
    let row_where = with_order_joins(&filtered.from_where, &key.joins);
    format!(
        "SELECT DISTINCT {selects}, ({}) AS __order_key {row_where} ORDER BY __order_key {direction} NULLS LAST, issue.created_at DESC LIMIT {limit} OFFSET {offset}",
        key.expr
    )
}

pub(crate) fn count_distinct_sql(filtered: &FilteredSet) -> String {
    format!("SELECT COUNT(DISTINCT issue.id) {}", filtered.from_where)
}

pub(crate) fn count_rows_sql(filtered: &FilteredSet, joins: &str) -> String {
    format!(
        "SELECT COUNT(*) {}",
        with_order_joins(&filtered.from_where, joins)
    )
}

/// The old grouped window page.
pub(crate) fn grouped_page_sql(
    filtered: &FilteredSet,
    group_by: &str,
    sub_group_by: Option<&str>,
    selects: &str,
    key: &OrderKey,
    offset: i64,
    stop: i64,
) -> Result<String, Denial> {
    let direction = if key.descending { "DESC" } else { "ASC" };
    let group_expr = group_expression(group_by)?;
    let member_select = group_member_select(group_by);
    let partition = match sub_group_by {
        Some(sub) => format!("{group_expr}, {}", group_expression(sub)?),
        None => group_expr.clone(),
    };
    let row_where = with_order_joins(&filtered.from_where, &key.joins);
    let key_expr = &key.expr;
    Ok(format!(
        "SELECT * FROM (SELECT DISTINCT {selects}{member_select}, ({key_expr}) AS __order_key,
         ROW_NUMBER() OVER (PARTITION BY {partition} ORDER BY ({key_expr}) {direction} NULLS LAST, issue.created_at DESC) AS __rn
         {row_where}) __w
         WHERE __w.__rn > {offset} AND __w.__rn <= {stop}
         ORDER BY __w.__order_key {direction} NULLS LAST, __w.created_at DESC"
    ))
}

pub(crate) const COUNT_FILTER: &str =
    "((issue_intake.status = ANY('{1,-1,2}')) OR issue_intake.id IS NULL) AND issue.archived_at IS NULL AND issue.is_draft = FALSE";

pub(crate) async fn group_totals(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    group_by: &str,
) -> Result<Vec<(String, i64)>, Denial> {
    group_total_pairs(pool, filtered, &group_expression(group_by)?, COUNT_FILTER).await
}

pub(crate) async fn sub_totals(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    group_by: &str,
    sub_group_by: &str,
) -> Result<Vec<(String, String, i64)>, Denial> {
    sub_total_pairs(
        pool,
        filtered,
        &group_expression(group_by)?,
        &group_expression(sub_group_by)?,
        COUNT_FILTER,
    )
    .await
}

/// The old `issues/list/` `.values()` rows.
pub(crate) fn flat_list_sql(
    filtered: &FilteredSet,
    order_spec: &OrderSpec,
    orig_param: &str,
) -> Result<String, Denial> {
    let (expr, descending, tiebreak, order_joins) = flat_order_key_sql(order_spec, orig_param)?;
    let selects = annotation_selects(true, None, false, true, true);
    let row_where = with_order_joins(&filtered.from_where, &order_joins);
    let mut sql = format!(
        "SELECT DISTINCT {selects}, ({expr}) AS __order_key {row_where} ORDER BY __order_key {}",
        if descending { "DESC" } else { "ASC" },
    );
    if tiebreak {
        sql.push_str(", created_at DESC");
    }
    Ok(sql)
}

pub(crate) fn serializer_sql(filtered: &FilteredSet) -> String {
    let selects = r#"issue.id, issue.workspace_id, issue.name, issue.state_id, issue.sort_order,
        issue.completed_at, issue.estimate_point_id AS estimate_point, issue.priority,
        issue.complexity_score, issue.start_date, issue.target_date, issue.sequence_id,
        issue.project_id, issue.parent_id, issue.type_id, issue.assigned_pod_id,
        issue.agent_executor, issue.created_at, issue.updated_at,
        issue.created_by_id AS created_by, issue.updated_by_id AS updated_by,
        issue.is_draft, issue.archived_at,
        (CASE WHEN issue.external_source IS NULL OR issue.external_source = '' THEN FALSE
         ELSE (EXISTS (SELECT 1 FROM git_issue_syncs g
                       WHERE g.issue_id = issue.id AND g.deleted_at IS NULL)
            OR EXISTS (SELECT 1 FROM github_issue_syncs gh
                       WHERE gh.issue_id = issue.id AND gh.deleted_at IS NULL))
         END) AS is_synced"#;
    format!(
        "SELECT {selects} {} ORDER BY issue.created_at DESC",
        filtered.from_where
    )
}

pub(crate) fn v2_sql(filtered: &FilteredSet, limit: i64, offset: i64) -> String {
    let mut selects = annotation_selects(true, None, true, true, false);
    selects.push_str(", issue.description_html");
    format!(
        "SELECT {selects} {} ORDER BY issue.updated_at LIMIT {limit} OFFSET {offset}",
        filtered.from_where
    )
}

/// The old deleted-issues statement and binds.
pub(crate) fn deleted_sql(
    gate: &Gate,
    slug: &str,
    query: &QueryMap,
) -> Result<(String, Vec<sea_query::Value>), Denial> {
    let mut binder = Binder::new();
    let slug_holder = binder.bind_string(slug.to_owned());
    let project_holder = binder.bind_uuid(gate.project_id);
    let mut where_sql = format!(
        "FROM issues AS issue JOIN projects AS project ON project.id = issue.project_id
         JOIN workspaces ON workspaces.id = issue.workspace_id
         WHERE workspaces.slug = {slug_holder} AND issue.project_id = {project_holder}
         AND (issue.archived_at IS NOT NULL OR issue.deleted_at IS NOT NULL)"
    );
    let updated_gt = updated_at_gt(query)?;
    if let Some(extra) = updated_gt.0 {
        where_sql.push_str(&format!(" AND {}", binder.splice(&extra, updated_gt.1)));
    }
    Ok((
        format!("SELECT issue.id::text AS id {where_sql} ORDER BY issue.created_at DESC"),
        binder.values(),
    ))
}

/// The old main-list `extra` (updated_at__gt + epic boundary).
pub(crate) fn list_extra(
    query: &QueryMap,
    epic: bool,
) -> Result<Option<(String, Vec<sea_query::Value>)>, Denial> {
    Ok(match (updated_at_gt(query)?, epic) {
        ((Some(fragment), binds), true) => {
            Some((format!("{fragment} AND {EPIC_ONLY_FRAGMENT}"), binds))
        }
        ((Some(fragment), binds), false) => Some((fragment, binds)),
        ((None, _), true) => Some((EPIC_ONLY_FRAGMENT.to_owned(), Vec::new())),
        ((None, _), false) => None,
    })
}

/// The old detail permission `extra`.
pub(crate) fn detail_extra(gate: &Gate) -> (String, Vec<sea_query::Value>) {
    let mut binder = Binder::new();
    let fragment = permission_exists_fragment(&mut binder, gate);
    (fragment, binder.values())
}

/// The old `issue.id IN (...)` extra.
pub(crate) fn ids_extra(ids: &[uuid::Uuid]) -> (String, Vec<sea_query::Value>) {
    let mut binder = Binder::new();
    let holders: Vec<String> = ids.iter().map(|id| binder.bind_uuid(*id)).collect();
    (
        format!("issue.id IN ({})", holders.join(",")),
        binder.values(),
    )
}

/// The old archive set, exposed for the differential test.
pub(crate) fn archive_set(
    gate: &Gate,
    slug: &str,
    query: &QueryMap,
    group_by: Option<&str>,
    sub_group_by: Option<&str>,
    show_sub_issues: bool,
) -> Result<FilteredSet, Denial> {
    archive_filtered_set(gate, slug, query, group_by, sub_group_by, show_sub_issues)
}

/// The old distinct group buckets.
pub(crate) async fn distinct_values(
    pool: &sqlx::PgPool,
    filtered: &FilteredSet,
    field: &str,
) -> Result<Vec<String>, Denial> {
    let column = match field {
        "target_date" => "issue.target_date::text",
        "start_date" => "issue.start_date::text",
        _ => "issue.created_by_id::text",
    };
    let sql = super::distinct_group_values_sql(column, &filtered.from_where);
    let rows = bind_all(&sql, filtered.values.clone())?
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::new();
    for row in rows {
        let bucket: Option<String> = row.try_get("bucket").map_err(|_| Denial::ServerError)?;
        out.push(bucket.unwrap_or_else(|| "None".to_owned()));
    }
    Ok(out)
}

#[cfg(test)]
mod pins {
    use super::super::OneOrMany;
    use super::*;

    #[test]
    fn count_annotations_grouped_paths_render_null_when_empty() {
        // `IssueViewSet.apply_annotations` + `IssuePaginatedViewSet`: the
        // grouped `Count` subquery has no `Coalesce`, so zero related rows
        // yield NULL, not 0 (`list_issues` flat/grouped, `v2_list`).
        let selects = annotation_selects(true, None, false, true, false);
        assert_eq!(selects.matches("NULLIF(COUNT(*), 0)").count(), 3);
        assert!(!selects.contains("(SELECT COUNT(*)"));
    }

    #[test]
    fn count_annotations_scalar_paths_render_zero_when_empty() {
        // `IssueListEndpoint.get` + `IssueDetailEndpoint`: `Func(Count)` is
        // not an Aggregate, so Django emits no GROUP BY and the subquery
        // always returns one row — 0 when empty, never NULL (`flat_list`,
        // `detail_list`).
        let selects = annotation_selects(true, None, false, true, true);
        assert_eq!(selects.matches("(SELECT COUNT(*)").count(), 3);
        assert!(!selects.contains("NULLIF"));
    }

    #[test]
    fn detail_arrays_keep_deleted_guard_without_module_join() {
        // The detail prefetches (`...objects.all()`) carry the managers'
        // deleted filter but no archived-module guard.
        let guarded = annotation_selects(true, None, false, true, false);
        assert!(guarded.contains("il.deleted_at IS NULL"));
        assert!(guarded.contains("JOIN modules m ON m.id = mi.module_id"));
        assert!(guarded.contains("m.archived_at IS NULL"));
        let detail = annotation_selects(true, None, false, false, true);
        assert!(detail.contains("il.deleted_at IS NULL"));
        assert!(detail.contains("ia.deleted_at IS NULL"));
        assert!(detail.contains("mi.deleted_at IS NULL"));
        assert!(!detail.contains("JOIN modules"));
        assert!(!detail.contains("m.archived_at IS NULL"));
    }

    #[test]
    fn permission_exists_has_three_membership_branches() {
        let gate = Gate {
            user_id: uuid::Uuid::nil(),
            timezone: "UTC".parse().expect("tz"),
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            guest_scoped: true,
        };
        let mut binder = Binder::new();
        let fragment = permission_exists_fragment(&mut binder, &gate);
        assert!(fragment.starts_with("EXISTS (SELECT 1 FROM issues AS _perm"));
        assert!(fragment.contains("_perm.id = issue.id"));
        assert!(fragment.contains("_pm.role > 5"));
        assert!(fragment.contains("_pm.role = 5"));
        assert!(fragment.contains("_pp.guest_view_all_features = TRUE"));
        assert!(fragment.contains("_pp.guest_view_all_features = FALSE"));
        assert!(fragment.contains("_perm.created_by_id = $2"));
        assert_eq!(binder.values().len(), 2);
    }

    #[test]
    fn detail_filtered_set_skips_guest_scoping_for_exists() {
        let gate = Gate {
            user_id: uuid::Uuid::nil(),
            timezone: "UTC".parse().expect("tz"),
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            guest_scoped: true,
        };
        let query: QueryMap = Default::default();
        let scoped = filtered_set(
            &gate,
            "w",
            &query,
            None,
            None,
            None,
            false,
            FilterLayers::FULL,
        )
        .expect("set");
        assert!(scoped.from_where.contains("issue.created_by_id"));
        // The triage exclusion keeps NULL-state rows (nullable FK +
        // `split_exclude` semantics).
        assert!(scoped
            .from_where
            .contains("(state.\"group\" IS NULL OR NOT (state.\"group\" = 'triage'))"));
        let mut binder = Binder::new();
        let exists = permission_exists_fragment(&mut binder, &gate);
        let detail = filtered_set(
            &gate,
            "w",
            &query,
            None,
            None,
            Some((exists, binder.values())),
            true,
            FilterLayers::NO_GUEST,
        )
        .expect("set");
        assert!(!detail.from_where.contains("issue.created_by_id"));
        assert!(detail
            .from_where
            .contains("EXISTS (SELECT 1 FROM issues AS _perm"));
    }

    #[test]
    fn v2_layers_skip_filter_stacks_but_keep_guest_scope() {
        let gate = Gate {
            user_id: uuid::Uuid::nil(),
            timezone: "UTC".parse().expect("tz"),
            workspace_id: uuid::Uuid::nil(),
            project_id: uuid::Uuid::nil(),
            guest_scoped: true,
        };
        let mut query: QueryMap = Default::default();
        query.insert("state".to_owned(), OneOrMany::One("x".to_owned()));
        let v2 = filtered_set(
            &gate,
            "w",
            &query,
            None,
            None,
            None,
            false,
            FilterLayers::V2,
        )
        .expect("set");
        // Guest scoping stays; no rich/legacy predicates, no relation joins.
        assert!(v2.from_where.contains("issue.created_by_id"));
        assert!(!v2.from_where.contains("issue_labels"));
        assert!(!v2.from_where.contains("issue_assignees"));
        assert!(!v2.from_where.contains("intake_issues"));
        assert!(v2.referenced.is_empty());
        let flat = filtered_set(
            &gate,
            "w",
            &Default::default(),
            None,
            None,
            None,
            false,
            FilterLayers::NO_GUEST,
        )
        .expect("set");
        assert!(!flat.from_where.contains("issue.created_by_id"));
    }
}

/// Old engine vs new engine on one seeded dataset: every list statement,
/// byte-identical `row_to_json` output (rows), equal counts and totals.
#[cfg(test)]
pub(crate) mod live {
    use super::super::list_engine as engine;
    use super::super::{fetch_count, fetch_json_rows, OneOrMany};
    use super::*;
    use sea_query::SelectStatement;
    use serde_json::{Map, Value};
    use std::collections::BTreeMap;

    const TABLES: &[&str] = &[
        "issues",
        "projects",
        "workspaces",
        "states",
        "issue_assignees",
        "cycle_issues",
        "module_issues",
        "issue_mentions",
        "issue_labels",
        "issue_subscribers",
        "intake_issues",
        "issue_links",
        "file_assets",
        "project_members",
        "modules",
        "labels",
        "users",
        "estimate_points",
        "cycles",
        "issue_types",
        "git_issue_syncs",
        "github_issue_syncs",
    ];

    pub(crate) fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(0x5eed_0000_0000_0000_0000_0000_0000_0000 + n)
    }

    // Fixed ids.
    pub(crate) const WS: u128 = 1;
    pub(crate) const PROJECT: u128 = 2;
    pub(crate) const OTHER_PROJECT: u128 = 3;
    pub(crate) const S_BACKLOG: u128 = 10;
    pub(crate) const S_STARTED: u128 = 11;
    pub(crate) const S_DONE: u128 = 12;
    pub(crate) const S_TRIAGE: u128 = 13;
    pub(crate) const S_GONE: u128 = 14;
    pub(crate) const U_ANN: u128 = 20;
    pub(crate) const U_BOB: u128 = 21;
    pub(crate) const U_CID: u128 = 22;
    pub(crate) const L_ALPHA: u128 = 30;
    pub(crate) const L_BETA: u128 = 31;
    pub(crate) const L_GONE: u128 = 32;
    pub(crate) const M_A: u128 = 40;
    pub(crate) const M_ARCH: u128 = 41;
    pub(crate) const M_GONE: u128 = 42;
    pub(crate) const C_ONE: u128 = 50;
    pub(crate) const C_TWO: u128 = 51;
    pub(crate) const C_GONE: u128 = 52;
    pub(crate) const E_ONE: u128 = 60;
    pub(crate) const E_THREE: u128 = 61;
    pub(crate) const T_EPIC: u128 = 70;
    pub(crate) const T_TASK: u128 = 71;

    fn q(id: u128) -> String {
        format!("'{}'", uid(id))
    }

    fn opt(id: Option<u128>) -> String {
        id.map(q).unwrap_or_else(|| "NULL".to_owned())
    }

    struct Issue {
        n: u128,
        state: Option<u128>,
        priority: &'static str,
        sort: &'static str,
        start: Option<&'static str>,
        target: Option<&'static str>,
        by: u128,
        parent: Option<u128>,
        estimate: Option<u128>,
        kind: Option<u128>,
        archived: bool,
        draft: bool,
        deleted: bool,
        completed: bool,
        external: Option<&'static str>,
        project: u128,
    }

    fn issue(n: u128) -> Issue {
        Issue {
            n,
            state: Some(S_BACKLOG),
            priority: "none",
            sort: "65535",
            start: None,
            target: None,
            by: U_ANN,
            parent: None,
            estimate: None,
            kind: None,
            archived: false,
            draft: false,
            deleted: false,
            completed: false,
            external: None,
            project: PROJECT,
        }
    }

    fn issues() -> Vec<Issue> {
        vec![
            Issue {
                priority: "urgent",
                sort: "10000.5",
                start: Some("2026-01-05"),
                target: Some("2026-02-01"),
                estimate: Some(E_ONE),
                kind: Some(T_TASK),
                external: Some("github"),
                ..issue(100)
            },
            Issue {
                state: Some(S_STARTED),
                priority: "high",
                sort: "20000",
                start: Some("2026-03-01"),
                by: U_BOB,
                kind: Some(T_EPIC),
                external: Some(""),
                ..issue(101)
            },
            Issue {
                state: Some(S_DONE),
                priority: "low",
                sort: "30000.25",
                target: Some("2026-04-01"),
                completed: true,
                estimate: Some(E_THREE),
                ..issue(102)
            },
            Issue {
                state: None,
                priority: "medium",
                sort: "40000",
                parent: Some(100),
                external: Some("git"),
                ..issue(103)
            },
            Issue {
                state: Some(S_TRIAGE),
                ..issue(104)
            },
            Issue {
                state: Some(S_GONE),
                priority: "high",
                start: Some("2026-01-05"),
                by: U_BOB,
                parent: Some(100),
                ..issue(105)
            },
            Issue {
                archived: true,
                priority: "urgent",
                start: Some("2026-02-02"),
                ..issue(106)
            },
            Issue {
                archived: true,
                parent: Some(100),
                kind: Some(T_EPIC),
                ..issue(107)
            },
            Issue {
                archived: true,
                by: U_BOB,
                kind: Some(T_TASK),
                state: Some(S_TRIAGE),
                ..issue(108)
            },
            Issue {
                draft: true,
                ..issue(109)
            },
            Issue {
                deleted: true,
                ..issue(110)
            },
            Issue {
                priority: "low",
                sort: "50000",
                target: Some("2026-02-01"),
                by: U_CID,
                kind: Some(T_EPIC),
                ..issue(111)
            },
            Issue {
                state: Some(S_STARTED),
                priority: "none",
                sort: "-5",
                start: Some("2025-12-31"),
                ..issue(112)
            },
            Issue {
                project: OTHER_PROJECT,
                ..issue(113)
            },
            Issue {
                state: Some(S_DONE),
                priority: "urgent",
                sort: "70000",
                parent: Some(102),
                by: U_BOB,
                ..issue(114)
            },
        ]
    }

    pub(crate) async fn seed(pool: &sqlx::PgPool) {
        for table in TABLES {
            sqlx::query(&format!(
                "CREATE TEMP TABLE {table} AS SELECT * FROM public.{table} WITH NO DATA"
            ))
            .execute(pool)
            .await
            .expect("temp table");
        }
        let mut sql = vec![
            format!("INSERT INTO workspaces (id, slug) VALUES ({}, 'acme')", q(WS)),
            format!(
                "INSERT INTO projects (id, workspace_id, archived_at, guest_view_all_features) VALUES \
                 ({}, {}, NULL, false), ({}, {}, NULL, true)",
                q(PROJECT), q(WS), q(OTHER_PROJECT), q(WS)
            ),
            format!(
                "INSERT INTO states (id, name, \"group\", project_id, workspace_id, deleted_at) VALUES \
                 ({}, 'Backlog', 'backlog', {p}, {w}, NULL), ({}, 'Doing', 'started', {p}, {w}, NULL), \
                 ({}, 'Done', 'completed', {p}, {w}, NULL), ({}, 'Triage', 'triage', {p}, {w}, NULL), \
                 ({}, 'Gone', 'unstarted', {p}, {w}, now())",
                q(S_BACKLOG), q(S_STARTED), q(S_DONE), q(S_TRIAGE), q(S_GONE), p = q(PROJECT), w = q(WS)
            ),
            format!(
                "INSERT INTO users (id, first_name) VALUES ({}, 'Ann'), ({}, 'Bob'), ({}, 'Cid')",
                q(U_ANN), q(U_BOB), q(U_CID)
            ),
            format!(
                "INSERT INTO project_members (id, project_id, workspace_id, member_id, role, is_active, deleted_at) VALUES \
                 (gen_random_uuid(), {p}, {w}, {}, 20, true, NULL), (gen_random_uuid(), {p}, {w}, {}, 5, true, NULL), \
                 (gen_random_uuid(), {p}, {w}, {}, 15, false, NULL)",
                q(U_ANN), q(U_BOB), q(U_CID), p = q(PROJECT), w = q(WS)
            ),
            format!(
                "INSERT INTO labels (id, name, project_id, workspace_id, deleted_at) VALUES \
                 ({}, 'alpha', {p}, {w}, NULL), ({}, 'beta', {p}, {w}, NULL), ({}, 'gamma', {p}, {w}, now())",
                q(L_ALPHA), q(L_BETA), q(L_GONE), p = q(PROJECT), w = q(WS)
            ),
            format!(
                "INSERT INTO modules (id, name, project_id, workspace_id, archived_at, deleted_at) VALUES \
                 ({}, 'Mod A', {p}, {w}, NULL, NULL), ({}, 'Mod Arch', {p}, {w}, now(), NULL), ({}, 'Mod Gone', {p}, {w}, NULL, now())",
                q(M_A), q(M_ARCH), q(M_GONE), p = q(PROJECT), w = q(WS)
            ),
            format!(
                "INSERT INTO cycles (id, name, project_id, workspace_id, deleted_at) VALUES \
                 ({}, 'Cycle 1', {p}, {w}, NULL), ({}, 'Cycle 2', {p}, {w}, NULL), ({}, 'Cycle Gone', {p}, {w}, now())",
                q(C_ONE), q(C_TWO), q(C_GONE), p = q(PROJECT), w = q(WS)
            ),
            format!(
                "INSERT INTO estimate_points (id, key) VALUES ({}, 1), ({}, 3)",
                q(E_ONE), q(E_THREE)
            ),
            format!(
                "INSERT INTO issue_types (id, is_epic) VALUES ({}, true), ({}, false)",
                q(T_EPIC), q(T_TASK)
            ),
        ];
        for (index, row) in issues().iter().enumerate() {
            let at = |minutes: usize| {
                format!("timestamptz '2026-10-01 08:00:00.123456+00' + interval '{minutes} min'")
            };
            sql.push(format!(
                "INSERT INTO issues (id, name, workspace_id, project_id, state_id, sort_order, priority, \
                 complexity_score, sequence_id, start_date, target_date, created_by_id, updated_by_id, parent_id, \
                 estimate_point_id, type_id, created_at, updated_at, completed_at, archived_at, deleted_at, \
                 is_draft, external_source, description_html) VALUES \
                 ({id}, 'Issue {n} 50%_off', {ws}, {project}, {state}, {sort}, '{priority}', 2, {n}, {start}, {target}, \
                 {by}, {by}, {parent}, {estimate}, {kind}, {created}, {updated}, {completed}, {archived}, {deleted}, \
                 {draft}, {external}, '<p>{n}</p>')",
                id = q(row.n),
                n = row.n,
                ws = q(WS),
                project = q(row.project),
                state = opt(row.state),
                sort = row.sort,
                priority = row.priority,
                start = row.start.map_or("NULL".to_owned(), |d| format!("'{d}'")),
                target = row.target.map_or("NULL".to_owned(), |d| format!("'{d}'")),
                by = q(row.by),
                parent = opt(row.parent),
                estimate = opt(row.estimate),
                kind = opt(row.kind),
                created = at(index * 7),
                updated = at(1000 - index * 3),
                completed = if row.completed { at(500) } else { "NULL".to_owned() },
                archived = if row.archived { "'2026-10-20'".to_owned() } else { "NULL".to_owned() },
                deleted = if row.deleted { at(900) } else { "NULL".to_owned() },
                draft = row.draft,
                external = row.external.map_or("NULL".to_owned(), |e| format!("'{e}'")),
            ));
        }
        let link = |table: &str, column: &str, issue: u128, target: u128, deleted: bool| {
            format!(
                "INSERT INTO {table} (id, issue_id, {column}, project_id, workspace_id, deleted_at) VALUES \
                 (gen_random_uuid(), {}, {}, {}, {}, {})",
                q(issue), q(target), q(PROJECT), q(WS), if deleted { "now()" } else { "NULL" }
            )
        };
        for (issue, user, deleted) in [
            (100, U_ANN, false),
            (100, U_BOB, false),
            (101, U_BOB, false),
            (102, U_CID, false),
            (103, U_ANN, true),
            (106, U_ANN, false),
            (111, U_ANN, false),
            (114, U_BOB, false),
        ] {
            sql.push(link("issue_assignees", "assignee_id", issue, user, deleted));
        }
        for (issue, label, deleted) in [
            (100, L_ALPHA, false),
            (100, L_BETA, false),
            (101, L_GONE, false),
            (102, L_ALPHA, true),
            (103, L_BETA, false),
            (107, L_ALPHA, false),
            (112, L_BETA, false),
        ] {
            sql.push(link("issue_labels", "label_id", issue, label, deleted));
        }
        for (issue, cycle, deleted) in [
            (100, C_ONE, false),
            (100, C_TWO, true),
            (101, C_TWO, false),
            (103, C_GONE, false),
            (106, C_ONE, false),
            (112, C_ONE, false),
        ] {
            sql.push(link("cycle_issues", "cycle_id", issue, cycle, deleted));
        }
        for (issue, module, deleted) in [
            (100, M_A, false),
            (100, M_ARCH, false),
            (101, M_GONE, false),
            (102, M_A, true),
            (111, M_A, false),
            (107, M_A, false),
        ] {
            sql.push(link("module_issues", "module_id", issue, module, deleted));
        }
        for (issue, user) in [(100, U_CID), (102, U_ANN)] {
            sql.push(link("issue_mentions", "mention_id", issue, user, false));
        }
        for (issue, user, deleted) in [(100, U_BOB, false), (101, U_ANN, true), (111, U_BOB, false)]
        {
            sql.push(link(
                "issue_subscribers",
                "subscriber_id",
                issue,
                user,
                deleted,
            ));
        }
        for (issue, status) in [(100, 1), (101, 0), (102, -2), (112, -1)] {
            sql.push(format!(
                "INSERT INTO intake_issues (id, issue_id, status) VALUES (gen_random_uuid(), {}, {status})",
                q(issue)
            ));
        }
        for (issue, deleted) in [(100, false), (100, false), (101, true), (103, false)] {
            sql.push(format!(
                "INSERT INTO issue_links (id, issue_id, deleted_at) VALUES (gen_random_uuid(), {}, {})",
                q(issue), if deleted { "now()" } else { "NULL" }
            ));
        }
        for (issue, kind, deleted) in [
            (100, "ISSUE_ATTACHMENT", false),
            (102, "ISSUE_ATTACHMENT", true),
            (102, "ISSUE_DESCRIPTION", false),
            (111, "ISSUE_ATTACHMENT", false),
        ] {
            sql.push(format!(
                "INSERT INTO file_assets (id, issue_id, entity_type, deleted_at) VALUES (gen_random_uuid(), {}, '{kind}', {})",
                q(issue), if deleted { "now()" } else { "NULL" }
            ));
        }
        sql.push(format!(
            "INSERT INTO github_issue_syncs (id, issue_id, deleted_at) VALUES (gen_random_uuid(), {}, NULL)",
            q(100)
        ));
        sql.push(format!(
            "INSERT INTO git_issue_syncs (id, issue_id, deleted_at) VALUES (gen_random_uuid(), {}, now())",
            q(103)
        ));
        for statement in sql {
            sqlx::query(&statement)
                .execute(pool)
                .await
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }
    }

    pub(crate) fn gate(user: u128, guest_scoped: bool) -> Gate {
        Gate {
            user_id: uid(user),
            timezone: "Asia/Kolkata".parse().expect("tz"),
            workspace_id: uid(WS),
            project_id: uid(PROJECT),
            guest_scoped,
        }
    }

    fn query(pairs: &[(&str, String)]) -> QueryMap {
        let mut out = QueryMap::default();
        for (key, value) in pairs {
            out.insert((*key).to_owned(), OneOrMany::One(value.clone()));
        }
        out
    }

    /// The filter mixes the variants run under.
    fn queries() -> Vec<(&'static str, QueryMap)> {
        let id = |n: u128| uid(n).to_string();
        vec![
            ("none", query(&[])),
            (
                "legacy",
                query(&[
                    ("priority", "urgent,high,low,none".to_owned()),
                    ("labels", format!("{},{}", id(L_ALPHA), id(L_BETA))),
                    ("start_date", "2025-12-01;after".to_owned()),
                ]),
            ),
            (
                "legacy_nulls",
                query(&[
                    ("cycle", "null".to_owned()),
                    ("assignees", format!("null,{}", id(U_ANN))),
                    ("sub_issue", "false".to_owned()),
                ]),
            ),
            (
                "legacy_dates",
                query(&[
                    ("created_at", "2026-10-01;after".to_owned()),
                    ("target_date", "2026-03-01;before".to_owned()),
                    ("name", "50%_".to_owned()),
                    ("state_group", "backlog,started,completed".to_owned()),
                    ("intake_status", "1,-1,0".to_owned()),
                ]),
            ),
            (
                "legacy_misc",
                query(&[
                    ("state", format!("{},{}", id(S_BACKLOG), id(S_STARTED))),
                    ("created_by", id(U_ANN)),
                    ("module", format!("{},null", id(M_A))),
                    ("mentions", id(U_CID)),
                    ("subscriber", id(U_BOB)),
                    ("start_target_date", "true".to_owned()),
                    ("estimate_point", id(E_ONE)),
                ]),
            ),
            (
                "rich",
                query(&[(
                    "filters",
                    format!(
                        r#"{{"and":[{{"assignee_id__in":"{},{}"}},{{"or":[{{"label_id":"{}"}},{{"priority":"urgent"}}]}}]}}"#,
                        id(U_ANN),
                        id(U_BOB),
                        id(L_ALPHA)
                    ),
                )]),
            ),
            (
                "rich_not",
                query(&[(
                    "filters",
                    format!(r#"{{"not":{{"cycle_id":"{}"}}}}"#, id(C_ONE)),
                )]),
            ),
            (
                "rich_mixed",
                query(&[
                    (
                        "filters",
                        format!(
                            r#"{{"and":[{{"state_group__in":"backlog,started"}},{{"start_date__range":"2025-01-01,2026-12-31"}},{{"subscriber_id":"{}"}},{{"module_id__in":"{}"}},{{"mention_id":""}}]}}"#,
                            id(U_BOB),
                            id(M_A)
                        ),
                    ),
                    ("priority", "urgent,low".to_owned()),
                ]),
            ),
            (
                "updated",
                query(&[("updated_at__gt", "2026-10-01T23:00:00Z".to_owned())]),
            ),
        ]
    }

    const ORDERS: &[&str] = &[
        "-created_at",
        "created_at",
        "-updated_at",
        "sort_order",
        "-sort_order",
        "priority",
        "-priority",
        "state__group",
        "-state__group",
        "labels__name",
        "-assignees__first_name",
        "issue_module__module__name",
        "start_date",
        "-target_date",
        "name",
        "sequence_id",
        "-completed_at",
        "estimate_point",
        "estimate_point__key",
        "state__name",
        "-cycle_id",
        "link_count",
        "-attachment_count",
        "sub_issues_count",
        "-sub_issues_count",
        "issue_cycle__cycle__name",
        "-issue_cycle__cycle__name",
        "created_by",
        "-project",
        "parent",
        "state",
        "-id",
    ];

    const GROUPS: &[&str] = &[
        "state_id",
        "priority",
        "state__group",
        "labels__id",
        "assignees__id",
        "issue_module__module_id",
        "cycle_id",
        "project_id",
        "target_date",
        "start_date",
        "created_by",
    ];

    async fn old_rows(pool: &sqlx::PgPool, sql: &str, values: Vec<sea_query::Value>) -> String {
        let rows = fetch_json_rows(pool, sql, values)
            .await
            .unwrap_or_else(|_| panic!("old rows: {sql}"));
        serde_json::to_string(&rows).expect("json")
    }

    async fn new_rows(pool: &sqlx::PgPool, stmt: SelectStatement) -> String {
        let sql = stmt.to_string(sea_query::PostgresQueryBuilder);
        let rows = engine::fetch_rows(pool, stmt)
            .await
            .unwrap_or_else(|_| panic!("new rows: {sql}"));
        serde_json::to_string(&rows).expect("json")
    }

    /// Rows bucketed by partition (relative order kept): the grouper's
    /// view. Same-issue rows in different m2m buckets tie on the global
    /// order, so only the per-partition order is deterministic.
    fn by_partition(
        json: &str,
        group_by: &str,
        sub: Option<&str>,
    ) -> BTreeMap<String, Vec<String>> {
        let rows: Vec<Map<String, Value>> = serde_json::from_str(json).expect("rows");
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for row in rows {
            let key = format!(
                "{}|{}",
                row.get(group_by).cloned().unwrap_or(Value::Null),
                sub.and_then(|s| row.get(s).cloned()).unwrap_or(Value::Null)
            );
            out.entry(key)
                .or_default()
                .push(serde_json::to_string(&row).expect("row"));
        }
        out
    }

    fn sorted<T: Ord>(mut items: Vec<T>) -> Vec<T> {
        items.sort();
        items
    }

    struct Pair {
        old: FilteredSet,
        new: engine::IssueSet,
    }

    /// Old and new filtered sets for one (layers, extra) variant.
    #[allow(clippy::too_many_arguments)]
    fn pair(
        gate: &Gate,
        query: &QueryMap,
        group_by: Option<&str>,
        sub_group_by: Option<&str>,
        old_extra: Option<(String, Vec<sea_query::Value>)>,
        new_extra: Option<sea_query::SimpleExpr>,
        detail: bool,
        layers: FilterLayers,
    ) -> Result<Pair, (Denial, Denial)> {
        let old = filtered_set(
            gate,
            "acme",
            query,
            group_by,
            sub_group_by,
            old_extra,
            detail,
            layers,
        );
        let new = engine::issue_set(
            gate,
            "acme",
            query,
            group_by,
            sub_group_by,
            new_extra,
            detail,
            layers,
        );
        match (old, new) {
            (Ok(old), Ok(new)) => Ok(Pair { old, new }),
            (Err(old), Err(new)) => Err((old, new)),
            (old, new) => panic!(
                "old/new disagree on errors: {:?} vs {:?}",
                old.err(),
                new.err()
            ),
        }
    }

    fn order_spec(param: &str) -> OrderSpec {
        pidash_services::app_issues::order_sql(param, "state.\"group\"", |name| {
            format!("min_{}", name.replace("__", "_"))
        })
    }

    /// Flat pages (two windows), counts, the fan-out count, for each order.
    async fn check_flat(
        pool: &sqlx::PgPool,
        label: &str,
        pair: &Pair,
        selects: &str,
        ann: engine::Annotations,
        null_counts: bool,
    ) -> usize {
        let mut checks = 0;
        assert_eq!(
            fetch_count(
                pool,
                &count_distinct_sql(&pair.old),
                pair.old.values.clone()
            )
            .await
            .expect("old count"),
            engine::fetch_n(pool, &engine::count_distinct(&pair.new))
                .await
                .expect("new count"),
            "{label}: count"
        );
        for param in ORDERS {
            let spec = order_spec(param);
            let old_key =
                order_key(&spec.out_param, param, Some("issue_cycle"), null_counts).expect("key");
            let new_key =
                engine::order_expr(&spec.out_param, param, Some("issue_cycle"), null_counts)
                    .expect("key");
            for (limit, offset) in [(4, 3), (1000, 0)] {
                let old = old_rows(
                    pool,
                    &flat_page_sql(&pair.old, selects, &old_key, limit, offset),
                    pair.old.values.clone(),
                )
                .await;
                let new = new_rows(
                    pool,
                    engine::flat_page(&pair.new, ann, &new_key, limit, offset).expect("page"),
                )
                .await;
                assert_eq!(old, new, "{label}: flat page {param} {limit}/{offset}");
                checks += 1;
            }
            if !old_key.joins.is_empty() {
                assert_eq!(
                    fetch_count(
                        pool,
                        &count_rows_sql(&pair.old, &old_key.joins),
                        pair.old.values.clone()
                    )
                    .await
                    .expect("old"),
                    engine::fetch_n(pool, &engine::count_rows(&pair.new, new_key.joins))
                        .await
                        .expect("new"),
                    "{label}: fan-out count {param}"
                );
            }
        }
        checks
    }

    /// Grouped windows, totals, sub-totals and buckets for every group.
    async fn check_grouped(
        pool: &sqlx::PgPool,
        label: &str,
        gate: &Gate,
        query: &QueryMap,
        old_extra: &Option<(String, Vec<sea_query::Value>)>,
        new_extra: &Option<sea_query::SimpleExpr>,
        archive: bool,
    ) -> usize {
        let mut checks = 0;
        let subs: Vec<Option<&str>> = vec![
            None,
            Some("priority"),
            Some("labels__id"),
            Some("state__group"),
        ];
        for group in GROUPS {
            for sub in &subs {
                if *sub == Some(*group) {
                    continue;
                }
                let pair = if archive {
                    Pair {
                        old: archive_set(gate, "acme", query, Some(group), *sub, true)
                            .expect("old"),
                        new: engine::archive_issue_set(
                            gate,
                            "acme",
                            query,
                            Some(group),
                            *sub,
                            true,
                        )
                        .expect("new"),
                    }
                } else {
                    match pair(
                        gate,
                        query,
                        Some(group),
                        *sub,
                        old_extra.clone(),
                        new_extra.clone(),
                        false,
                        FilterLayers::FULL,
                    ) {
                        Ok(pair) => pair,
                        Err(_) => return checks,
                    }
                };
                let skip = match *group {
                    "labels__id" => Some("label_ids"),
                    "assignees__id" => Some("assignee_ids"),
                    "issue_module__module_id" => Some("module_ids"),
                    _ => None,
                };
                let selects = annotation_selects(true, skip, false, true, false);
                let ann = engine::Annotations {
                    skip_array: skip,
                    ..super::super::LIST_ANNOTATIONS
                };
                for param in [
                    "priority",
                    "-issue_cycle__cycle__name",
                    "labels__name",
                    "sub_issues_count",
                ] {
                    let spec = order_spec(param);
                    let old_key =
                        order_key(&spec.out_param, param, Some("issue_cycle"), true).expect("key");
                    let new_key =
                        engine::order_expr(&spec.out_param, param, Some("issue_cycle"), true)
                            .expect("key");
                    for (offset, stop) in [(2, 5), (0, 1001)] {
                        let old = old_rows(
                            pool,
                            &grouped_page_sql(
                                &pair.old, group, *sub, &selects, &old_key, offset, stop,
                            )
                            .expect("old"),
                            pair.old.values.clone(),
                        )
                        .await;
                        let new = new_rows(
                            pool,
                            engine::grouped_page(
                                &pair.new, group, *sub, ann, &new_key, offset, stop,
                            )
                            .expect("new"),
                        )
                        .await;
                        assert_eq!(
                            by_partition(&old, group, *sub),
                            by_partition(&new, group, *sub),
                            "{label}: grouped {group}/{sub:?} {param} {offset}..{stop}"
                        );
                        checks += 1;
                    }
                }
                assert_eq!(
                    sorted(
                        group_totals(pool, &pair.old, group)
                            .await
                            .expect("old totals")
                    ),
                    sorted(
                        engine::fetch_group_totals(
                            pool,
                            &engine::group_totals(&pair.new, group).expect("stmt")
                        )
                        .await
                        .expect("new totals")
                    ),
                    "{label}: totals {group}"
                );
                if let Some(sub) = sub {
                    assert_eq!(
                        sorted(
                            super::sub_totals(pool, &pair.old, group, sub)
                                .await
                                .expect("old subs")
                        ),
                        sorted(
                            engine::fetch_sub_totals(
                                pool,
                                &engine::sub_totals(&pair.new, group, sub).expect("stmt")
                            )
                            .await
                            .expect("new subs")
                        ),
                        "{label}: sub totals {group}/{sub}"
                    );
                }
                if let Some(stmt) = engine::distinct_bucket_values(&pair.new, group) {
                    assert_eq!(
                        distinct_values(pool, &pair.old, group)
                            .await
                            .expect("old buckets"),
                        engine::fetch_buckets(pool, &stmt)
                            .await
                            .expect("new buckets"),
                        "{label}: buckets {group}"
                    );
                }
                checks += 3;
            }
        }
        checks
    }

    #[tokio::test]
    async fn live_engine_matches_text_engine() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping live-db test: DATABASE_URL is not set");
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect");
        seed(&pool).await;
        let mut checks = 0usize;
        let all_ids: Vec<uuid::Uuid> = (100..=114).map(uid).collect();
        for (label, query) in queries() {
            for guest in [false, true] {
                let gate = gate(if guest { U_BOB } else { U_ANN }, guest);
                let label = format!("{label}/guest={guest}");
                // Main list (and epics) — flat page + grouped.
                for epic in [false, true] {
                    let label = format!("{label}/epic={epic}");
                    let old_extra = list_extra(&query, epic).expect("old extra");
                    let updated = engine::updated_at_gt(&query).expect("updated");
                    let new_extra = match (updated, epic) {
                        (Some(u), true) => Some(u.and(engine::epic_only())),
                        (Some(u), false) => Some(u),
                        (None, true) => Some(engine::epic_only()),
                        (None, false) => None,
                    };
                    let Ok(main) = pair(
                        &gate,
                        &query,
                        None,
                        None,
                        old_extra.clone(),
                        new_extra.clone(),
                        false,
                        FilterLayers::FULL,
                    ) else {
                        continue;
                    };
                    let selects = annotation_selects(true, None, false, true, false);
                    checks += check_flat(
                        &pool,
                        &format!("{label}/main"),
                        &main,
                        &selects,
                        super::super::LIST_ANNOTATIONS,
                        true,
                    )
                    .await;
                    checks += check_grouped(
                        &pool,
                        &format!("{label}/main"),
                        &gate,
                        &query,
                        &old_extra,
                        &new_extra,
                        false,
                    )
                    .await;
                }
                // Detail list.
                let detail_ann = engine::Annotations {
                    arrays: true,
                    skip_array: None,
                    assignee_active_member: false,
                    module_archived_guard: false,
                    scalar_counts: true,
                };
                for user in [U_ANN, U_BOB, U_CID] {
                    let gate = Gate {
                        user_id: uid(user),
                        ..gate
                    };
                    if let Ok(detail) = pair(
                        &gate,
                        &query,
                        None,
                        None,
                        Some(detail_extra(&gate)),
                        Some(engine::permission_exists(&gate)),
                        true,
                        FilterLayers::NO_GUEST,
                    ) {
                        let selects = annotation_selects(true, None, false, false, true);
                        checks += check_flat(
                            &pool,
                            &format!("{label}/detail/{user}"),
                            &detail,
                            &selects,
                            detail_ann,
                            false,
                        )
                        .await;
                    }
                }
                // Flat `.values()` list over an id set.
                if let Ok(flat) = pair(
                    &gate,
                    &query,
                    None,
                    None,
                    Some(ids_extra(&all_ids)),
                    Some(engine::id_in(all_ids.clone())),
                    false,
                    FilterLayers::NO_GUEST,
                ) {
                    for param in ORDERS {
                        let spec = order_spec(param);
                        let old = old_rows(
                            &pool,
                            &flat_list_sql(&flat.old, &spec, param).expect("old"),
                            flat.old.values.clone(),
                        )
                        .await;
                        let new = new_rows(
                            &pool,
                            engine::flat_list_rows(&flat.new, &spec.out_param, param).expect("new"),
                        )
                        .await;
                        assert_eq!(old, new, "{label}: flat list {param}");
                        checks += 1;
                    }
                }
                // Serializer branch (rich only).
                if let Ok(ser) = pair(
                    &gate,
                    &query,
                    None,
                    None,
                    Some(ids_extra(&all_ids)),
                    Some(engine::id_in(all_ids.clone())),
                    false,
                    FilterLayers::SERIALIZER,
                ) {
                    let old =
                        old_rows(&pool, &serializer_sql(&ser.old), ser.old.values.clone()).await;
                    let new = new_rows(&pool, engine::serializer_rows(&ser.new)).await;
                    assert_eq!(old, new, "{label}: serializer");
                    checks += 1;
                }
                // v2 (no filter stacks).
                let v2_old_extra = list_extra(&query, false).expect("extra");
                let v2_new_extra = engine::updated_at_gt(&query).expect("extra");
                if let Ok(v2) = pair(
                    &gate,
                    &query,
                    None,
                    None,
                    v2_old_extra,
                    v2_new_extra,
                    false,
                    FilterLayers::V2,
                ) {
                    assert_eq!(
                        fetch_count(&pool, &count_distinct_sql(&v2.old), v2.old.values.clone())
                            .await
                            .expect("old"),
                        engine::fetch_n(&pool, &engine::count_distinct(&v2.new))
                            .await
                            .expect("new"),
                        "{label}: v2 count"
                    );
                    for (limit, offset) in [(3, 0), (3, 3), (1000, 0)] {
                        let old = old_rows(
                            &pool,
                            &v2_sql(&v2.old, limit, offset),
                            v2.old.values.clone(),
                        )
                        .await;
                        let new =
                            new_rows(&pool, engine::v2_rows(&v2.new, limit, offset).expect("new"))
                                .await;
                        assert_eq!(old, new, "{label}: v2 {limit}/{offset}");
                        checks += 1;
                    }
                }
                // Deleted ids.
                let (sql, values) = deleted_sql(&gate, "acme", &query).expect("old");
                let old: Vec<String> = fetch_json_rows(&pool, &sql, values)
                    .await
                    .expect("old deleted")
                    .into_iter()
                    .map(|row| row["id"].as_str().expect("id").to_owned())
                    .collect();
                let new = engine::fetch_ids(
                    &pool,
                    &engine::deleted_ids(&gate, "acme", engine::updated_at_gt(&query).expect("u")),
                )
                .await
                .expect("new deleted");
                assert_eq!(old, new, "{label}: deleted");
                // Archive list.
                for show in [true, false] {
                    let old = archive_set(&gate, "acme", &query, None, None, show);
                    let new = engine::archive_issue_set(&gate, "acme", &query, None, None, show);
                    let (Ok(old), Ok(new)) = (old, new) else {
                        continue;
                    };
                    let archive = Pair { old, new };
                    let selects = annotation_selects(true, None, false, true, false);
                    checks += check_flat(
                        &pool,
                        &format!("{label}/archive/{show}"),
                        &archive,
                        &selects,
                        super::super::LIST_ANNOTATIONS,
                        true,
                    )
                    .await;
                }
                if !guest {
                    checks += check_grouped(
                        &pool,
                        &format!("{label}/archive"),
                        &gate,
                        &query,
                        &None,
                        &None,
                        true,
                    )
                    .await;
                }
            }
        }
        // The dataset must actually exercise the lists.
        let none = pair(
            &gate(U_ANN, false),
            &QueryMap::default(),
            None,
            None,
            None,
            None,
            false,
            FilterLayers::FULL,
        )
        .unwrap_or_else(|_| panic!("set"));
        let count = engine::fetch_n(&pool, &engine::count_distinct(&none.new))
            .await
            .expect("count");
        assert!(count >= 6, "seeded list too small: {count}");
        eprintln!("live_engine_matches_text_engine: {checks} differential checks");
        assert!(checks > 1000, "{checks}");
    }

    /// Error parity: invalid legacy and rich inputs fail the same way.
    #[test]
    fn engine_errors_match_text_engine() {
        let gate = gate(U_ANN, false);
        for pairs in [
            vec![("logged_by", uid(U_ANN).to_string())],
            vec![("intake_status", "x".to_owned())],
            vec![("estimate_point", "not-a-uuid".to_owned())],
            vec![("filters", "{bad".to_owned())],
            vec![("filters", r#"{"nope":"1"}"#.to_owned())],
            vec![(
                "filters",
                r#"{"start_date__range":"2026-01-01"}"#.to_owned(),
            )],
            vec![("updated_at__gt", "garbage".to_owned())],
        ] {
            let query = query(&pairs);
            let old = filtered_set(
                &gate,
                "acme",
                &query,
                None,
                None,
                None,
                false,
                FilterLayers::FULL,
            )
            .map(|_| ())
            .and_then(|_| list_extra(&query, false).map(|_| ()));
            let new = engine::issue_set(
                &gate,
                "acme",
                &query,
                None,
                None,
                None,
                false,
                FilterLayers::FULL,
            )
            .map(|_| ())
            .and_then(|_| engine::updated_at_gt(&query).map(|_| ()));
            assert_eq!(format!("{old:?}"), format!("{new:?}"), "{pairs:?}");
        }
    }
}
