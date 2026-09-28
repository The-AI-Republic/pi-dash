//! Space public issue-list read query: `ProjectIssuesPublicEndpoint.get`.
//!
//! Port of `apps/api/pi_dash/space/views/issue.py:76-211`
//! (`ProjectIssuesPublicEndpoint`, `permission_classes = [AllowAny]` at
//! `:74`) plus, as used by this endpoint, `space/utils/grouper.py:28-252`
//! (`issue_queryset_grouper`, `issue_on_results`, `issue_group_values`),
//! `pi_dash/utils/issue_filters.py:431-466` (`issue_filters(params, "GET")`),
//! `pi_dash/utils/order_queryset.py:14-58` (`order_issue_queryset`, default
//! `"-created_at"` from `views/issue.py:78`), and
//! `pi_dash/utils/paginator.py:194-632` (`GroupedOffsetPaginator`,
//! `SubGroupedOffsetPaginator`, and the 12-key `BasePaginator.paginate`
//! envelope at `:654+`).
//! Fixture record `rust-api/fixtures/space/queries/issue_list.{sql,rows.json}`
//! (filed by PIDASHCONV-135; trace: `space/views/issue.py:76-211`,
//! `space/utils/grouper.py:28-70,:73-182,:185-252`).
//!
//! Conventions (same as [`super::project_meta`], which owns the shared board
//! closures this module reuses by equality test):
//!
//! * Builders return the SQL text with Postgres `$N` placeholders in first-
//!   appearance order (Django renders `%s` / `%(name)s`; same binding order).
//!   Execution belongs to the handlers layer (PIDASHCONV-175), which binds
//!   the documented `$N` params in order and maps row counts onto the `get()`
//!   contract (`0 -> DoesNotExist`, `>1 -> MultipleObjectsReturned`).
//! * `.first()` board lookups keep the default `Meta.ordering =
//!   ("-created_at",)` (`db/models/deploy_board.py:57`) with `LIMIT 1`.
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings; rows keep them as `String`. UUID and FK keys render as strings.
//! * Error bodies and status codes belong to the guards layer, not here; the
//!   Python line for each is cited so handlers wire the same mapping.
//! * `select_related` joins (workspace, project, state, parent) change no SQL
//!   fanout — one row per issue — so they contribute no builder; the
//!   `prefetch_related` halves (assignees, labels, `issue_module__module`,
//!   `issue_reactions`-with-actor, `votes`-with-actor) are separate queries
//!   whose shapes are owned by the serializers/models layers. What this
//!   module pins is everything that changes the list statement or its rows:
//!   the board lookup, the base `WHERE`, the four annotation subqueries, the
//!   ordering, the grouper (filters + always-on annotations), the grouped
//!   window pagination, the group-values branches, and the `on_results`
//!   projection with its vote/reaction items.
//!
//! # Ported bugs and quirks (translate, don't redesign)
//!
//! * BUG-always-annotate (`space/utils/grouper.py:67`): the
//!   `default_annotations` guard reads `if FIELD_MAPPER.get(key) != group_by
//!   or FIELD_MAPPER.get(key) != sub_group_by` — `or`, not `and` — so it is
//!   always true and ALL three id-list annotations are applied on every path,
//!   grouped or not. [`default_annotation_keys`] ports the `or` literally.
//! * BUG-assignees-unwrapped (`space/utils/grouper.py:204-208`):
//!   `issue_group_values("assignees__id", ..., project_id=<set>)` returns the
//!   bare `values_list` QuerySet with NO `list()` wrap (every other branch
//!   wraps). [`group_values_needs_list_wrap`] returns `false` for exactly
//!   that case so the handler preserves the lazy-QuerySet semantics.
//! * BUG-group-values-no-queryset (`space/utils/grouper.py:233-250`): the
//!   `target_date` / `start_date` / `created_by` branches call
//!   `queryset.values_list(...)` on the default `queryset=None`. This endpoint
//!   always calls `issue_group_values` without `queryset`
//!   (`views/issue.py:156-167,189-194`), so grouping by any of those three
//!   fields raises `AttributeError` (500 via the dispatch `return exc` bug,
//!   `space/views/base.py:199-200`). [`group_values_requires_queryset`]
//!   marks those branches; the builders still emit the intended SQL for the
//!   follow-up fix only.
//! * QUIRK-filtered-out-updated-at (`pi_dash/utils/issue_filters.py:230-238`):
//!   `filter_updated_at` writes the `created_at__date` term, not
//!   `updated_at__date` — an `updated_at` query param filters `created_at`.
//!   [`compile_issue_filters`] keeps that mapping.
//! * QUIRK-intake-status-fallback (`issue_filters.py:353-365`): the POST
//!   branch of `filter_intake_status` reads `params.get("inbox_status")`,
//!   not `"intake_status"`. This endpoint only uses `"GET"`, so the quirk
//!   never fires here; it is recorded so a shared compiler does not
//!   "fix" it.
//! * QUIRK-count-filter-intake (`views/issue.py:170-177,196-203`): the
//!   grouped totals `count_filter` counts intake rows with `status IN
//!   (1, -1, 2)` OR no intake row at all — status `0` (pending triage) and
//!   any other status contribute zero to `total_results` while the row still
//!   appears in `results`. [`count_filter_sql`] keeps that predicate as-is.

use serde::{Deserialize, Serialize};

use super::intake_assets::asset_board_first_scoped_sql;

// ---------------------------------------------------------------------------
// L1 anchor lookup (views/issue.py:80-85)
// ---------------------------------------------------------------------------

/// Anchor lookup: `DeployBoard.objects.filter(anchor=anchor,
/// entity_name="project").first()` (`views/issue.py:80`).
/// `project_id = deploy_board.entity_identifier` (`:84`),
/// `slug = deploy_board.workspace.slug` (`:85`).
/// Text-identical to the asset scoped board-first closure; `$1` = anchor.
pub fn issue_list_board_sql() -> String {
    asset_board_first_scoped_sql()
}

/// Bad anchor body: `{"error": "Project is not published"}`, 404
/// (`views/issue.py:81-82`).
pub const NOT_PUBLISHED_BODY: &str = r#"{"error": "Project is not published"}"#;

// ---------------------------------------------------------------------------
// L2 base queryset (views/issue.py:87-126)
// ---------------------------------------------------------------------------

/// `Issue.issue_objects` manager conjuncts (`db/models/issue.py:95-104`):
/// not soft-deleted, state group is not triage (joins `states`), project not
/// archived (needs the `projects` join — applied by the queries layer),
/// `archived_at IS NULL`, `is_draft = false`.
/// Rendered here on the `issues` (+ `states`/`projects`) aliases so the base
/// `WHERE` and the `sub_issues_count` subquery share one spelling.
/// (Single-table rendering of `db::space::columns::issue_objects_scope`,
/// plus the projects conjunct that scope leaves to the queries layer.)
pub const ISSUE_OBJECTS_WHERE: &str = "\"issues\".\"deleted_at\" IS NULL AND \"states\".\"group\" != 'triage' AND \"issues\".\"archived_at\" IS NULL AND \"projects\".\"archived_at\" IS NULL AND \"issues\".\"is_draft\" = false";

/// Base `WHERE` before user filters: manager exclusions plus tenant scoping
/// (`workspace__slug=slug, project_id=project_id`, `views/issue.py:88`).
/// `$1` = workspace id (resolved from the board row's `workspace.slug`),
/// `$2` = project id (the board row's `entity_identifier`).
pub fn base_where_sql() -> String {
    format!(
        "({ISSUE_OBJECTS_WHERE} AND \"issues\".\"workspace_id\" = $1 AND \"issues\".\"project_id\" = $2)"
    )
}

/// `cycle_id` annotation subquery (`views/issue.py:98-102`):
/// `Subquery(CycleIssue.objects.filter(issue=OuterRef("id"),
/// deleted_at__isnull=True).values("cycle_id")[:1])`.
pub fn cycle_id_annotation_sql() -> String {
    "(SELECT U0.\"cycle_id\" FROM \"cycle_issues\" U0 WHERE (U0.\"deleted_at\" IS NULL AND U0.\"issue_id\" = (\"issues\".\"id\")) LIMIT 1) AS \"cycle_id\"".to_string()
}

/// `link_count` annotation subquery (`views/issue.py:103-108`).
pub fn link_count_annotation_sql() -> String {
    "(SELECT COUNT(U0.\"id\") AS \"count\" FROM \"issue_links\" U0 WHERE U0.\"issue_id\" = (\"issues\".\"id\")) AS \"link_count\"".to_string()
}

/// `attachment_count` annotation subquery (`views/issue.py:109-117`):
/// `FileAsset` rows for this issue with
/// `entity_type = ISSUE_ATTACHMENT` (`db/models/asset.py:33-43`).
pub fn attachment_count_annotation_sql() -> String {
    "(SELECT COUNT(U0.\"id\") AS \"count\" FROM \"file_assets\" U0 WHERE (U0.\"issue_id\" = (\"issues\".\"id\") AND U0.\"entity_type\" = 'ISSUE_ATTACHMENT')) AS \"attachment_count\"".to_string()
}

/// `sub_issues_count` annotation subquery (`views/issue.py:118-123`):
/// `Issue.issue_objects.filter(parent=OuterRef("id"))` counted — the
/// `IssueManager` exclusions are re-applied, so triage/archived/draft
/// children are NOT counted.
pub fn sub_issues_count_annotation_sql() -> String {
    "(SELECT COUNT(U0.\"id\") AS \"count\" FROM \"issues\" U0 INNER JOIN \"states\" U1 ON (U0.\"state_id\" = U1.\"id\") INNER JOIN \"projects\" U2 ON (U0.\"project_id\" = U2.\"id\") WHERE (U0.\"deleted_at\" IS NULL AND U1.\"group\" != 'triage' AND U0.\"archived_at\" IS NULL AND U2.\"archived_at\" IS NULL AND U0.\"is_draft\" = false AND U0.\"parent_id\" = (\"issues\".\"id\"))) AS \"sub_issues_count\"".to_string()
}

// ---------------------------------------------------------------------------
// Ordering (order_queryset.py:14-58, default "-created_at" at views/issue.py:78)
// ---------------------------------------------------------------------------

/// Priority display order (`order_queryset.py:10`).
pub const PRIORITY_ORDER: &[&str] = &["urgent", "high", "medium", "low", "none"];

/// State-group display order: `STATE_GROUP_ORDER`
/// (`pi_dash/utils/constants.py:76-84`).
pub const STATE_GROUP_ORDER: &[&str] = &[
    "backlog",
    "unstarted",
    "started",
    "review",
    "test",
    "completed",
    "cancelled",
];

/// `ORDER BY` for `order_issue_queryset(issue_queryset, order_by_param)`
/// (`order_queryset.py:14-58`). Returns the Django `ORDER BY` clause text.
/// Anything containing `created_at` orders by that alone (`:53-54`);
/// every other param appends the `-created_at` tiebreak (`:56`).
/// The paginator re-applies the same key with nulls-last handling
/// (`paginator.py:136-139,253-267,454-472`); [`order_key`] maps the public
/// param the paginator carries.
pub fn order_by_sql(order_by_param: &str) -> String {
    match order_by_param {
        "priority" | "-priority" => {
            let cases: Vec<String> = PRIORITY_ORDER
                .iter()
                .enumerate()
                .map(|(i, p)| format!("WHEN \"issues\".\"priority\" = '{p}' THEN {i}"))
                .collect();
            format!(
                "ORDER BY CASE {} ELSE {} END, \"issues\".\"created_at\" DESC",
                cases.join(" "),
                PRIORITY_ORDER.len()
            )
        }
        "state__group" | "-state__group" => {
            // `:26`: ascending uses STATE_ORDER as-is only for the (dead)
            // "state__name" spelling; both live spellings reverse it.
            let rev: Vec<&&str> = STATE_GROUP_ORDER.iter().rev().collect();
            let cases: Vec<String> = rev
                .iter()
                .enumerate()
                .map(|(i, g)| format!("WHEN \"states\".\"group\" = '{g}' THEN {i}"))
                .collect();
            format!(
                "ORDER BY CASE {} ELSE {} END, \"issues\".\"created_at\" DESC",
                cases.join(" "),
                rev.len()
            )
        }
        "labels__name"
        | "assignees__first_name"
        | "issue_module__module__name"
        | "-labels__name"
        | "-assignees__first_name"
        | "-issue_module__module__name" => {
            let field = order_by_param.trim_start_matches('-');
            let dir = if order_by_param.starts_with('-') {
                "DESC"
            } else {
                "ASC"
            };
            format!("ORDER BY MIN(\"{field}\") {dir}, \"issues\".\"created_at\" DESC")
        }
        other if other.contains("created_at") => {
            let dir = if other.starts_with('-') {
                "DESC"
            } else {
                "ASC"
            };
            let col = other.trim_start_matches('-');
            format!("ORDER BY \"issues\".\"{col}\" {dir}")
        }
        other => {
            let (col, dir) = match other.strip_prefix('-') {
                Some(c) => (c, "DESC"),
                None => (other, "ASC"),
            };
            format!("ORDER BY \"issues\".\"{col}\" {dir}, \"issues\".\"created_at\" DESC")
        }
    }
}

/// The public order param the paginator carries after
/// `order_issue_queryset` returns (`order_queryset.py:23,34,48,57`):
/// `priority*` folds to the `priority_order` annotation, `state__group*` to
/// `state_order`, m2m-name orderings to `min_values`, everything else stays.
pub fn order_key(order_by_param: &str) -> &str {
    match order_by_param {
        "priority" => "-priority_order",
        "-priority" => "priority_order",
        "state__group" => "state_order",
        "-state__group" => "-state_order",
        "labels__name" | "assignees__first_name" | "issue_module__module__name" => "min_values",
        "-labels__name" | "-assignees__first_name" | "-issue_module__module__name" => "-min_values",
        other => other,
    }
}

// ---------------------------------------------------------------------------
// issue_filters(query_params, "GET") (pi_dash/utils/issue_filters.py:431-466)
// ---------------------------------------------------------------------------

/// GET-compiled filter conjuncts for one query param: `(sql, params)`.
/// The SQL uses `$N` placeholders allocated from `next_param`; the returned
/// `usize` is the next free index. Only params present in `query_params`
/// contribute (`issue_filters.py:462-465`); every helper below mirrors its
/// `filter_*` function's `"GET"` branch exactly, including the `"None"` /
/// `"null"` token rules and the unconditional `deleted_at` guards.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterCompile {
    /// SQL conjuncts joined with `AND` by the caller.
    pub conjuncts: Vec<String>,
    /// Bound values in `$N` first-appearance order.
    pub params: Vec<String>,
}

/// Keep tokens that are neither `"null"` nor empty
/// (`issue_filters.py:88,100,110,...`: `split(",")` + `!= "null"` + `"" not in`).
fn get_tokens(raw: &str) -> Option<Vec<String>> {
    let tokens: Vec<String> = raw
        .split(',')
        .filter(|t| *t != "null")
        .map(str::to_string)
        .collect();
    if tokens.is_empty() || tokens.iter().any(|t| t.is_empty()) {
        return None;
    }
    Some(tokens)
}

/// UUID-valid tokens only; invalid ones are silently dropped
/// (`filter_valid_uuids`, `issue_filters.py:18-27`).
fn valid_uuids(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .filter(|t| t.parse::<uuid::Uuid>().is_ok())
        .cloned()
        .collect()
}

/// `IN` conjunct with one `$N` per value, e.g.
/// `"issues"."state_id" IN ($3, $4)`.
fn in_conjunct(column: &str, values: &[String], next_param: usize) -> (String, usize) {
    let holders: Vec<String> = (0..values.len())
        .map(|i| format!("${}", next_param + i))
        .collect();
    (
        format!("{column} IN ({})", holders.join(", ")),
        next_param + values.len(),
    )
}

/// UUID-list filter used by the state/parent/project/mentions/created_by/
/// logged_by GET branches: drop `"None"`-adjacent handling per branch, keep
/// valid UUIDs, emit `column IN (...)`. Returns `None` when nothing valid
/// remains (no conjunct — `issue_filters.py:90-91` etc.).
fn uuid_in(column: &str, raw: &str, next_param: usize) -> Option<(String, Vec<String>, usize)> {
    let tokens = get_tokens(raw)?;
    let ids = valid_uuids(&tokens);
    if ids.is_empty() {
        return None;
    }
    let (conjunct, next) = in_conjunct(column, &ids, next_param);
    Some((conjunct, ids, next))
}

/// Compile the GET branch of every `ISSUE_FILTER` entry
/// (`issue_filters.py:434-460`) for the params present in `query_params`.
/// `query_params` is the ordered list of `(key, raw_value)` pairs;
/// `next_param` is the first free `$N` (the base `WHERE` owns `$1..$2`).
/// Plain-string branches (`name`, date terms) are inlined; UUID/date details
/// live in the helpers above and below.
pub fn compile_issue_filters(
    query_params: &[(String, String)],
    next_param: usize,
) -> FilterCompile {
    let mut conjuncts = Vec::new();
    let mut params: Vec<String> = Vec::new();
    let mut next = next_param;
    for (key, raw) in query_params {
        match key.as_str() {
            "state" => {
                if let Some((c, ids, n)) = uuid_in("\"issues\".\"state_id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
            }
            "state_group" => {
                if let Some(tokens) = get_tokens(raw) {
                    let (c, n) = in_conjunct("\"states\".\"group\"", &tokens, next);
                    conjuncts.push(c);
                    params.extend(tokens);
                    next = n;
                }
            }
            "estimate_point" => {
                if let Some(tokens) = get_tokens(raw) {
                    let (c, n) = in_conjunct("\"issues\".\"estimate_point\"", &tokens, next);
                    conjuncts.push(c);
                    params.extend(tokens);
                    next = n;
                }
            }
            "priority" => {
                if let Some(tokens) = get_tokens(raw) {
                    let (c, n) = in_conjunct("\"issues\".\"priority\"", &tokens, next);
                    conjuncts.push(c);
                    params.extend(tokens);
                    next = n;
                }
            }
            "parent" => {
                let has_none = raw.split(',').any(|t| t == "None");
                if has_none {
                    conjuncts.push("\"issues\".\"parent_id\" IS NULL".to_string());
                }
                if let Some((c, ids, n)) = uuid_in("\"issues\".\"parent_id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
            }
            "labels" => {
                let has_none = raw.split(',').any(|t| t == "None");
                if has_none {
                    conjuncts.push("\"labels\".\"id\" IS NULL".to_string());
                }
                if let Some((c, ids, n)) = uuid_in("\"labels\".\"id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
                conjuncts.push("\"label_issue\".\"deleted_at\" IS NULL".to_string());
            }
            "assignees" => {
                let has_none = raw.split(',').any(|t| t == "None");
                if has_none {
                    conjuncts.push("\"users\".\"id\" IS NULL".to_string());
                }
                if let Some((c, ids, n)) = uuid_in("\"users\".\"id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
                conjuncts.push("\"issue_assignee\".\"deleted_at\" IS NULL".to_string());
            }
            "mentions" => {
                if let Some((c, ids, n)) = uuid_in("\"issue_mention\".\"mention_id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
            }
            "created_by" => {
                let has_none = raw.split(',').any(|t| t == "None");
                if has_none {
                    conjuncts.push("\"issues\".\"created_by_id\" IS NULL".to_string());
                }
                if let Some((c, ids, n)) = uuid_in("\"issues\".\"created_by_id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
            }
            "logged_by" => {
                let has_none = raw.split(',').any(|t| t == "None");
                if has_none {
                    conjuncts.push("\"issues\".\"logged_by_id\" IS NULL".to_string());
                }
                if let Some((c, ids, n)) = uuid_in("\"issues\".\"logged_by_id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
            }
            "name" if !raw.is_empty() => {
                conjuncts.push(format!("\"issues\".\"name\" ILIKE ${next}"));
                params.push(format!("%{raw}%"));
                next += 1;
            }
            "created_at" => {
                for (c, p) in date_conjuncts("\"issues\".\"created_at\"", raw) {
                    conjuncts.push(c);
                    params.extend(p);
                }
            }
            // QUIRK-filtered-out-updated-at: writes the created_at term.
            "updated_at" => {
                for (c, p) in date_conjuncts("\"issues\".\"created_at\"", raw) {
                    conjuncts.push(c);
                    params.extend(p);
                }
            }
            "start_date" => {
                for (c, p) in date_conjuncts("\"issues\".\"start_date\"", raw) {
                    conjuncts.push(c);
                    params.extend(p);
                }
            }
            "target_date" => {
                for (c, p) in date_conjuncts("\"issues\".\"target_date\"", raw) {
                    conjuncts.push(c);
                    params.extend(p);
                }
            }
            "completed_at" => {
                for (c, p) in date_conjuncts("\"issues\".\"completed_at\"", raw) {
                    conjuncts.push(c);
                    params.extend(p);
                }
            }
            "type" => {
                // `filter_issue_state_type` (`issue_filters.py:298-308`):
                // always emits state__group__in; "backlog" narrows to
                // ["backlog"], "active" to ACTIVE_STATE_GROUPS, anything
                // else (incl. missing/"all") to full STATE_GROUP_ORDER.
                let groups: Vec<String> = match raw.as_str() {
                    "backlog" => vec!["backlog".to_string()],
                    "active" => vec!["unstarted", "started", "review", "test"]
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    _ => STATE_GROUP_ORDER.iter().map(|s| s.to_string()).collect(),
                };
                let (c, n) = in_conjunct("\"states\".\"group\"", &groups, next);
                conjuncts.push(c);
                params.extend(groups);
                next = n;
            }
            "project" => {
                if let Some((c, ids, n)) = uuid_in("\"issues\".\"project_id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
            }
            "cycle" => {
                let has_none = raw.split(',').any(|t| t == "None");
                if has_none {
                    conjuncts.push("\"issue_cycle\".\"cycle_id\" IS NULL".to_string());
                }
                if let Some((c, ids, n)) = uuid_in("\"issue_cycle\".\"cycle_id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
                conjuncts.push("\"issue_cycle\".\"deleted_at\" IS NULL".to_string());
            }
            "module" => {
                let has_none = raw.split(',').any(|t| t == "None");
                if has_none {
                    conjuncts.push("\"issue_module\".\"module_id\" IS NULL".to_string());
                }
                if let Some((c, ids, n)) = uuid_in("\"issue_module\".\"module_id\"", raw, next) {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
                conjuncts.push("\"issue_module\".\"deleted_at\" IS NULL".to_string());
            }
            "intake_status" | "inbox_status" => {
                if let Some(tokens) = get_tokens(raw) {
                    let (c, n) = in_conjunct("\"issue_intake\".\"status\"", &tokens, next);
                    conjuncts.push(c);
                    params.extend(tokens);
                    next = n;
                }
            }
            "sub_issue" => {
                // `filter_sub_issue_toggle` (`issue_filters.py:383-392`):
                // default "false" excludes childless-parent... precisely:
                // "false" keeps only top-level issues.
                let v = if raw.is_empty() {
                    "false"
                } else {
                    raw.as_str()
                };
                if v == "false" {
                    conjuncts.push("\"issues\".\"parent_id\" IS NULL".to_string());
                }
            }
            "subscriber" => {
                if let Some((c, ids, n)) =
                    uuid_in("\"issue_subscribers\".\"subscriber_id\"", raw, next)
                {
                    conjuncts.push(c);
                    params.extend(ids);
                    next = n;
                }
                conjuncts.push("\"issue_subscribers\".\"deleted_at\" IS NULL".to_string());
            }
            "start_target_date" if raw == "true" => {
                conjuncts.push("\"issues\".\"target_date\" IS NOT NULL".to_string());
                conjuncts.push("\"issues\".\"start_date\" IS NOT NULL".to_string());
            }
            _ => {}
        }
    }
    FilterCompile { conjuncts, params }
}

/// Date-term conjuncts for one comma-separated GET value
/// (`date_filter`, `issue_filters.py:57-83`): `after` selects `>=`, anything
/// else `<=` on two-part queries; a bare single part is `__contains`
/// (ILIKE here); `N_weeks`/`N_months` relative terms are handler-resolved
/// against "today" and intentionally contribute no static conjunct.
fn date_conjuncts(column: &str, raw: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for query in raw.split(',') {
        if query.is_empty() {
            continue;
        }
        let parts: Vec<&str> = query.split(';').collect();
        if parts.len() >= 2 {
            let head = parts[0];
            let is_relative = head.split('_').count() == 2
                && head
                    .split('_')
                    .nth(1)
                    .is_some_and(|t| t == "weeks" || t == "months")
                && head
                    .split('_')
                    .next()
                    .is_some_and(|d| d.parse::<u64>().is_ok());
            if is_relative {
                continue;
            }
            if parts.contains(&"after") {
                out.push((format!("{column} >= '{head}'"), vec![]));
            } else {
                out.push((format!("{column} <= '{head}'"), vec![]));
            }
        } else {
            out.push((format!("{column}::text ILIKE '%{}%'", parts[0]), vec![]));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// issue_queryset_grouper (space/utils/grouper.py:28-70)
// ---------------------------------------------------------------------------

/// Django lookup path per group key (`FIELD_MAPPER`, `grouper.py:31-35`).
pub fn group_lookup(group_key: &str) -> Option<&'static str> {
    match group_key {
        "label_ids" => Some("labels__id"),
        "assignee_ids" => Some("assignees__id"),
        "module_ids" => Some("issue_module__module_id"),
        _ => None,
    }
}

/// Extra `filter()` applied before annotating when a group key needs it
/// (`GROUP_FILTER_MAPPER`, `grouper.py:37-45`): label/assignee groups keep
/// only live m2m rows; module groups keep only live module links.
pub fn group_prefilter_sql(group_key: &str) -> Option<&'static str> {
    match group_key {
        "assignees__id" => Some("\"issue_assignee\".\"deleted_at\" IS NULL"),
        "labels__id" => Some("\"label_issue\".\"deleted_at\" IS NULL"),
        "issue_module__module_id" => Some("\"issue_module\".\"deleted_at\" IS NULL"),
        _ => None,
    }
}

/// Id-list annotation keys Django adds for the non-grouped m2m axes
/// (`grouper.py:47-68`). BUG-always-annotate: the guard is
/// `FIELD_MAPPER.get(key) != group_by OR FIELD_MAPPER.get(key) !=
/// sub_group_by`, which is true for every key on every reachable path — so
/// all three annotations are always present (the only exception is the
/// rejected `group_by == sub_group_by == <mapped path>` corner, where that
/// one key drops out — and the 400 fires after the grouper already ran).
/// Ported literally: the `||` below is the bug, do not "fix" it into `&&`.
pub fn default_annotation_keys(group_by: &str, sub_group_by: &str) -> Vec<&'static str> {
    ["assignee_ids", "label_ids", "module_ids"]
        .into_iter()
        .filter(|key| {
            group_lookup(key).unwrap_or("") != group_by
                || group_lookup(key).unwrap_or("") != sub_group_by
        })
        .collect()
}

/// `COALESCE(ARRAY_AGG(...), [])` annotation SQL per id-list key
/// (`grouper.py:61-68`): distinct ids with the live-link-only filter,
/// defaulting to `[]` via `COALESCE`.
pub fn default_annotation_sql(key: &str) -> Option<String> {
    let (target, source, predicate) = match key {
        "assignee_ids" => (
            "assignee_ids",
            "\"users\".\"id\"",
            "(\"users\".\"id\" IS NOT NULL AND \"issue_assignee\".\"deleted_at\" IS NULL)",
        ),
        "label_ids" => (
            "label_ids",
            "\"labels\".\"id\"",
            "(\"labels\".\"id\" IS NOT NULL AND \"label_issue\".\"deleted_at\" IS NULL)",
        ),
        "module_ids" => (
            "module_ids",
            "\"issue_module\".\"module_id\"",
            "(\"issue_module\".\"module_id\" IS NOT NULL)",
        ),
        _ => return None,
    };
    Some(format!(
        "COALESCE(ARRAY_AGG(DISTINCT {source}) FILTER (WHERE {predicate}), '{{}}') AS \"{target}\""
    ))
}

// ---------------------------------------------------------------------------
// Grouped pagination (views/issue.py:140-204, paginator.py:194-632)
// ---------------------------------------------------------------------------

/// `group_by == sub_group_by` body: 400
/// (`views/issue.py:142-146`).
pub const GROUP_MISMATCH_BODY: &str =
    r#"{"error": "Group by and sub group by cannot have same parameters"}"#;

/// Intake/archived/draft totals predicate shared by the grouped and
/// sub-grouped paginators (`count_filter=Q(...)`, `views/issue.py:170-177`
/// and `:196-203`; QUIRK-count-filter-intake documented at the top of this
/// module).
pub fn count_filter_sql() -> String {
    "((\"issue_intake\".\"status\" = 1 OR \"issue_intake\".\"status\" = -1 OR \"issue_intake\".\"status\" = 2 OR \"issue_intake\".\"id\" IS NULL) AND \"issues\".\"archived_at\" IS NULL AND \"issues\".\"is_draft\" = false)".to_string()
}

/// Per-group totals query behind `__get_total_queryset`
/// (`paginator.py:297-303`): one row per group value with the distinct-issue
/// count under [`count_filter_sql`].
pub fn group_totals_sql(group_field: &str) -> String {
    format!(
        "SELECT {group_field}, COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE {}) AS \"count\" FROM ({{base}}) GROUP BY {group_field}",
        count_filter_sql()
    )
}

/// Per-group/sub-group totals query behind `__get_subgroup_total_queryset`
/// (`paginator.py:511-518`).
pub fn subgroup_totals_sql(group_field: &str, sub_group_field: &str) -> String {
    format!(
        "SELECT {group_field}, {sub_group_field}, COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE {}) AS \"count\" FROM ({{base}}) GROUP BY {group_field}, {sub_group_field}",
        count_filter_sql()
    )
}

/// Grouped window: `ROW_NUMBER() OVER (PARTITION BY <group> ORDER BY <key>,
/// created_at DESC)` then `row_number > offset AND row_number < stop`
/// (`GroupedOffsetPaginator.get_result`, `paginator.py:223-295`).
/// `offset = cursor.offset * cursor.value`, `stop = offset + (cursor.value or
/// limit) + 1` (`:234-236`); default limit 50 (`:223`).
pub fn grouped_window_sql(group_field: &str, order_param: &str) -> String {
    let key = order_key(order_param);
    let (col, desc) = match key.strip_prefix('-') {
        Some(c) => (c, true),
        None => (key, false),
    };
    let dir = if desc {
        "DESC NULLS LAST"
    } else {
        "ASC NULLS LAST"
    };
    format!(
        "SELECT *, ROW_NUMBER() OVER (PARTITION BY {group_field} ORDER BY \"{col}\" {dir}, \"created_at\" DESC) AS \"row_number\" FROM ({{base}}) WHERE \"row_number\" > {{offset}} AND \"row_number\" < {{stop}} ORDER BY \"{col}\" {dir}, \"created_at\" DESC"
    )
}

/// Sub-grouped window: same shape partitioned by `(group, sub_group)`
/// (`SubGroupedOffsetPaginator.get_result`, `paginator.py:424-500`).
/// Default limit 30 (`:424`).
pub fn subgrouped_window_sql(
    group_field: &str,
    sub_group_field: &str,
    order_param: &str,
) -> String {
    let key = order_key(order_param);
    let (col, desc) = match key.strip_prefix('-') {
        Some(c) => (c, true),
        None => (key, false),
    };
    let dir = if desc {
        "DESC NULLS LAST"
    } else {
        "ASC NULLS LAST"
    };
    format!(
        "SELECT *, ROW_NUMBER() OVER (PARTITION BY {group_field}, {sub_group_field} ORDER BY \"{col}\" {dir}, \"created_at\" DESC) AS \"row_number\" FROM ({{base}}) WHERE \"row_number\" > {{offset}} AND \"row_number\" < {{stop}} ORDER BY \"{col}\" {dir}, \"created_at\" DESC"
    )
}

/// m2m group axes whose `process_results` fans one row out to several groups
/// (`FIELD_MAPPER`, `paginator.py:196-200,391-395`).
pub const GROUP_FIELD_MAPPER: &[(&str, &str)] = &[
    ("labels__id", "label_ids"),
    ("assignees__id", "assignee_ids"),
    ("issue_module__module_id", "module_ids"),
];

/// Ungrouped cursor defaults: absent cursor is `{per_page}:0:0`
/// (`paginator.py:678`); ungrouped default limit 1000 (`:660`).
pub const UNGROUPED_DEFAULT_LIMIT: i64 = 1000;
/// Grouped default limit (`paginator.py:223`).
pub const GROUPED_DEFAULT_LIMIT: i64 = 50;
/// Sub-grouped default limit (`paginator.py:424`).
pub const SUBGROUPED_DEFAULT_LIMIT: i64 = 30;

// ---------------------------------------------------------------------------
// issue_group_values (space/utils/grouper.py:185-252)
// ---------------------------------------------------------------------------

/// Static group-value branches (`grouper.py:228-231`).
pub fn static_group_values(field: &str) -> Option<&'static [&'static str]> {
    match field {
        "priority" => Some(&["low", "medium", "high", "urgent", "none"]),
        "state__group" => Some(STATE_GROUP_ORDER),
        _ => None,
    }
}

/// How the DB-backed `issue_group_values` branches resolve
/// (`grouper.py:192-250`): the `values_list` column, whether the project
/// scope applies, and whether `"None"` is appended as a sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupValuesQuery {
    /// Table the ids are read from.
    pub table: &'static str,
    /// Column read via `values_list(..., flat=True)`.
    pub column: &'static str,
    /// `queryset.filter(project_id=project_id)` applies when set.
    pub project_filter: &'static str,
    /// Extra `WHERE` conjuncts (manager scopes, active flags).
    pub extra_where: &'static str,
    /// `"None"` sentinel appended after the ids.
    pub none_sentinel: bool,
}

/// DB-backed branch descriptor, or `None` for the static branches above
/// and unknown fields (which return `[]`, `grouper.py:252`).
pub fn group_values_query(field: &str) -> Option<GroupValuesQuery> {
    match field {
        "state_id" => Some(GroupValuesQuery {
            table: "\"states\"",
            column: "\"id\"",
            project_filter: "\"project_id\"",
            extra_where: "\"is_triage\" = false",
            none_sentinel: false,
        }),
        "labels__id" => Some(GroupValuesQuery {
            table: "\"labels\"",
            column: "\"id\"",
            project_filter: "\"project_id\"",
            extra_where: "\"deleted_at\" IS NULL",
            none_sentinel: true,
        }),
        "assignees__id" => Some(GroupValuesQuery {
            table: "\"project_members\"",
            column: "\"member_id\"",
            project_filter: "\"project_id\"",
            extra_where: "\"is_active\" = true",
            none_sentinel: false,
        }),
        "issue_module__module_id" => Some(GroupValuesQuery {
            table: "\"modules\"",
            column: "\"id\"",
            project_filter: "\"project_id\"",
            extra_where: "\"deleted_at\" IS NULL",
            none_sentinel: true,
        }),
        "cycle_id" => Some(GroupValuesQuery {
            table: "\"cycles\"",
            column: "\"id\"",
            project_filter: "\"project_id\"",
            extra_where: "\"deleted_at\" IS NULL",
            none_sentinel: true,
        }),
        "project_id" => Some(GroupValuesQuery {
            table: "\"projects\"",
            column: "\"id\"",
            project_filter: "",
            extra_where: "\"deleted_at\" IS NULL",
            none_sentinel: false,
        }),
        // The date/author branches read distinct values off the issue
        // queryset itself (`grouper.py:232-250`) — BUG-group-values-no-
        // queryset: the caller here always passes the default
        // `queryset=None`, so these raise `AttributeError`. The intended
        // SQL is still recorded.
        "target_date" => Some(GroupValuesQuery {
            table: "\"issues\"",
            column: "\"target_date\"",
            project_filter: "\"project_id\"",
            extra_where: "",
            none_sentinel: false,
        }),
        "start_date" => Some(GroupValuesQuery {
            table: "\"issues\"",
            column: "\"start_date\"",
            project_filter: "\"project_id\"",
            extra_where: "",
            none_sentinel: false,
        }),
        "created_by" => Some(GroupValuesQuery {
            table: "\"issues\"",
            column: "\"created_by_id\"",
            project_filter: "\"project_id\"",
            extra_where: "",
            none_sentinel: false,
        }),
        _ => None,
    }
}

/// Whether the branch result is a real `list` when `project_id` is set.
/// BUG-assignees-unwrapped (`grouper.py:204-208`): the `assignees__id` +
/// project branch returns the bare `values_list` QuerySet — every other
/// branch wraps with `list()`.
pub fn group_values_needs_list_wrap(field: &str, project_id_present: bool) -> bool {
    !(field == "assignees__id" && project_id_present)
}

/// Whether the branch needs the caller's queryset (and therefore raises
/// `AttributeError` on this endpoint's default `queryset=None`).
/// BUG-group-values-no-queryset (`grouper.py:233-250`).
pub fn group_values_requires_queryset(field: &str) -> bool {
    matches!(field, "target_date" | "start_date" | "created_by")
}

// ---------------------------------------------------------------------------
// issue_on_results (space/utils/grouper.py:73-182)
// ---------------------------------------------------------------------------

/// Always-selected columns (`required_fields`, `grouper.py:84-99`).
pub const REQUIRED_FIELDS: &[&str] = &[
    "id",
    "name",
    "state_id",
    "sort_order",
    "estimate_point",
    "priority",
    "start_date",
    "target_date",
    "sequence_id",
    "project_id",
    "parent_id",
    "cycle_id",
    "created_by",
    "state__group",
];

/// Ungrouped m2m axes (`original_list`, `grouper.py:82`).
pub const ORIGINAL_LIST: &[&str] = &["assignee_ids", "label_ids", "module_ids"];

/// Reverse lookup: grouped Django path → replaced original key
/// (`FIELD_MAPPER`, `grouper.py:76-80`).
pub fn on_results_group_key(group_by: &str) -> Option<&'static str> {
    match group_by {
        "labels__id" => Some("label_ids"),
        "assignees__id" => Some("assignee_ids"),
        "issue_module__module_id" => Some("module_ids"),
        _ => None,
    }
}

/// Full `.values()` key list for `issue_on_results(issues, group_by,
/// sub_group_by)` (`grouper.py:101-109`): the grouped paths swap their
/// `original_list` entry for the Django lookup path, then `vote_items` and
/// `reaction_items` are appended (`:180`).
pub fn on_results_fields(group_by: &str, sub_group_by: &str) -> Vec<String> {
    let mut fields: Vec<String> = REQUIRED_FIELDS.iter().map(|s| s.to_string()).collect();
    let mut rest: Vec<String> = ORIGINAL_LIST.iter().map(|s| s.to_string()).collect();
    for key in [group_by, sub_group_by] {
        if let Some(original) = on_results_group_key(key) {
            if let Some(pos) = rest.iter().position(|f| f == original) {
                rest.remove(pos);
            }
            if !rest.contains(&key.to_string()) {
                rest.push(key.to_string());
            }
        }
    }
    fields.extend(rest);
    fields.push("vote_items".to_string());
    fields.push("reaction_items".to_string());
    fields
}

/// `vote_items` annotation: `ArrayAgg(Case(When(votes present and live,
/// then JSONObject(vote, actor_details)), default=None))` with the
/// `avatar_url` fallback (`grouper.py:112-145`).
pub fn vote_items_annotation_sql() -> String {
    "ARRAY_AGG(DISTINCT CASE WHEN (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL) THEN JSONB_BUILD_OBJECT('vote', \"votes\".\"vote\", 'actor_details', JSONB_BUILD_OBJECT('id', \"vote_actor\".\"id\", 'first_name', \"vote_actor\".\"first_name\", 'last_name', \"vote_actor\".\"last_name\", 'avatar', \"vote_actor\".\"avatar\", 'avatar_url', CASE WHEN \"vote_actor\".\"avatar_asset\" IS NOT NULL THEN CONCAT('/api/assets/v2/static/', \"vote_actor\".\"avatar_asset\", '/') ELSE \"vote_actor\".\"avatar\" END, 'display_name', \"vote_actor\".\"display_name\")) ELSE NULL END) FILTER (WHERE (\"votes\".\"id\" IS NOT NULL AND \"votes\".\"deleted_at\" IS NULL)) AS \"vote_items\"".to_string()
}

/// `reaction_items` annotation: same shape over `issue_reactions`
/// (`grouper.py:146-179`).
pub fn reaction_items_annotation_sql() -> String {
    "ARRAY_AGG(DISTINCT CASE WHEN (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL) THEN JSONB_BUILD_OBJECT('reaction', \"issue_reactions\".\"reaction\", 'actor_details', JSONB_BUILD_OBJECT('id', \"reaction_actor\".\"id\", 'first_name', \"reaction_actor\".\"first_name\", 'last_name', \"reaction_actor\".\"last_name\", 'avatar', \"reaction_actor\".\"avatar\", 'avatar_url', CASE WHEN \"reaction_actor\".\"avatar_asset\" IS NOT NULL THEN CONCAT('/api/assets/v2/static/', \"reaction_actor\".\"avatar_asset\", '/') ELSE \"reaction_actor\".\"avatar\" END, 'display_name', \"reaction_actor\".\"display_name\")) ELSE NULL END) FILTER (WHERE (\"issue_reactions\".\"id\" IS NOT NULL AND \"issue_reactions\".\"deleted_at\" IS NULL)) AS \"reaction_items\"".to_string()
}

// ---------------------------------------------------------------------------
// Rows (issue_on_results projection, grouper.py:73-182)
// ---------------------------------------------------------------------------

/// Actor detail nested inside vote/reaction items, wire key order
/// `id, first_name, last_name, avatar, avatar_url, display_name`
/// (`grouper.py:119-137,153-171`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActorDetails {
    /// `votes__actor__id` / `issue_reactions__actor__id`.
    pub id: String,
    /// Actor first name.
    pub first_name: String,
    /// Actor last name.
    pub last_name: String,
    /// Raw avatar value.
    pub avatar: String,
    /// `avatar_url`: presigned `/api/assets/v2/static/<asset>/` when the
    /// actor has an avatar asset, else the raw avatar
    /// (`grouper.py:124-135,158-169`).
    pub avatar_url: String,
    /// Display name.
    pub display_name: String,
}

/// One `vote_items` element, wire key order `vote, actor_details`
/// (`grouper.py:117-138`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoteItem {
    /// `votes__vote`.
    pub vote: i32,
    /// Voter detail.
    pub actor_details: ActorDetails,
}

/// One `reaction_items` element, wire key order `reaction, actor_details`
/// (`grouper.py:151-172`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReactionItem {
    /// `issue_reactions__reaction`.
    pub reaction: String,
    /// Reactor detail.
    pub actor_details: ActorDetails,
}

/// One `issue_on_results` row on the ungrouped path: the 14
/// [`REQUIRED_FIELDS`] in source order, then [`ORIGINAL_LIST`], then the
/// two item arrays. Serde field order matches the `.values()` order so
/// `serde_json::to_string` renders the wire order; the fixture rows carry
/// the same value set (sorted keys) for the `to_value` check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueListRow {
    /// `id`.
    pub id: String,
    /// `name`.
    pub name: String,
    /// `state_id`.
    pub state_id: String,
    /// `sort_order` (DRF renders floats, e.g. `65535.0`).
    pub sort_order: f64,
    /// `estimate_point` (nullable).
    pub estimate_point: Option<i32>,
    /// `priority`.
    pub priority: String,
    /// `start_date` (nullable ISO date).
    pub start_date: Option<String>,
    /// `target_date` (nullable ISO date).
    pub target_date: Option<String>,
    /// `sequence_id`.
    pub sequence_id: i64,
    /// `project_id`.
    pub project_id: String,
    /// `parent_id` (nullable).
    pub parent_id: Option<String>,
    /// `cycle_id` subquery annotation (nullable).
    pub cycle_id: Option<String>,
    /// `created_by` (nullable in the model; required in the projection).
    pub created_by: Option<String>,
    /// `state__group`.
    #[serde(rename = "state__group")]
    pub state_group: String,
    /// `assignee_ids` (always annotated — BUG-always-annotate).
    pub assignee_ids: Vec<String>,
    /// `label_ids` (always annotated — BUG-always-annotate).
    pub label_ids: Vec<String>,
    /// `module_ids` (always annotated — BUG-always-annotate).
    pub module_ids: Vec<String>,
    /// `vote_items` annotation.
    pub vote_items: Vec<VoteItem>,
    /// `reaction_items` annotation.
    pub reaction_items: Vec<ReactionItem>,
}

#[cfg(test)]
mod tests {
    use super::super::intake_assets::asset_board_first_scoped_sql;
    use super::*;

    const FIXTURE_SQL: &str = include_str!("../../../../../fixtures/space/queries/issue_list.sql");
    const FIXTURE_ROWS: &str =
        include_str!("../../../../../fixtures/space/queries/issue_list.rows.json");

    /// Collapse every whitespace run to one space (Django's compiler wraps
    /// lines; the text is what matters).
    fn squashed(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Render a builder's `$N` placeholders Django-style (`%(name)s`, as the
    /// fixture records them) for a containment check against the fixture.
    fn with_named_params(sql: &str, names: &[&str]) -> String {
        let mut out = sql.to_string();
        for (i, name) in names.iter().enumerate().rev() {
            out = out.replace(&format!("${}", i + 1), &format!("%({name})s"));
        }
        out
    }

    /// The builder's Django-shaped text must appear verbatim in the fixture
    /// (whitespace-insensitive). Use when the fixture records the full shape.
    fn assert_in_fixture(fixture: &str, sql: &str, params: &[&str]) {
        let hay = squashed(fixture);
        let needle = squashed(&with_named_params(sql, params));
        assert!(
            hay.contains(&needle),
            "builder SQL not found in fixture:\n{needle}"
        );
    }

    /// The fixture's concrete fragment must appear verbatim in the builder's
    /// output. Use when the fixture abbreviates the statement (ellipses,
    /// prose) while the builder emits fully-qualified executable SQL.
    fn assert_builder_contains(builder_sql: &str, params: &[&str], fragment: &str) {
        let hay = squashed(&with_named_params(builder_sql, params));
        let needle = squashed(fragment);
        assert!(
            hay.contains(&needle),
            "fixture fragment not found in builder SQL:\n{needle}\n----\n{hay}"
        );
    }

    fn qp(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn board_lookup_is_scoped_first_with_404_body() {
        // Same ORM call (filter + entity_name scoping + .first());
        // equality with the intake_assets builder is the contract.
        assert_eq!(issue_list_board_sql(), asset_board_first_scoped_sql());
        assert!(issue_list_board_sql().contains("\"entity_name\" = 'project'"));
        assert!(issue_list_board_sql()
            .ends_with("ORDER BY \"deploy_boards\".\"created_at\" DESC LIMIT 1"));
        assert_eq!(
            NOT_PUBLISHED_BODY,
            r#"{"error": "Project is not published"}"#
        );
        assert!(squashed(FIXTURE_SQL).contains("Project is not published"));
    }

    #[test]
    fn annotation_subqueries_match_fixture_l2() {
        assert_in_fixture(FIXTURE_SQL, &cycle_id_annotation_sql(), &[]);
        assert_in_fixture(FIXTURE_SQL, &link_count_annotation_sql(), &[]);
        assert_in_fixture(FIXTURE_SQL, &attachment_count_annotation_sql(), &[]);
        // The fixture abbreviates the manager exclusions inside the
        // sub-issues count (`AND NOT ...`), so pin the stable fragments
        // from the builder side.
        assert_builder_contains(
            &sub_issues_count_annotation_sql(),
            &[],
            "SELECT COUNT(U0.\"id\"",
        );
        assert_builder_contains(
            &sub_issues_count_annotation_sql(),
            &[],
            "U0.\"parent_id\" = (\"issues\".\"id\")",
        );
        for conjunct in [
            "U0.\"deleted_at\" IS NULL",
            "U1.\"group\" != 'triage'",
            "U0.\"archived_at\" IS NULL",
            "U2.\"archived_at\" IS NULL",
            "U0.\"is_draft\" = false",
        ] {
            assert_builder_contains(&sub_issues_count_annotation_sql(), &[], conjunct);
        }
    }

    #[test]
    fn base_where_carries_manager_scope_and_tenant() {
        let sql = base_where_sql();
        for conjunct in [
            "\"issues\".\"deleted_at\" IS NULL",
            "\"states\".\"group\" != 'triage'",
            "\"issues\".\"archived_at\" IS NULL",
            "\"projects\".\"archived_at\" IS NULL",
            "\"issues\".\"is_draft\" = false",
        ] {
            assert!(sql.contains(conjunct), "{conjunct}");
        }
        // Tenant params render Django-style to the fixture's tokens.
        assert_builder_contains(&sql, &["workspace_id", "project_id"], "%(workspace_id)s");
        assert_builder_contains(&sql, &["workspace_id", "project_id"], "%(project_id)s");
        assert!(squashed(FIXTURE_SQL).contains("%(workspace_id)s"));
        assert!(squashed(FIXTURE_SQL).contains("%(project_id)s"));
    }

    #[test]
    fn ordering_default_and_branches() {
        assert_eq!(
            order_by_sql("-created_at"),
            "ORDER BY \"issues\".\"created_at\" DESC"
        );
        assert_eq!(order_key("-created_at"), "-created_at");
        let prio = order_by_sql("-priority");
        assert!(prio.contains("WHEN \"issues\".\"priority\" = 'urgent' THEN 0"));
        assert!(prio.ends_with("\"issues\".\"created_at\" DESC"));
        assert_eq!(order_key("-priority"), "priority_order");
        assert_eq!(order_key("priority"), "-priority_order");
        let state = order_by_sql("-state__group");
        assert!(state.contains("WHEN \"states\".\"group\" = 'cancelled' THEN 0"));
        assert_eq!(order_key("state__group"), "state_order");
        assert_eq!(order_key("-state__group"), "-state_order");
        let m2m = order_by_sql("-assignees__first_name");
        assert!(m2m.contains("MIN(\"assignees__first_name\") DESC"));
        assert_eq!(order_key("-assignees__first_name"), "-min_values");
        assert_eq!(
            order_by_sql("sequence_id"),
            "ORDER BY \"issues\".\"sequence_id\" ASC, \"issues\".\"created_at\" DESC"
        );
        assert_eq!(order_key("sequence_id"), "sequence_id");
    }

    #[test]
    fn filters_uuid_branches_accept_and_drop() {
        let valid = "44444444-4444-4444-4444-444444444444";
        let c = compile_issue_filters(&qp(&[("state", valid)]), 3);
        assert_eq!(
            c.conjuncts,
            vec![format!("\"issues\".\"state_id\" IN ($3)")]
        );
        assert_eq!(c.params, vec![valid.to_string()]);
        // Invalid UUIDs are silently dropped with no conjunct.
        let bad = compile_issue_filters(&qp(&[("state", "not-a-uuid")]), 3);
        assert!(bad.conjuncts.is_empty());
        assert!(bad.params.is_empty());
        // "null" tokens contribute nothing; empty query contributes nothing.
        let null = compile_issue_filters(&qp(&[("state", "null")]), 3);
        assert!(null.conjuncts.is_empty());
        let empty = compile_issue_filters(&[], 3);
        assert!(empty.conjuncts.is_empty());
        // Unknown keys are ignored (only ISSUE_FILTER entries compile).
        let unknown = compile_issue_filters(&qp(&[("group_by", "priority")]), 3);
        assert!(unknown.conjuncts.is_empty());
    }

    #[test]
    fn filters_none_sentinels_and_unconditional_guards() {
        let c = compile_issue_filters(&qp(&[("labels", "None")]), 3);
        assert!(c
            .conjuncts
            .contains(&"\"labels\".\"id\" IS NULL".to_string()));
        assert!(c
            .conjuncts
            .contains(&"\"label_issue\".\"deleted_at\" IS NULL".to_string()));
        let c = compile_issue_filters(&qp(&[("assignees", "None")]), 3);
        assert!(c
            .conjuncts
            .contains(&"\"users\".\"id\" IS NULL".to_string()));
        assert!(c
            .conjuncts
            .contains(&"\"issue_assignee\".\"deleted_at\" IS NULL".to_string()));
        // The deleted_at guards apply even with no value filter at all.
        let c = compile_issue_filters(&qp(&[("cycle", "not-a-uuid"), ("module", "not-a-uuid")]), 3);
        assert!(c
            .conjuncts
            .contains(&"\"issue_cycle\".\"deleted_at\" IS NULL".to_string()));
        assert!(c
            .conjuncts
            .contains(&"\"issue_module\".\"deleted_at\" IS NULL".to_string()));
        let c = compile_issue_filters(&qp(&[("parent", "None")]), 3);
        assert!(c
            .conjuncts
            .contains(&"\"issues\".\"parent_id\" IS NULL".to_string()));
    }

    #[test]
    fn filters_updated_at_hits_created_at_and_type_always_emits() {
        // QUIRK-filtered-out-updated-at: updated_at compiles to created_at.
        let c = compile_issue_filters(&qp(&[("updated_at", "2026-01-01;after")]), 3);
        assert_eq!(
            c.conjuncts,
            vec!["\"issues\".\"created_at\" >= '2026-01-01'".to_string()]
        );
        let back = compile_issue_filters(&qp(&[("type", "backlog")]), 3);
        assert_eq!(back.params, vec!["backlog".to_string()]);
        let active = compile_issue_filters(&qp(&[("type", "active")]), 3);
        assert_eq!(
            active.params,
            vec!["unstarted", "started", "review", "test"]
        );
        let all = compile_issue_filters(&qp(&[("type", "all")]), 3);
        assert_eq!(all.params.len(), STATE_GROUP_ORDER.len());
        // sub_issue defaults to top-level-only; start_target_date needs both.
        let sub = compile_issue_filters(&qp(&[("sub_issue", "false")]), 3);
        assert_eq!(
            sub.conjuncts,
            vec!["\"issues\".\"parent_id\" IS NULL".to_string()]
        );
        let both = compile_issue_filters(&qp(&[("start_target_date", "true")]), 3);
        assert!(both
            .conjuncts
            .contains(&"\"issues\".\"target_date\" IS NOT NULL".to_string()));
        assert!(both
            .conjuncts
            .contains(&"\"issues\".\"start_date\" IS NOT NULL".to_string()));
    }

    #[test]
    fn grouper_prefilter_and_always_annotate_bug() {
        assert_eq!(
            group_prefilter_sql("labels__id"),
            Some("\"label_issue\".\"deleted_at\" IS NULL")
        );
        assert_eq!(
            group_prefilter_sql("assignees__id"),
            Some("\"issue_assignee\".\"deleted_at\" IS NULL")
        );
        assert_eq!(
            group_prefilter_sql("issue_module__module_id"),
            Some("\"issue_module\".\"deleted_at\" IS NULL")
        );
        assert_eq!(group_prefilter_sql("priority"), None);
        // BUG-always-annotate: the `or` guard is true for every key on every
        // reachable path, so all three axes are always annotated — even the
        // axis being grouped by. The single exception proves the `or`: when
        // group_by == sub_group_by == one mapped path (rejected later with
        // the 400 at views/issue.py:142-146, AFTER the grouper already ran
        // at :138), exactly that key drops out.
        for (group_by, sub_group_by) in [
            ("", ""),
            ("labels__id", ""),
            ("assignees__id", "labels__id"),
            ("priority", "state__group"),
        ] {
            assert_eq!(
                default_annotation_keys(group_by, sub_group_by),
                vec!["assignee_ids", "label_ids", "module_ids"],
                "group_by={group_by} sub_group_by={sub_group_by}"
            );
        }
        assert_eq!(
            default_annotation_keys("issue_module__module_id", "issue_module__module_id"),
            vec!["assignee_ids", "label_ids"]
        );
        assert_eq!(
            default_annotation_keys("labels__id", "labels__id"),
            vec!["assignee_ids", "module_ids"]
        );
        for key in ["assignee_ids", "label_ids", "module_ids"] {
            let sql = default_annotation_sql(key).unwrap();
            assert!(sql.contains("COALESCE(ARRAY_AGG(DISTINCT"), "{key}");
            assert!(sql.contains("FILTER (WHERE"), "{key}");
            assert!(sql.ends_with(&format!("AS \"{key}\"")));
        }
        assert_eq!(default_annotation_sql("priority"), None);
    }

    #[test]
    fn pagination_mismatch_count_filter_and_windows() {
        assert_eq!(
            GROUP_MISMATCH_BODY,
            r#"{"error": "Group by and sub group by cannot have same parameters"}"#
        );
        assert!(
            squashed(FIXTURE_SQL).contains("Group by and sub group by cannot have same parameters")
        );
        // QUIRK-count-filter-intake: 1/-1/2-or-null + archived-null + draft.
        let count = count_filter_sql();
        for frag in [
            "\"issue_intake\".\"status\" = 1",
            "\"issue_intake\".\"status\" = -1",
            "\"issue_intake\".\"status\" = 2",
            "\"issue_intake\".\"id\" IS NULL",
            "\"issues\".\"archived_at\" IS NULL",
            "\"issues\".\"is_draft\" = false",
        ] {
            assert!(count.contains(frag), "{frag}");
        }
        let grouped = grouped_window_sql("priority", "-created_at");
        assert!(grouped.contains("PARTITION BY priority"));
        assert!(grouped.contains("ROW_NUMBER()"));
        assert!(grouped.contains("\"row_number\" > {offset} AND \"row_number\" < {stop}"));
        let subgrouped = subgrouped_window_sql("priority", "state__group", "-created_at");
        assert!(subgrouped.contains("PARTITION BY priority, state__group"));
        assert!(subgrouped.contains("\"row_number\" > {offset} AND \"row_number\" < {stop}"));
        let totals = group_totals_sql("priority");
        assert!(totals.contains("COUNT(DISTINCT \"issues\".\"id\")"));
        assert!(totals.contains(&count));
        let subtotals = subgroup_totals_sql("priority", "state__group");
        assert!(subtotals.contains("GROUP BY priority, state__group"));
        assert_eq!(UNGROUPED_DEFAULT_LIMIT, 1000);
        assert_eq!(GROUPED_DEFAULT_LIMIT, 50);
        assert_eq!(SUBGROUPED_DEFAULT_LIMIT, 30);
    }

    #[test]
    fn group_values_branches_and_sentinels() {
        assert_eq!(
            static_group_values("priority"),
            Some(&["low", "medium", "high", "urgent", "none"][..])
        );
        assert_eq!(static_group_values("state__group"), Some(STATE_GROUP_ORDER));
        assert_eq!(static_group_values("state_id"), None);
        let labels = group_values_query("labels__id").unwrap();
        assert!(labels.none_sentinel);
        let states = group_values_query("state_id").unwrap();
        assert!(!states.none_sentinel);
        assert!(states.extra_where.contains("is_triage"));
        let members = group_values_query("assignees__id").unwrap();
        assert!(members.extra_where.contains("is_active"));
        // BUG-assignees-unwrapped: project-scoped assignees lose list().
        assert!(!group_values_needs_list_wrap("assignees__id", true));
        assert!(group_values_needs_list_wrap("assignees__id", false));
        assert!(group_values_needs_list_wrap("labels__id", true));
        // BUG-group-values-no-queryset: date/author branches need queryset.
        for field in ["target_date", "start_date", "created_by"] {
            assert!(group_values_requires_queryset(field), "{field}");
            assert!(group_values_query(field).is_some(), "{field}");
        }
        assert!(!group_values_requires_queryset("priority"));
        assert_eq!(group_values_query("nope"), None);
    }

    #[test]
    fn on_results_fields_swap_grouped_axes() {
        // Ungrouped: 14 required + original 3 + vote/reaction items.
        let ungrouped = on_results_fields("", "");
        assert_eq!(ungrouped.len(), 14 + 3 + 2);
        assert_eq!(
            &ungrouped[14..17],
            &["assignee_ids", "label_ids", "module_ids"]
        );
        assert_eq!(&ungrouped[17..], &["vote_items", "reaction_items"]);
        // Grouped: the grouped axis swaps its original entry for the lookup.
        let grouped = on_results_fields("labels__id", "");
        assert!(grouped.contains(&"labels__id".to_string()));
        assert!(!grouped.contains(&"label_ids".to_string()));
        assert!(grouped.contains(&"assignee_ids".to_string()));
        let sub = on_results_fields("priority", "assignees__id");
        assert!(sub.contains(&"assignees__id".to_string()));
        assert!(!sub.contains(&"assignee_ids".to_string()));
        // Item annotations carry the avatar_url fallback shape.
        for sql in [vote_items_annotation_sql(), reaction_items_annotation_sql()] {
            assert!(sql.contains("'/api/assets/v2/static/'"));
            assert!(sql.contains("CONCAT("));
            assert!(sql.contains("FILTER (WHERE"));
            assert!(sql.contains("DISTINCT CASE WHEN"));
        }
        assert!(vote_items_annotation_sql().ends_with("AS \"vote_items\""));
        assert!(reaction_items_annotation_sql().ends_with("AS \"reaction_items\""));
    }

    fn fixture_row() -> serde_json::Value {
        serde_json::from_str(FIXTURE_ROWS).expect("rows fixture parses")
    }

    fn sample_row() -> IssueListRow {
        let actor = ActorDetails {
            id: "11111111-1111-1111-1111-111111111111".to_string(),
            first_name: "Ada".to_string(),
            last_name: "L".to_string(),
            avatar: String::new(),
            avatar_url: String::new(),
            display_name: "Ada L".to_string(),
        };
        IssueListRow {
            id: "99999999-9999-9999-9999-999999999999".to_string(),
            name: "Login broken".to_string(),
            state_id: "44444444-4444-4444-4444-444444444444".to_string(),
            sort_order: 65535.0,
            estimate_point: None,
            priority: "high".to_string(),
            start_date: None,
            target_date: Some("2026-10-01".to_string()),
            sequence_id: 42,
            project_id: "33333333-3333-3333-3333-333333333333".to_string(),
            parent_id: None,
            cycle_id: Some("55555555-5555-5555-5555-555555555555".to_string()),
            created_by: Some("11111111-1111-1111-1111-111111111111".to_string()),
            state_group: "started".to_string(),
            assignee_ids: vec!["11111111-1111-1111-1111-111111111111".to_string()],
            label_ids: vec!["77777777-7777-7777-7777-777777777777".to_string()],
            module_ids: vec![],
            vote_items: vec![VoteItem {
                vote: 1,
                actor_details: actor.clone(),
            }],
            reaction_items: vec![ReactionItem {
                reaction: "+1".to_string(),
                actor_details: actor,
            }],
        }
    }

    #[test]
    fn row_byte_matches_fixture() {
        assert_eq!(
            serde_json::to_string(&sample_row()).unwrap(),
            r#"{"id":"99999999-9999-9999-9999-999999999999","name":"Login broken","state_id":"44444444-4444-4444-4444-444444444444","sort_order":65535.0,"estimate_point":null,"priority":"high","start_date":null,"target_date":"2026-10-01","sequence_id":42,"project_id":"33333333-3333-3333-3333-333333333333","parent_id":null,"cycle_id":"55555555-5555-5555-5555-555555555555","created_by":"11111111-1111-1111-1111-111111111111","state__group":"started","assignee_ids":["11111111-1111-1111-1111-111111111111"],"label_ids":["77777777-7777-7777-7777-777777777777"],"module_ids":[],"vote_items":[{"vote":1,"actor_details":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Ada","last_name":"L","avatar":"","avatar_url":"","display_name":"Ada L"}}],"reaction_items":[{"reaction":"+1","actor_details":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Ada","last_name":"L","avatar":"","avatar_url":"","display_name":"Ada L"}}]}"#
        );
        assert_eq!(
            serde_json::to_value(sample_row()).unwrap(),
            fixture_row()["rows"][0]
        );
        assert_eq!(
            fixture_row()["rows_when_empty"],
            serde_json::Value::Array(vec![])
        );
    }
}
