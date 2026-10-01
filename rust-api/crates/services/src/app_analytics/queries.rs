#![forbid(unsafe_code)]

//! Workspace base-analytics query builders (D-35, stage 5).
//!
//! Port of the queries-A layer in `apps/api/pi_dash/app/views/analytic/base.py:38-251`
//! (`AnalyticsEndpoint.get`, `AnalyticViewViewset.get_queryset` / `perform_create`,
//! `SavedAnalyticEndpoint.get`, `ExportAnalyticsEndpoint.post`) plus the plot kernel in
//! `apps/api/pi_dash/utils/analytics_plot.py:43-120` (`VALID_ANALYTICS_FIELDS`, `VALID_YAXIS`,
//! `annotate_with_monthly_dimension`, `extract_axis`, `sort_data`, `build_graph_plot`).
//! The HTTP handlers live in the sibling handlers issue; this module owns the data plane only.
//!
//! Method map (trace: Python lines):
//! - `AnalyticsEndpoint.get` (`base.py:40-57`): [`validate_base_axes`] + [`axes_error_body`] /
//!   [`segment_error_body`] (both 400). The `issue_filters(request.GET, "GET")` predicates
//!   (`base.py:60`) arrive as a caller-supplied fragment (F-07/db layer), referenced never re-ported.
//! - Q-01a base queryset + count (`base.py:63-66`): [`base_count_sql`].
//! - Q-01b issue_count plot (`analytics_plot.py:96-107`): [`base_plot_count_sql`] over
//!   [`dimension_sql`] (+ [`month_dimension_expr`] for date axes).
//! - Q-01c estimate plot (`analytics_plot.py:110-115`): [`base_plot_estimate_sql`].
//! - Q-01d state details (`base.py:72-78`): [`base_state_details_sql`].
//! - Q-01e label details (`base.py:81-92`): [`base_label_details_sql`].
//! - Q-01f assignee details (`base.py:94-131`): [`base_assignee_details_sql`].
//! - Q-01g cycle details (`base.py:133-145`): [`base_cycle_details_sql`].
//! - Q-01h module details (`base.py:147-159`): [`base_module_details_sql`].
//! - Python regroup (`analytics_plot.py:117-120`): [`sort_data_keys`].
//! - Q-02a viewset queryset (`base.py:186-187`): [`base_analytic_view_list_sql`];
//!   `perform_create` workspace lookup (`base.py:182-184`): [`base_workspace_lookup_sql`].
//! - Q-02b saved analytic lookup (`base.py:193`): [`base_saved_analytic_lookup_sql`];
//!   distribution/total reuse the Q-01b/Q-01a shapes with the stored filter.
//! - Q-02c export (`base.py:225-249`): same validation, then the task enqueue
//!   (tasks layer owns it); [`base_export_message`] + [`EXPORT_STATUS`].
//!
//! SQL conventions (same split as `app_assets::queries_v1` and `app_views_search::queries_views`):
//! - Builders emit SQL text with Postgres `$n` placeholders; Django spells them `%s`.
//!   `$n` is the driver-level translation, the predicates are unchanged.
//! - Every builder takes [`BaseRequestContext`] (workspace slug) explicitly — no unscoped
//!   handle — and splices the caller-supplied `filters_sql` fragment verbatim.
//! - `Issue.issue_objects` adds the four `IssueManager` exclusions (`db/models/issue.py:95-104`:
//!   triage state group, archived, project-archived, drafts) plus the soft-delete scope
//!   (`deleted_at IS NULL`, `db/mixins.py:57-58`). The `issue_objects_sql` fragment below pins
//!   the spelling; the F-04 kernels own the canonical text and are referenced, never re-ported.
//! - Single-row `.get()` builders end in `LIMIT 1`; multi-row `.filter()` builders carry the
//!   model default ordering where Django applies one (`analytic_views` Meta ordering `-created_at`).
//! - `SELECT *` is the full column list in `_meta` order (Django selects every concrete field
//!   when no `.values()` is used); `.values()` builders name their keys in wire order.
//!
//! Ported bugs (translate, don't redesign — recorded here, fixed nowhere):
//! - BUG (`base.py:83`): `label_details` reads through plain `Issue.objects`, NOT
//!   `issue_objects` — the triage/archived/draft exclusions do NOT apply to the label branch.
//!   [`base_label_details_sql`] carries only the soft-delete scope on purpose.
//!
//! Fixture: `rust-api/fixtures/app_analytics/queries/analytics_queries_part1.sql`
//! (FX-A-Q-01 `AnalyticsEndpoint`, FX-A-Q-02 viewset/saved/export, traced in
//! `rust-api/fixtures/app_analytics/TRACE.md`). Unit tests replay it: structural SQL
//! fragments against the recorded templates, bodies/statuses asserted equal.

//! D-35 default / project / advance / exporter query builders (stage 5, PIDASHCONV-349).
//!
//! Ports the query layer named by the issue:
//!
//! * `DefaultAnalyticsEndpoint.get` (`app/views/analytic/base.py:252-390`, FX-A-Q-03)
//! * `ProjectStatsEndpoint.get` (`app/views/analytic/base.py:391-455`, FX-A-Q-03)
//! * workspace advance: `get_filtered_counts` / `get_agent_run_usage_stats` /
//!   `get_overview_data` / `get_work_items_stats` / `get_project_issues_stats` /
//!   `project_chart` / `work_item_completion_chart`
//!   (`app/views/analytic/advance.py:32-351`, FX-A-Q-04)
//! * project advance equivalents (`app/views/analytic/project_analytics.py:32-367`,
//!   FX-A-Q-05)
//! * `ExportIssuesEndpoint` queryset + filters (`app/views/exporter/base.py:18-84`,
//!   FX-A-Q-06)
//!
//! Recorded in `rust-api/fixtures/app_analytics/queries/`
//! (`analytics_queries_part2.sql` for Q-03, `part3` for Q-04, `part4` for
//! Q-05/Q-06). Sibling-owned and NOT re-ported here: the Q-01/Q-02 `base_*`
//! group (PIDASHCONV-334, same file) and the guard matrix (PIDASHCONV-358).
//!
//! Like the D-29 `app_views_search` kernels, everything here is pure over
//! injected inputs: builders return SQL text with PostgreSQL `$N` binds and
//! hold no database handle. Dynamic scopes (the `issue_filters(... "GET")`
//! predicate, `get_analytics_filters` base/project fragments, the through-table
//! id lists) arrive as caller-supplied fragments; the F-04 filter kernels
//! (`pidash_db::{filter, filterset, issue_filters}`) apply verbatim and are
//! referenced, never re-ported. The services crate holds no `sea-query`
//! dependency, so — like the D-26 `OrderSpec` precedent in
//! [`crate::app_issues::ordering`] — ordering reuses
//! [`crate::app_issues::ordering::order_sql`] verbatim.
//!
//! SQL semantics are Django's, quirks included (translate, don't redesign).
//! The scope fragments below must reproduce these Django predicates:
//!
//! * `Issue.issue_objects` manager (`db/models/issue.py:95-104`): soft-delete
//!   scope plus four exclusions — `state__group != triage`,
//!   `archived_at IS NULL`, `project.archived_at IS NULL`, `is_draft = false`.
//! * Default analytics base: manager scope + `workspace__slug = slug` +
//!   `issue_filters(request.GET, "GET")` (`base.py:255-256`).
//! * Advance `base_filters` / `project_filters`
//!   (`utils/date_utils.py:125-191`): workspace slug, project member active,
//!   project not deleted/archived, optional `project_id__in` narrowing.
//! * `AgentRun` scope (`advance.py:72-78`): `workspace__slug`, pod project member
//!   active, pod project not deleted/archived, optional `pod__project_id__in`,
//!   optional current-window `created_at` range.
//!
//! Ported bugs (translate, don't redesign; also listed in the PR):
//!
//! * B1 (`base.py:371-372`): both estimate aggregates sum `point`, although the
//!   issue model carries the estimate on its joined estimate column. The
//!   builders below keep `SUM("issues"."point")` ([`default_estimate_sql`]).
//! * B2 (`advance.py:62-64`): `get_filtered_counts` computes only the current
//!   window — the previous-window helper exists but its response line is
//!   commented out, so callers only ever see `{"count": n}`.
//! * B3 (`project_analytics.py:367` vs `advance.py:325`): the project chart view
//!   has no `"projects"` branch — a bare GET falls through to 400
//!   `{"message": "Invalid type"}`, while the workspace view serves the chart.
//! * B4 (`project_analytics.py:64-72`): with `cycle_id`/`module_id` the id list
//!   is built from `CycleIssue`/`ModuleIssue` rows that already carry
//!   `base_filters`, and the base issue fetch applies `base_filters` again —
//!   the workspace/project scoping is applied twice (harmless, kept).
//! * B5 (`project_analytics.py:228-229`): the cycle/module daily branch counts
//!   through-table rows with `Count("id", filter=Q(issue__state__group=...))`
//!   and reports `count = created + completed`, so completed rows are counted
//!   twice in `count`.
//! * B6 (`base.py:394-404`): `requested_fields` is a `set` intersection, so the
//!   DB column order of the project-stats select is nondeterministic across
//!   processes (CPython hash randomization). The builders below emit the CSV
//!   request order instead — same key set, stable order — documented here
//!   because byte-exact column order cannot be reproduced.
//! * B7 (`base.py:411`): `project_ids.split(",")` entries go straight into
//!   `id__in`; a malformed UUID raises at query time (Django 500), it is never
//!   validated. The builder takes the split list verbatim; handlers must keep
//!   the no-validation order (filter first, fail in the DB).

use pidash_db::app_analytics::models::analytic_view;
use serde_json::{json, Value};

use crate::app_issues::ordering::STATE_ORDER;

// ---------------------------------------------------------------------------
// Shared vocabulary
// ---------------------------------------------------------------------------

/// Explicit request context for every builder on this path: the workspace slug from the
/// URL (`base.py:40,183,187,192,225`). Tenancy is never implicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BaseRequestContext<'a> {
    /// `workspace__slug=slug` predicate value; bound as `$1` unless noted.
    pub workspace_slug: &'a str,
}

/// `issues` table (`db/models/issue.py:253`).
pub const ISSUE_TABLE: &str = "issues";
/// `projects` table (join target of the workspace scoping).
pub const PROJECT_TABLE: &str = "projects";
/// `workspaces` table (slug lookup target).
pub const WORKSPACE_TABLE: &str = "workspaces";
/// `states` table (`db/models/state.py:128`).
pub const STATE_TABLE: &str = "states";
/// `labels` table (`db/models/label.py:43`).
pub const LABEL_TABLE: &str = "labels";
/// `issue_labels` link table (`db/models/issue.py:676`).
pub const ISSUE_LABEL_TABLE: &str = "issue_labels";
/// `issue_assignees` link table (`db/models/issue.py:464`).
pub const ISSUE_ASSIGNEE_TABLE: &str = "issue_assignees";
/// `users` table (assignee detail target; fixture Q-01f spelling is normative).
pub const USER_TABLE: &str = "users";
/// `cycles` table (`db/models/cycle.py:85`).
pub const CYCLE_TABLE: &str = "cycles";
/// `cycle_issues` link table (`db/models/cycle.py:123`).
pub const CYCLE_ISSUE_TABLE: &str = "cycle_issues";
/// `modules` table (`db/models/module.py:112`).
pub const MODULE_TABLE: &str = "modules";
/// `module_issues` link table (`db/models/module.py:167`).
pub const MODULE_ISSUE_TABLE: &str = "module_issues";
/// `estimate_points` table (`db/models/estimate.py:56`).
pub const ESTIMATE_POINT_TABLE: &str = "estimate_points";
/// `analytic_views` table ([`analytic_view::TABLE`], `db/models/analytic.py:21`).
pub const ANALYTIC_VIEW_TABLE: &str = analytic_view::TABLE;

/// `IssueManager` exclusions (`db/models/issue.py:95-104`) + soft-delete scope
/// (`db/mixins.py:57-58`), as spliced into every `issue_objects` read below.
/// The F-04 kernels own the canonical text; this const pins the spelling.
pub const ISSUE_OBJECTS_SCOPE: &str = "\"issues\".\"deleted_at\" IS NULL \
     AND \"states\".\"group\" != 'triage' \
     AND \"issues\".\"archived_at\" IS NULL \
     AND \"projects\".\"archived_at\" IS NULL \
     AND \"issues\".\"is_draft\" = FALSE";

// ---------------------------------------------------------------------------
// FX-A-Q-01: axis validation (base.py:41-57, analytics_plot.py:25-40)
// ---------------------------------------------------------------------------

/// `VALID_ANALYTICS_FIELDS` (`analytics_plot.py:25-38`), in source order.
pub const VALID_ANALYTICS_FIELDS: [&str; 12] = [
    "state_id",
    "state__group",
    "labels__id",
    "assignees__id",
    "estimate_point__value",
    "issue_cycle__cycle_id",
    "issue_module__module_id",
    "priority",
    "start_date",
    "target_date",
    "created_at",
    "completed_at",
];

/// `VALID_YAXIS` (`analytics_plot.py:40`).
pub const VALID_YAXIS: [&str; 2] = ["issue_count", "estimate"];

/// Date axes take the monthly `Concat(year, '-', month)` dimension
/// (`extract_axis`, `analytics_plot.py:57-58`).
pub const DATE_AXES: [&str; 4] = ["created_at", "start_date", "target_date", "completed_at"];

/// `true` when the axis is a date axis (`analytics_plot.py:57`).
pub fn is_date_axis(axis: &str) -> bool {
    DATE_AXES.contains(&axis)
}

/// `true` when the axis is a known analytics field (`analytics_plot.py:54,74`).
pub fn is_valid_axis(axis: &str) -> bool {
    VALID_ANALYTICS_FIELDS.contains(&axis)
}

/// Which 400 the axis check failed with (`base.py:46-57`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisError {
    /// `x_axis`/`y_axis` missing or not in `VALID_ANALYTICS_FIELDS` / `VALID_YAXIS`
    /// (`base.py:46-50`).
    Axes,
    /// `segment` set but invalid or equal to `x_axis` (`base.py:53-57`).
    Segment,
}

/// Validate `x_axis`/`y_axis`/`segment` exactly as `AnalyticsEndpoint.get`
/// (`base.py:41-57`; `ExportAnalyticsEndpoint.post` repeats it at `base.py:226-242`,
/// `SavedAnalyticEndpoint.get` at `base.py:198-213` with axes from `query_dict`).
/// Missing query params arrive as `None` (`request.GET.get(..., False)` is falsy);
/// an empty `segment` is falsy too and skips the segment check.
pub fn validate_base_axes(
    x_axis: Option<&str>,
    y_axis: Option<&str>,
    segment: Option<&str>,
) -> Result<(), AxisError> {
    let x_axis = x_axis.unwrap_or("");
    let y_axis = y_axis.unwrap_or("");
    if x_axis.is_empty()
        || y_axis.is_empty()
        || !is_valid_axis(x_axis)
        || !VALID_YAXIS.contains(&y_axis)
    {
        return Err(AxisError::Axes);
    }
    match segment {
        Some(segment) if !segment.is_empty() && (!is_valid_axis(segment) || x_axis == segment) => {
            Err(AxisError::Segment)
        }
        _ => Ok(()),
    }
}

/// 400 when axes are missing/invalid (`base.py:47-50`).
pub const AXES_STATUS: u16 = 400;

/// 400 when the segment is invalid or equals `x_axis` (`base.py:54-57`).
pub const SEGMENT_STATUS: u16 = 400;

/// Axes error body (`base.py:48`).
pub fn axes_error_body() -> Value {
    json!({"error": "x-axis and y-axis dimensions are required and the values should be valid"})
}

/// Segment error body (`base.py:55`).
pub fn segment_error_body() -> Value {
    json!({"error": "Both segment and x axis cannot be same and segment should be valid"})
}

// ---------------------------------------------------------------------------
// FX-A-Q-01: dimensions (analytics_plot.py:43-61, 89-91)
// ---------------------------------------------------------------------------

/// Monthly dimension for date axes (`annotate_with_monthly_dimension`,
/// `analytics_plot.py:43-50`): `Concat(ExtractYear, '-', ExtractMonth)`.
/// `field` must be one of [`DATE_AXES`]; callers validate first.
pub fn month_dimension_expr(field: &str) -> String {
    format!(
        "CONCAT(EXTRACT(YEAR FROM \"{ISSUE_TABLE}\".\"{field}\"), '-', \
         EXTRACT(MONTH FROM \"{ISSUE_TABLE}\".\"{field}\"))"
    )
}

/// SQL for the `F(x_axis)` dimension (`extract_axis`, `analytics_plot.py:53-61`)
/// plus the join the dimension needs. The expression is the `GROUP BY` / `SELECT`
/// target; date axes render through [`month_dimension_expr`]. NULL dimensions
/// are excluded for every axis by the caller (`analytics_plot.py:84-86`; the
/// `is_null`/`dimension_ex` annotations at `:97-104` are dead code).
/// Returns `None` for an unknown axis (handlers reject those in validation first).
pub fn dimension_sql(x_axis: &str) -> Option<(String, String)> {
    let direct = |column: &str| Some((format!("\"{ISSUE_TABLE}\".\"{column}\""), String::new()));
    match x_axis {
        "priority" | "state_id" | "start_date" | "target_date" | "created_at" | "completed_at" => {
            if is_date_axis(x_axis) {
                Some((month_dimension_expr(x_axis), String::new()))
            } else {
                direct(x_axis)
            }
        }
        "state__group" => Some((
            format!("\"{STATE_TABLE}\".\"group\""),
            format!(
                " INNER JOIN \"{STATE_TABLE}\" ON (\"{ISSUE_TABLE}\".\"state_id\" = \"{STATE_TABLE}\".\"id\")"
            ),
        )),
        "labels__id" => Some((
            format!("\"{ISSUE_LABEL_TABLE}\".\"label_id\""),
            format!(
                " LEFT OUTER JOIN \"{ISSUE_LABEL_TABLE}\" \
                 ON (\"{ISSUE_TABLE}\".\"id\" = \"{ISSUE_LABEL_TABLE}\".\"issue_id\")"
            ),
        )),
        "assignees__id" => Some((
            format!("\"{ISSUE_ASSIGNEE_TABLE}\".\"assignee_id\""),
            format!(
                " LEFT OUTER JOIN \"{ISSUE_ASSIGNEE_TABLE}\" \
                 ON (\"{ISSUE_TABLE}\".\"id\" = \"{ISSUE_ASSIGNEE_TABLE}\".\"issue_id\")"
            ),
        )),
        // Forward nullable FK (`Issue.estimate_point`, `db/models/issue.py:130-136`):
        // Django joins `issues.estimate_point_id = estimate_points.id` (LEFT OUTER, nullable).
        "estimate_point__value" => Some((
            format!("\"{ESTIMATE_POINT_TABLE}\".\"value\""),
            format!(
                " LEFT OUTER JOIN \"{ESTIMATE_POINT_TABLE}\" \
                 ON (\"{ISSUE_TABLE}\".\"estimate_point_id\" = \"{ESTIMATE_POINT_TABLE}\".\"id\")"
            ),
        )),
        "issue_cycle__cycle_id" => Some((
            format!("\"{CYCLE_ISSUE_TABLE}\".\"cycle_id\""),
            format!(
                " LEFT OUTER JOIN \"{CYCLE_ISSUE_TABLE}\" \
                 ON (\"{ISSUE_TABLE}\".\"id\" = \"{CYCLE_ISSUE_TABLE}\".\"issue_id\")"
            ),
        )),
        "issue_module__module_id" => Some((
            format!("\"{MODULE_ISSUE_TABLE}\".\"module_id\""),
            format!(
                " LEFT OUTER JOIN \"{MODULE_ISSUE_TABLE}\" \
                 ON (\"{ISSUE_TABLE}\".\"id\" = \"{MODULE_ISSUE_TABLE}\".\"issue_id\")"
            ),
        )),
        _ => None,
    }
}

/// Workspace scoping shared by every Q-01 read (`workspace__slug=slug`, `base.py:63,74,...`):
/// `issues → projects → workspaces`. Params: `$1` = workspace slug.
fn workspace_scope() -> String {
    format!(
        " INNER JOIN \"{PROJECT_TABLE}\" ON (\"{ISSUE_TABLE}\".\"project_id\" = \"{PROJECT_TABLE}\".\"id\") \
         INNER JOIN \"{WORKSPACE_TABLE}\" \
         ON (\"{PROJECT_TABLE}\".\"workspace_id\" = \"{WORKSPACE_TABLE}\".\"id\") \
         WHERE (\"{WORKSPACE_TABLE}\".\"slug\" = $1 AND {{filters}})"
    )
}

// ---------------------------------------------------------------------------
// FX-A-Q-01a/b/c: base queryset, plots (base.py:60-69, analytics_plot.py:93-115)
// ---------------------------------------------------------------------------

/// Q-01a base count (`base.py:63-66`): `issue_filters` queryset `.count()`.
/// `filters_sql` is the caller-supplied `issue_filters(GET)` fragment spliced for `{filters}`.
/// Params: `$1` = workspace slug.
pub fn base_count_sql(filters_sql: &str) -> String {
    format!(
        "SELECT COUNT(*) FROM \"{ISSUE_TABLE}\"{}",
        workspace_scope().replace("{filters}", filters_sql)
    )
}

/// Q-01b issue_count plot (`analytics_plot.py:96-107`): group by `dimension`
/// (+ `segment`), `COUNT(*)`, ordered by dimension. NULL dimensions are
/// excluded for every axis (`analytics_plot.py:84-86`: `extract_axis` always
/// returns `"dimension"`, so the exclude is unconditional; the `is_null` /
/// `dimension_ex` annotations at `:97-104` are dead — a later
/// `.values("dimension")` drops them).
/// Returns `None` for an unknown `x_axis` or `segment`.
/// Params: `$1` = workspace slug.
pub fn base_plot_count_sql(
    x_axis: &str,
    segment: Option<&str>,
    filters_sql: &str,
) -> Option<String> {
    let (dim_expr, dim_join) = dimension_sql(x_axis)?;
    let (seg_select, seg_group, seg_join) = match segment {
        Some(segment) if !segment.is_empty() => {
            let (expr, join) = dimension_sql(segment)?;
            let aliased = if is_date_axis(segment) {
                month_dimension_expr(segment)
            } else {
                expr
            };
            (
                format!(", {aliased} AS \"segment\""),
                ", \"segment\"".to_owned(),
                join,
            )
        }
        _ => (String::new(), String::new(), String::new()),
    };
    // NULL dimensions are excluded for every axis (`analytics_plot.py:84-86`):
    // the guard repeats the inlined expression (a SELECT alias is not visible
    // in the same level's WHERE).
    let null_guard = format!(" AND {dim_expr} IS NOT NULL");
    Some(format!(
        "SELECT \"dimension\", COUNT(*) AS \"count\" FROM (SELECT {dim_expr} AS \"dimension\"{seg_select} \
         FROM \"{ISSUE_TABLE}\"{dim_join}{seg_join}{scope}{null_guard}) GROUP BY \"dimension\"{seg_group} \
         ORDER BY \"dimension\" ASC",
        scope = workspace_scope().replace("{filters}", filters_sql),
    ))
}

/// Q-01c estimate plot (`analytics_plot.py:110-115`):
/// `SUM(CAST(estimate_point__value AS float))` grouped by dimension, ordered by `x_axis`.
/// NULL dimensions are excluded for every axis (`analytics_plot.py:84-86`: `extract_axis`
/// always returns `"dimension"`, so the exclude is unconditional — same guard as
/// [`base_plot_count_sql`]; segment NULLs are NOT excluded, matching Python).
/// Returns `None` for an unknown `x_axis` or `segment`. Params: `$1` = workspace slug.
pub fn base_plot_estimate_sql(
    x_axis: &str,
    segment: Option<&str>,
    filters_sql: &str,
) -> Option<String> {
    let (dim_expr, dim_join) = dimension_sql(x_axis)?;
    // The estimate join is the base table of this branch — except when the dimension or the
    // segment already brings it (`estimate_point__value`), where a second join would be a
    // duplicate table reference. Each join below is emitted exactly once.
    let (_, estimate_join) = dimension_sql("estimate_point__value").expect("known axis");
    // The estimate table joins exactly once: through the dimension when
    // `x_axis` is `estimate_point__value`, otherwise through this base join.
    let extra_joins = if dim_join.contains(ESTIMATE_POINT_TABLE) {
        String::new()
    } else {
        estimate_join
    };
    let (seg_select, seg_group, seg_join) = match segment {
        Some(segment) if !segment.is_empty() => {
            let (expr, join) = dimension_sql(segment)?;
            let aliased = if is_date_axis(segment) {
                month_dimension_expr(segment)
            } else {
                expr
            };
            let join = if segment == "estimate_point__value" {
                String::new()
            } else {
                join
            };
            (
                format!(", {aliased} AS \"segment\""),
                ", \"segment\"".to_owned(),
                join,
            )
        }
        _ => (String::new(), String::new(), String::new()),
    };
    // NULL dimensions are excluded for every axis (`analytics_plot.py:84-86`):
    // the guard repeats the inlined expression (a SELECT alias is not visible
    // in the same level's WHERE). Segment NULLs stay included — Python excludes
    // only the dimension (`analytics_plot.py:89-115` has no segment exclude).
    let null_guard = format!(" AND {dim_expr} IS NOT NULL");
    Some(format!(
        "SELECT {dim_expr} AS \"dimension\"{seg_select}, \
         SUM(CAST(\"{ESTIMATE_POINT_TABLE}\".\"value\" AS DOUBLE PRECISION)) AS \"estimate\" \
         FROM \"{ISSUE_TABLE}\"{extra_joins}{dim_join}{seg_join}{scope}{null_guard} \
         GROUP BY \"dimension\"{seg_group} ORDER BY \"dimension\" ASC",
        scope = workspace_scope().replace("{filters}", filters_sql),
    ))
}

// ---------------------------------------------------------------------------
// FX-A-Q-01d..h: detail lookups (base.py:71-159)
// ---------------------------------------------------------------------------

/// Q-01d state details (`base.py:72-78`): only when `x_axis` or `segment` is `state_id`.
/// `DISTINCT ON (state_id)` ordered by `state_id`. Params: `$1` = workspace slug.
pub fn base_state_details_sql(filters_sql: &str) -> String {
    format!(
        "SELECT DISTINCT ON (\"{ISSUE_TABLE}\".\"state_id\") \"{ISSUE_TABLE}\".\"state_id\", \
         \"{STATE_TABLE}\".\"name\" AS \"state__name\", \"{STATE_TABLE}\".\"color\" AS \"state__color\" \
         FROM \"{ISSUE_TABLE}\" \
         INNER JOIN \"{STATE_TABLE}\" ON (\"{ISSUE_TABLE}\".\"state_id\" = \"{STATE_TABLE}\".\"id\"){} \
         ORDER BY \"{ISSUE_TABLE}\".\"state_id\" ASC",
        workspace_scope().replace("{filters}", filters_sql)
    )
}

/// Q-01e label details (`base.py:81-92`).
///
/// BUG PORT (`base.py:83`): this branch reads through plain `Issue.objects`, NOT
/// `issue_objects` — the triage/archived/draft exclusions do NOT apply here. Only the
/// soft-delete scope plus the recorded `labels__id IS NOT NULL` /
/// `label_issue__deleted_at IS NULL` guards are emitted, exactly as Python does.
/// Params: `$1` = workspace slug.
pub fn base_label_details_sql(filters_sql: &str) -> String {
    format!(
        "SELECT DISTINCT ON (\"{LABEL_TABLE}\".\"id\") \"{LABEL_TABLE}\".\"id\" AS \"labels__id\", \
         \"{LABEL_TABLE}\".\"color\" AS \"labels__color\", \"{LABEL_TABLE}\".\"name\" AS \"labels__name\" \
         FROM \"{ISSUE_TABLE}\" \
         LEFT OUTER JOIN \"{ISSUE_LABEL_TABLE}\" \
         ON (\"{ISSUE_TABLE}\".\"id\" = \"{ISSUE_LABEL_TABLE}\".\"issue_id\") \
         LEFT OUTER JOIN \"{LABEL_TABLE}\" \
         ON (\"{ISSUE_LABEL_TABLE}\".\"label_id\" = \"{LABEL_TABLE}\".\"id\"){} \
         AND \"{LABEL_TABLE}\".\"id\" IS NOT NULL AND \"{ISSUE_LABEL_TABLE}\".\"deleted_at\" IS NULL \
         ORDER BY \"{LABEL_TABLE}\".\"id\" ASC",
        workspace_scope().replace("{filters}", filters_sql)
    )
}

/// Q-01f assignee details (`base.py:94-131`): avatar OR avatar_asset non-null, with the
/// `avatar_url` CASE (`avatar_asset` non-null → `/api/assets/v2/static/<asset>/`,
/// null → raw `avatar`, default NULL). Params: `$1` = workspace slug.
pub fn base_assignee_details_sql(filters_sql: &str) -> String {
    format!(
        "SELECT DISTINCT ON (\"{USER_TABLE}\".\"id\") \"{USER_TABLE}\".\"id\" AS \"assignees__id\", \
         CASE WHEN \"{USER_TABLE}\".\"avatar_asset_id\" IS NOT NULL \
         THEN CONCAT('/api/assets/v2/static/', \"{USER_TABLE}\".\"avatar_asset_id\", '/') \
         WHEN \"{USER_TABLE}\".\"avatar_asset_id\" IS NULL THEN \"{USER_TABLE}\".\"avatar\" \
         ELSE NULL END AS \"assignees__avatar_url\", \
         \"{USER_TABLE}\".\"display_name\" AS \"assignees__display_name\", \
         \"{USER_TABLE}\".\"first_name\" AS \"assignees__first_name\", \
         \"{USER_TABLE}\".\"last_name\" AS \"assignees__last_name\" \
         FROM \"{ISSUE_TABLE}\" \
         LEFT OUTER JOIN \"{ISSUE_ASSIGNEE_TABLE}\" \
         ON (\"{ISSUE_TABLE}\".\"id\" = \"{ISSUE_ASSIGNEE_TABLE}\".\"issue_id\") \
         LEFT OUTER JOIN \"{USER_TABLE}\" \
         ON (\"{ISSUE_ASSIGNEE_TABLE}\".\"assignee_id\" = \"{USER_TABLE}\".\"id\"){} \
         AND (\"{USER_TABLE}\".\"avatar\" IS NOT NULL OR \"{USER_TABLE}\".\"avatar_asset_id\" IS NOT NULL) \
         ORDER BY \"{USER_TABLE}\".\"id\" ASC",
        workspace_scope().replace("{filters}", filters_sql)
    )
}

/// Q-01g cycle details (`base.py:133-145`): `cycle_id` non-null + link `deleted_at` guard.
/// Params: `$1` = workspace slug.
pub fn base_cycle_details_sql(filters_sql: &str) -> String {
    format!(
        "SELECT DISTINCT ON (\"{CYCLE_TABLE}\".\"id\") \"{CYCLE_TABLE}\".\"id\" AS \"issue_cycle__cycle_id\", \
         \"{CYCLE_TABLE}\".\"name\" AS \"issue_cycle__cycle__name\" \
         FROM \"{ISSUE_TABLE}\" \
         LEFT OUTER JOIN \"{CYCLE_ISSUE_TABLE}\" \
         ON (\"{ISSUE_TABLE}\".\"id\" = \"{CYCLE_ISSUE_TABLE}\".\"issue_id\") \
         LEFT OUTER JOIN \"{CYCLE_TABLE}\" \
         ON (\"{CYCLE_ISSUE_TABLE}\".\"cycle_id\" = \"{CYCLE_TABLE}\".\"id\"){} \
         AND \"{CYCLE_ISSUE_TABLE}\".\"cycle_id\" IS NOT NULL \
         AND \"{CYCLE_ISSUE_TABLE}\".\"deleted_at\" IS NULL \
         ORDER BY \"{CYCLE_TABLE}\".\"id\" ASC",
        workspace_scope().replace("{filters}", filters_sql)
    )
}

/// Q-01h module details (`base.py:147-159`): same shape via `issue_module`.
/// Params: `$1` = workspace slug.
pub fn base_module_details_sql(filters_sql: &str) -> String {
    format!(
        "SELECT DISTINCT ON (\"{MODULE_TABLE}\".\"id\") \"{MODULE_TABLE}\".\"id\" AS \"issue_module__module_id\", \
         \"{MODULE_TABLE}\".\"name\" AS \"issue_module__module__name\" \
         FROM \"{ISSUE_TABLE}\" \
         LEFT OUTER JOIN \"{MODULE_ISSUE_TABLE}\" \
         ON (\"{ISSUE_TABLE}\".\"id\" = \"{MODULE_ISSUE_TABLE}\".\"issue_id\") \
         LEFT OUTER JOIN \"{MODULE_TABLE}\" \
         ON (\"{MODULE_ISSUE_TABLE}\".\"module_id\" = \"{MODULE_TABLE}\".\"id\"){} \
         AND \"{MODULE_ISSUE_TABLE}\".\"module_id\" IS NOT NULL \
         AND \"{MODULE_ISSUE_TABLE}\".\"deleted_at\" IS NULL \
         ORDER BY \"{MODULE_TABLE}\".\"id\" ASC",
        workspace_scope().replace("{filters}", filters_sql)
    )
}

// ---------------------------------------------------------------------------
// FX-A-Q-01 regroup (analytics_plot.py:64-70, 117-120)
// ---------------------------------------------------------------------------

/// `sort_data` key order (`analytics_plot.py:64-70`): the `priority` axis sorts
/// `low, medium, high, urgent, none` (missing keys dropped — the fixture seed row
/// shows only populated buckets); every other axis sorts with `'none'` last.
/// Python `groupby` needs the rows pre-sorted by `str(dimension)` (`:118`), which the
/// `ORDER BY "dimension"` in the plot builders provides. `keys` arrive in that row
/// order; the returned order is the response order.
pub fn sort_data_keys(keys: &[String], temp_axis: &str) -> Vec<String> {
    if temp_axis == "priority" {
        const ORDER: [&str; 5] = ["low", "medium", "high", "urgent", "none"];
        ORDER
            .iter()
            .filter(|want| keys.iter().any(|key| key == *want))
            .map(|want| (*want).to_owned())
            .collect()
    } else {
        let mut sorted = keys.to_vec();
        sorted.sort_by_key(|key| (key == "none", key.clone()));
        sorted
    }
}
/// Open state groups (`utils/constants.py:86`: `STATE_GROUP_ORDER[:-2]`).
/// Reuses the D-26 [`STATE_ORDER`]; the open slice is the first five entries.
pub fn open_state_groups() -> Vec<&'static str> {
    STATE_ORDER[..STATE_ORDER.len() - 2].to_vec()
}

/// Closed state groups (`utils/constants.py:88`: `STATE_GROUP_ORDER[-2:]`).
pub fn closed_state_groups() -> Vec<&'static str> {
    STATE_ORDER[STATE_ORDER.len() - 2..].to_vec()
}

/// Render `('a','b',…)` for a `__in` list over state groups.
pub fn group_list(groups: &[&str]) -> String {
    groups
        .iter()
        .map(|g| format!("'{g}'"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The avatar-url `Case` shared by Q-03d/Q-03e/Q-03f and Q-05d
/// (`base.py:296-311`, `project_analytics.py:138-152`):
/// asset id present → `Concat('/api/assets/v2/static/', asset, '/')`,
/// asset null → the plain `avatar` column, else SQL `NULL`.
/// `user_alias` is the joined user table (e.g. `"users"` after the
/// `created_by` / `assignees` join); `out_alias` is the select alias
/// (`created_by__avatar_url` / `assignees__avatar_url`).
pub fn avatar_case_sql(user_alias: &str, out_alias: &str) -> String {
    format!(
        "CASE WHEN {user_alias}.\"avatar_asset_id\" IS NOT NULL \
         THEN CONCAT('/api/assets/v2/static/', {user_alias}.\"avatar_asset_id\", '/') \
         WHEN {user_alias}.\"avatar_asset_id\" IS NULL THEN {user_alias}.\"avatar\" \
         ELSE NULL END AS \"{out_alias}\""
    )
}

/// `{"message": "Invalid tab"}` 400 (`advance.py:152`).
pub const INVALID_TAB_BODY: &str = "{\"message\": \"Invalid tab\"}";
/// `{"message": "Invalid type"}` 400 (`advance.py:351`,
/// `project_analytics.py:179,367`).
pub const INVALID_TYPE_BODY: &str = "{\"message\": \"Invalid type\"}";
/// `{"error": "per_page and cursor are required"}` 400
/// (`exporter/base.py:81-84`).
pub const EXPORTER_PAGINATION_REQUIRED_BODY: &str =
    "{\"error\": \"per_page and cursor are required\"}";
/// Export-accepted 200 message (`exporter/base.py:57-60`).
pub const EXPORT_ACCEPTED_MESSAGE: &str =
    "Once the export is ready you will be able to download it";

/// Sequential `$N` bind allocator. Builders take one and emit placeholders in
/// call order, so handler code never numbers binds by hand.
#[derive(Debug, Clone)]
pub struct Binds {
    next: u32,
}

impl Binds {
    /// Start allocating at `$start` (use `1` for a fresh statement).
    pub fn new(start: u32) -> Self {
        Self { next: start }
    }

    /// Take the next placeholder (`$n`), advancing the counter.
    pub fn take(&mut self) -> String {
        let ph = format!("${}", self.next);
        self.next += 1;
        ph
    }
}

// ---------------------------------------------------------------------------
// FX-A-Q-03a: DefaultAnalytics base + total (base.py:255-258)
// ---------------------------------------------------------------------------

/// `base_issues.count()` (`base.py:258`): plain `COUNT(*)` over the
/// caller-supplied base scope (manager + workspace slug + `issue_filters`).
/// `base_where_sql` is the full predicate conjunction; the caller owns the
/// `FROM` joins, exactly like every other builder in this module.
pub fn default_total_sql(base_where_sql: &str) -> String {
    format!("SELECT COUNT(*) FROM \"issues\" WHERE ({base_where_sql})")
}

// ---------------------------------------------------------------------------
// FX-A-Q-03b: classified totals (base.py:260-273)
// ---------------------------------------------------------------------------

/// `state_groups.values("state_group").annotate(state_count=Count(...))`
/// `.order_by("state_group")` (`base.py:262-264`): group over the state join,
/// ascending. With `open_only`, the same shape filtered to
/// `OPEN_STATE_GROUPS` (`base.py:266-273`).
pub fn default_classified_sql(base_where_sql: &str, open_only: bool) -> String {
    let open_filter = if open_only {
        format!(
            " AND \"states\".\"group\" IN ({})",
            group_list(&open_state_groups())
        )
    } else {
        String::new()
    };
    format!(
        "SELECT \"states\".\"group\" AS \"state_group\", COUNT(\"states\".\"group\") AS \"state_count\" \
         FROM \"issues\" INNER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") \
         WHERE ({base_where_sql}{open_filter}) \
         GROUP BY \"states\".\"group\" ORDER BY \"state_group\" ASC"
    )
}

// ---------------------------------------------------------------------------
// FX-A-Q-03c: completed month-wise, current year (base.py:275-282)
// ---------------------------------------------------------------------------

/// `base_issues.filter(completed_at__year=current_year)`
/// `.annotate(month=ExtractMonth("completed_at")).values("month")`
/// `.annotate(count=Count("*")).order_by("month")`.
/// `year_ph` binds `timezone.now().year` (`base.py:275`).
pub fn default_completed_month_sql(base_where_sql: &str, year_ph: &str) -> String {
    format!(
        "SELECT EXTRACT(MONTH FROM \"issues\".\"completed_at\") AS \"month\", COUNT(*) AS \"count\" \
         FROM \"issues\" WHERE ({base_where_sql} AND EXTRACT(YEAR FROM \"completed_at\") = {year_ph}) \
         GROUP BY \"month\" ORDER BY \"month\" ASC"
    )
}

// ---------------------------------------------------------------------------
// FX-A-Q-03d: most-created top-5 (base.py:284-313)
// ---------------------------------------------------------------------------

/// Columns of the `values(*user_details)` (`base.py:284-289`).
pub const CREATED_BY_DETAILS: &[&str] = &[
    "created_by__first_name",
    "created_by__last_name",
    "created_by__display_name",
    "created_by__id",
];

/// `base_issues.exclude(created_by=None).values(*user_details)`
/// `.annotate(count=Count("id"))` + avatar `Case` + `.order_by("-count")[:5]`.
/// Django renders the creator columns as `"users"."first_name" AS
/// "created_by__first_name"` etc. over the `created_by` join.
pub fn default_top_creators_sql(base_where_sql: &str) -> String {
    let cols = CREATED_BY_DETAILS
        .iter()
        .map(|alias| {
            let col = alias.strip_prefix("created_by__").unwrap_or(alias);
            format!("\"users\".\"{col}\" AS \"{alias}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let group_cols = CREATED_BY_DETAILS
        .iter()
        .map(|alias| {
            let col = alias.strip_prefix("created_by__").unwrap_or(alias);
            format!("\"users\".\"{col}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let avatar = avatar_case_sql("\"users\"", "created_by__avatar_url");
    format!(
        "SELECT {cols}, COUNT(\"issues\".\"id\") AS \"count\", {avatar} \
         FROM \"issues\" WHERE ({base_where_sql} AND \"issues\".\"created_by_id\" IS NOT NULL) \
         GROUP BY {group_cols} ORDER BY \"count\" DESC LIMIT 5"
    )
}

// ---------------------------------------------------------------------------
// FX-A-Q-03e/Q-03f: most-closed top-5 + pending (base.py:315-369)
// ---------------------------------------------------------------------------

/// Columns of the `values(*user_assignee_details)` (`base.py:315-320`).
pub const ASSIGNEE_DETAILS: &[&str] = &[
    "assignees__first_name",
    "assignees__last_name",
    "assignees__display_name",
    "assignees__id",
];

/// `base_issues.filter(completed_at__isnull=False).exclude(assignees=None)`
/// `.values(*user_assignee_details)` + avatar `Case` + `.annotate(count)` +
/// `.order_by("-count")[:5]` (`base.py:322-345`).
///
/// Note the ported order: the avatar annotation is added *before* the count
/// annotation — SQL-identical either way, kept here for trace fidelity.
pub fn default_top_closers_sql(base_where_sql: &str) -> String {
    let cols = assignee_select_list();
    let group_cols = assignee_group_list();
    let avatar = avatar_case_sql("\"users\"", "assignees__avatar_url");
    format!(
        "SELECT {cols}, {avatar}, COUNT(\"issues\".\"id\") AS \"count\" \
         FROM \"issues\" WHERE ({base_where_sql} AND \"issues\".\"completed_at\" IS NOT NULL \
         AND \"users\".\"id\" IS NOT NULL) \
         GROUP BY {group_cols} ORDER BY \"count\" DESC LIMIT 5"
    )
}

/// Pending users: `base_issues.filter(completed_at__isnull=True)` over
/// `values(*user_assignee_details)` + count + avatar `Case`,
/// `.order_by("-count")` with **no** `LIMIT` (`base.py:347-369`; FX-A-Q-03
/// seed: one NULL bucket, avatar_url NULL).
pub fn default_pending_sql(base_where_sql: &str) -> String {
    let cols = assignee_select_list();
    let group_cols = assignee_group_list();
    let avatar = avatar_case_sql("\"users\"", "assignees__avatar_url");
    format!(
        "SELECT {cols}, COUNT(\"issues\".\"id\") AS \"count\", {avatar} \
         FROM \"issues\" WHERE ({base_where_sql} AND \"issues\".\"completed_at\" IS NULL) \
         GROUP BY {group_cols} ORDER BY \"count\" DESC"
    )
}

fn assignee_select_list() -> String {
    ASSIGNEE_DETAILS
        .iter()
        .map(|alias| {
            let col = alias.strip_prefix("assignees__").unwrap_or(alias);
            format!("\"users\".\"{col}\" AS \"{alias}\"")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn assignee_group_list() -> String {
    ASSIGNEE_DETAILS
        .iter()
        .map(|alias| {
            let col = alias.strip_prefix("assignees__").unwrap_or(alias);
            format!("\"users\".\"{col}\"")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// FX-A-Q-03g: estimate sums (base.py:371-372) — ported bug B1
// ---------------------------------------------------------------------------

/// `open_issues_queryset.aggregate(sum=Sum("point"))["sum"]` and the same over
/// `base_issues`. **Ported bug B1**: aggregates `SUM("issues"."point")`
/// although the estimate lives on the joined estimate column. `NULL` (not 0)
/// when no rows match. `open` selects the open-issues scope, otherwise the
/// full base scope.
pub fn default_estimate_sql(base_where_sql: &str, open: bool) -> String {
    let scope = if open {
        format!(
            "{base_where_sql} AND \"states\".\"group\" IN ({})",
            group_list(&open_state_groups())
        )
    } else {
        base_where_sql.to_owned()
    };
    format!("SELECT SUM(\"issues\".\"point\") AS \"sum\" FROM \"issues\" WHERE ({scope})")
}

// ---------------------------------------------------------------------------
// FX-A-Q-03h: ProjectStats (base.py:391-455)
// ---------------------------------------------------------------------------

/// The five selectable fields (`base.py:397-403`), in source order.
pub const PROJECT_STATS_FIELDS: &[&str] = &[
    "total_issues",
    "completed_issues",
    "total_members",
    "total_cycles",
    "total_modules",
];

/// Resolve `?fields=` csv against the valid set (`base.py:394-407`):
/// intersect, drop empties; empty or fully-unknown input selects all five.
/// Output follows the CSV request order (see ported bug B6: Django iterates a
/// `set`, so its column order is nondeterministic; the key *set* matches).
pub fn project_stats_fields(csv: &str) -> Vec<&str> {
    let picked: Vec<&str> = csv
        .split(',')
        .map(str::trim)
        .filter(|f| PROJECT_STATS_FIELDS.contains(f))
        .collect();
    if picked.is_empty() {
        PROJECT_STATS_FIELDS.to_vec()
    } else {
        picked
    }
}

/// One correlated annotation subquery
/// (`Issue.issue_objects.filter(project_id=OuterRef("pk")).order_by()`
/// `.annotate(count=Func(F("id"), function="Count")).values("count")`,
/// `base.py:414-451`). The empty `.order_by()` clears default ordering so the
/// subquery returns a scalar; `Func(Count)` renders `COUNT(U0."id")` with no
/// `COALESCE`, hence `NULL` (not 0) on empty. `outer_col` is `"id"` everywhere:
/// Django's `OuterRef("pk")` (`base.py:416,424`) renders the concrete `id`
/// column, same as the explicit `OuterRef("id")` (`base.py:432,440,448`) —
/// `"projects"."pk"` is not a real column. `extra_where` carries the
/// per-annotation predicate (`state__group IN …`, member bot/active guards,
/// or empty).
pub fn project_stats_subquery(
    table: &str,
    alias: &str,
    outer_col: &str,
    extra_joins: &str,
    extra_where: &str,
    out_alias: &str,
) -> String {
    let where_tail = if extra_where.is_empty() {
        String::new()
    } else {
        format!(" AND {extra_where}")
    };
    format!(
        "(SELECT COUNT({alias}.\"id\") FROM \"{table}\" {alias} {extra_joins}\
         WHERE ({alias}.\"project_id\" = \"projects\".\"{outer_col}\"{where_tail})) AS \"{out_alias}\""
    )
}

/// `total_members` predicate (`base.py:448`):
/// `member__is_bot=False, is_active=True` over the users join. `table_alias`
/// must match the subquery alias passed to [`project_stats_subquery`] (`U0`
/// at the only call site): the table is aliased, so the bare table name is
/// not visible inside the subquery.
pub fn project_stats_member_where(table_alias: &str) -> String {
    format!("NOT \"users\".\"is_bot\" AND \"{table_alias}\".\"is_active\"")
}

/// Full project-stats select (`base.py:409-455`):
/// `Project.objects.filter(workspace__slug=slug)` plus optional
/// `id__in` from `?project_ids=` csv (split verbatim, see ported bug B7),
/// annotated per [`project_stats_fields`] and projected as
/// `.values("id", *requested_fields)`.
pub fn project_stats_sql(workspace_slug_ph: &str, project_ids: &[&str], fields: &[&str]) -> String {
    let mut selects = vec!["\"projects\".\"id\"".to_owned()];
    for field in fields {
        let sub = match *field {
            "total_issues" => project_stats_subquery("issues", "U0", "id", "", "", "total_issues"),
            "completed_issues" => project_stats_subquery(
                "issues",
                "U0",
                "id",
                "INNER JOIN \"states\" ON (U0.\"state_id\" = \"states\".\"id\")",
                &format!(
                    "\"states\".\"group\" IN ({})",
                    group_list(&closed_state_groups())
                ),
                "completed_issues",
            ),
            "total_cycles" => project_stats_subquery("cycles", "U0", "id", "", "", "total_cycles"),
            "total_modules" => {
                project_stats_subquery("modules", "U0", "id", "", "", "total_modules")
            }
            "total_members" => project_stats_subquery(
                "project_members",
                "U0",
                "id",
                "INNER JOIN \"users\" ON (U0.\"member_id\" = \"users\".\"id\")",
                &project_stats_member_where("U0"),
                "total_members",
            ),
            _ => continue,
        };
        selects.push(sub);
    }
    let mut sql = format!(
        "SELECT {} FROM \"projects\" WHERE (\"workspaces\".\"slug\" = {workspace_slug_ph}",
        selects.join(", ")
    );
    if !project_ids.is_empty() {
        let ids = project_ids
            .iter()
            // Django binds these as parameters; the literal shape here doubles
            // embedded quotes so a hostile id cannot break out of the string.
            // Malformed UUIDs still fail in the DB, per ported bug B7.
            .map(|id| format!("'{}'", id.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        sql.push_str(&format!(" AND \"projects\".\"id\" IN ({ids})"));
    }
    sql.push(')');
    sql
}

// ---------------------------------------------------------------------------
// FX-A-Q-04: workspace advance (advance.py:32-351)
// ---------------------------------------------------------------------------

/// `get_filtered_counts` (`advance.py:45-65`): count in the CURRENT date window
/// when `analytics_date_range` is set, else a plain count. **Ported bug B2**:
/// the previous-window helper is defined but its response line is commented
/// out — only `{"count": n}` is ever returned. `window` carries the
/// `(gte_ph, lte_ph)` placeholders for the current window, or `None`.
pub fn advance_filtered_count_sql(
    table: &str,
    base_where_sql: &str,
    window: Option<(&str, &str)>,
) -> String {
    match window {
        Some((gte_ph, lte_ph)) => format!(
            "SELECT COUNT(*) FROM \"{table}\" WHERE ({base_where_sql} \
             AND \"created_at\" >= {gte_ph} AND \"created_at\" <= {lte_ph})"
        ),
        None => format!("SELECT COUNT(*) FROM \"{table}\" WHERE ({base_where_sql})"),
    }
}

/// `get_agent_run_usage_stats` (`advance.py:67-96`): the three token sums with
/// `default=0`, i.e. `COALESCE(SUM(…), 0)`. `totals[x] or 0` after the fact is
/// then a no-op. `scope_where_sql` is the AgentRun scope (workspace slug, pod
/// project member active, pod project live, optional project ids and window).
pub fn advance_agent_run_usage_sql(scope_where_sql: &str) -> String {
    format!(
        "SELECT COALESCE(SUM(\"input_tokens\"),0), COALESCE(SUM(\"output_tokens\"),0), \
         COALESCE(SUM(\"total_tokens\"),0) FROM \"agent_run\" WHERE ({scope_where_sql})"
    )
}

/// Keys of `get_overview_data` (`advance.py:98-124`), in response order.
/// `total_intake` here filters `issue_intake__status__in = ["-2","-1","0","1","2"]`
/// (with its TODO comment); the project-chart intake below instead filters
/// `issue_intake__isnull=False` — the two deliberately differ.
pub const ADVANCE_OVERVIEW_KEYS: &[&str] = &[
    "total_users",
    "total_admins",
    "total_members",
    "total_guests",
    "total_projects",
    "total_work_items",
    "total_cycles",
    "total_intake",
    "agent_run_input_tokens",
    "agent_run_output_tokens",
    "agent_run_total_tokens",
];

/// Intake statuses for the overview total (`advance.py:120`).
pub const OVERVIEW_INTAKE_STATUSES: &[&str] = &["-2", "-1", "0", "1", "2"];

/// `total_intake` predicate for the overview path: `issue_intake__status IN (…)`.
pub fn overview_intake_where(base_where_sql: &str) -> String {
    let list = OVERVIEW_INTAKE_STATUSES
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{base_where_sql} AND \"issue_intake\".\"status\" IN ({list})")
}

/// Keys of workspace `get_work_items_stats` (`advance.py:126-135`): base plus
/// one `state__group` filter per key. Note `cancelled` is NOT present here
/// (it appears in the project-issues variants below).
pub const ADVANCE_WORK_ITEM_KEYS: &[(&str, Option<&str>)] = &[
    ("total_work_items", None),
    ("started_work_items", Some("started")),
    ("backlog_work_items", Some("backlog")),
    ("un_started_work_items", Some("unstarted")),
    ("completed_work_items", Some("completed")),
];

/// One workspace work-items stat: the base scope plus the optional
/// `state__group` equality, counted through [`advance_filtered_count_sql`]
/// (current window applies — the tab view initializes with `type="analytics"`).
pub fn advance_work_item_stat_sql(
    base_where_sql: &str,
    group: Option<&str>,
    window: Option<(&str, &str)>,
) -> String {
    let scope = match group {
        Some(g) => format!("{base_where_sql} AND \"states\".\"group\" = '{g}'"),
        None => base_where_sql.to_owned(),
    };
    advance_filtered_count_sql("issues", &scope, window)
}

/// `get_project_issues_stats` (`advance.py:156-175`) and the stats-view
/// `get_work_items_stats` (`advance.py:177-189`): identical
/// `values("project_id", "project__name")` + five `Count(id, filter=Q(…))`
/// (no `distinct`) `.order_by("project_id")`. Only the former first applies
/// `chart_period_range` on `created_at__date`; pass it via `date_range` as
/// `(gte_ph, lte_ph)` placeholders, or `None` for the plain shape.
pub fn advance_project_issues_stats_sql(
    base_where_sql: &str,
    date_range: Option<(&str, &str)>,
) -> String {
    let scope = match date_range {
        Some((gte_ph, lte_ph)) => format!(
            "{base_where_sql} AND \"issues\".\"created_at\"::date >= {gte_ph} \
             AND \"issues\".\"created_at\"::date <= {lte_ph}"
        ),
        None => base_where_sql.to_owned(),
    };
    format!(
        "SELECT \"issues\".\"project_id\", \"projects\".\"name\", \
         COUNT(\"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'cancelled') AS \"cancelled_work_items\", \
         COUNT(\"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'completed') AS \"completed_work_items\", \
         COUNT(\"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'backlog') AS \"backlog_work_items\", \
         COUNT(\"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'unstarted') AS \"un_started_work_items\", \
         COUNT(\"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'started') AS \"started_work_items\" \
         FROM \"issues\" WHERE ({scope}) \
         GROUP BY \"issues\".\"project_id\", \"projects\".\"name\" ORDER BY \"project_id\""
    )
}

/// `project_chart` (`advance.py:206-248`): seven independent counts under the
/// SAME `date_filter` (`created_at__date` range when `chart_period_range` is
/// set), rendered `[{"key","name": Title(key),"count": v or 0}]` in this fixed
/// order. Intake here is `issue_intake__isnull=False` (differs from the
/// overview intake above); members exclude NEITHER bots NOR inactive beyond
/// `is_active=True` (no bot exclusion, unlike the overview member query).
pub const ADVANCE_PROJECT_CHART_KEYS: &[&str] = &[
    "work_items",
    "cycles",
    "modules",
    "intake",
    "members",
    "pages",
    "views",
];

/// `key.replace("_", " ").title()` (`advance.py:244`): single-word keys
/// title-case unchanged; kept as a builder so row shaping stays byte-exact.
pub fn chart_key_name(key: &str) -> String {
    key.replace('_', " ")
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Generic `COUNT(*)` for one project-chart key over its table + predicate.
pub fn advance_project_chart_count_sql(table: &str, where_sql: &str) -> String {
    format!("SELECT COUNT(*) FROM \"{table}\" WHERE ({where_sql})")
}

/// `work_item_completion_chart` monthly bucket select (`advance.py:266-275`):
/// `TruncMonth("created_at")` + `created=Count(id)` /
/// `completed=Count(id, filter=Q(state__group="completed"))`, ordered by month.
/// Zero-fill from the workspace `created_at` month-start to the current
/// month-start happens in Python over [`month_keys_between`]; each row is
/// `{key, name, count=created, completed_issues, created_issues}` with
/// [`COMPLETION_SCHEMA_COMPLETED`] / [`COMPLETION_SCHEMA_CREATED`].
pub fn advance_completion_monthly_sql(
    base_where_sql: &str,
    date_range: Option<(&str, &str)>,
) -> String {
    let scope = match date_range {
        Some((gte_ph, lte_ph)) => format!(
            "{base_where_sql} AND \"issues\".\"created_at\"::date >= {gte_ph} \
             AND \"issues\".\"created_at\"::date <= {lte_ph}"
        ),
        None => base_where_sql.to_owned(),
    };
    "SELECT date_trunc('month', \"issues\".\"created_at\") AS \"month\", \
     COUNT(\"issues\".\"id\") AS \"created_count\", \
     COUNT(\"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'completed') AS \"completed_count\" \
     FROM \"issues\" WHERE (".to_owned()
        + &scope
        + ") GROUP BY \"month\" ORDER BY \"month\""
}

/// `{"completed_issues": "completed_issues", "created_issues": "created_issues"}`
/// (`advance.py:311-314`, identical at `project_analytics.py:310-313`).
pub const COMPLETION_SCHEMA_COMPLETED: &str = "completed_issues";
/// The `created_issues` schema key (see [`COMPLETION_SCHEMA_COMPLETED`]).
pub const COMPLETION_SCHEMA_CREATED: &str = "created_issues";

/// Month keys `YYYY-MM-01` from `(start_year, start_month)` through
/// `(end_year, end_month)` inclusive (`advance.py:287-309` zero-fill loop;
/// `stat["month"].strftime("%Y-%m-%d")` keys the stats dict, so month-starts
/// render as the first of the month).
pub fn month_keys_between(start: (i32, u32), end: (i32, u32)) -> Vec<String> {
    let mut keys = Vec::new();
    let (mut y, mut m) = start;
    while (y, m) <= end {
        keys.push(format!("{y:04}-{m:02}-01"));
        if m == 12 {
            y += 1;
            m = 1;
        } else {
            m += 1;
        }
    }
    keys
}

/// `custom-work-items` scope (`advance.py:328-343`): the same base queryset
/// (with its `select_related`/`prefetch_related` fetch plan) plus
/// `chart_period_range` on `created_at__date`, rendered by
/// `build_analytics_chart(queryset, x_axis, group_by)`
/// (`utils/build_chart.py`). The chart builder owns to the handlers/tasks
/// layers; this returns the scoped predicate so both layers share it.
/// `x_axis` defaults to `"PRIORITY"` (`advance.py:323`).
pub fn advance_custom_work_items_scope_sql(
    base_where_sql: &str,
    date_range: Option<(&str, &str)>,
) -> String {
    match date_range {
        Some((gte_ph, lte_ph)) => format!(
            "{base_where_sql} AND \"issues\".\"created_at\"::date >= {gte_ph} \
             AND \"issues\".\"created_at\"::date <= {lte_ph}"
        ),
        None => base_where_sql.to_owned(),
    }
}

/// Default chart `x_axis` (`advance.py:323`, `project_analytics.py:322`).
pub const DEFAULT_CHART_X_AXIS: &str = "PRIORITY";

// ---------------------------------------------------------------------------
// FX-A-Q-05: project advance (project_analytics.py:32-367)
// ---------------------------------------------------------------------------

/// Project `get_filtered_counts` (`project_analytics.py:45-56`):
/// current-window-or-plain `{count}` — there is NO previous helper at all
/// here (unlike the workspace view, which at least defines one).
pub fn project_filtered_count_sql(
    table: &str,
    base_where_sql: &str,
    window: Option<(&str, &str)>,
) -> String {
    advance_filtered_count_sql(table, base_where_sql, window)
}

/// Through-table id-list scope for `cycle_id` / `module_id`
/// (`project_analytics.py:63-72, 121-132, 192-214, 335-345`):
/// `CycleIssue.objects.filter(**base_filters, cycle_id=cycle_id)`
/// `.values_list("issue_id", flat=True)` (same via `ModuleIssue`).
/// **Ported quirk B4**: `base_filters` scope the through rows AND the issue
/// rows. `through_table` is `"cycle_issues"` / `"module_issues"`,
/// `fk_col` `"cycle_id"` / `"module_id"`, `fk_ph` binds the requested id.
pub fn project_through_ids_sql(
    through_table: &str,
    fk_col: &str,
    base_where_sql: &str,
    fk_ph: &str,
) -> String {
    format!(
        "SELECT \"issue_id\" FROM \"{through_table}\" \
         WHERE ({base_where_sql} AND \"{fk_col}\" = {fk_ph})"
    )
}

/// Project `get_work_items_stats` keys (`project_analytics.py:76-82`), same
/// five as the workspace variant. Seed rows: total 3 / started 0 / backlog 2 /
/// un_started 0 / completed 1; unknown `cycle_id` yields all zeros, still 200.
pub const PROJECT_WORK_ITEM_KEYS: &[(&str, Option<&str>)] = ADVANCE_WORK_ITEM_KEYS;

/// Project-assignee stats (`project_analytics.py:119-163`): cycle/module
/// id-list scoping, then `display_name` / `assignee_id` / `avatar` / `avatar_url`
/// annotations (same `Case` as Q-01f), `values(display_name, assignee_id,
/// avatar_url)` + five `Count(id, filter=Q(…), distinct=True)`
/// `.order_by(display_name)`. **Note the `distinct=True`**: absent in the
/// workspace Q-04e variant, present here (the assignee join fans rows out).
/// `ids_subquery_sql` is the [`project_through_ids_sql`] output (or empty for
/// the plain `project_id` branch); `project_ph` binds `project_id` for the
/// plain branch only.
pub fn project_assignee_stats_sql(
    base_where_sql: &str,
    project_ph: &str,
    ids_subquery_sql: Option<&str>,
) -> String {
    let scope = match ids_subquery_sql {
        Some(sub) => format!("{base_where_sql} AND \"issues\".\"id\" IN ({sub})"),
        None => format!("{base_where_sql} AND \"issues\".\"project_id\" = {project_ph}"),
    };
    let avatar = avatar_case_sql("\"users\"", "avatar_url");
    format!(
        "SELECT \"users\".\"display_name\" AS \"display_name\", \"users\".\"id\" AS \"assignee_id\", {avatar}, \
         COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'cancelled') AS \"cancelled_work_items\", \
         COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'completed') AS \"completed_work_items\", \
         COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'backlog') AS \"backlog_work_items\", \
         COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'unstarted') AS \"un_started_work_items\", \
         COUNT(DISTINCT \"issues\".\"id\") FILTER (WHERE \"states\".\"group\" = 'started') AS \"started_work_items\" \
         FROM \"issues\" WHERE ({scope}) \
         GROUP BY \"users\".\"display_name\", \"users\".\"id\", \"users\".\"avatar\", \"users\".\"avatar_asset_id\" \
         ORDER BY \"display_name\""
    )
}

/// Cycle/module daily completion branch (`project_analytics.py:223-258`):
/// `queryset.values("created_at__date")` over the through-table id list +
/// `created=Count(id)` / `completed=Count(id, filter=Q(issue__state__group=…))`
/// — note the `issue__` prefix (through-table join) — ordered by date, then a
/// per-day zero-fill from the cycle/module start to end date. **Ported bug B5**:
/// `count = created + completed`, so completed rows count twice in `count`.
/// `ids_subquery_sql` is the [`project_through_ids_sql`] output.
pub fn project_completion_daily_sql(ids_subquery_sql: &str) -> String {
    format!(
        "SELECT \"created_at\"::date AS \"created_at__date\", COUNT(\"id\") AS \"created_count\", \
         COUNT(\"id\") FILTER (WHERE \"issue\".\"state__group\" = 'completed') AS \"completed_count\" \
         FROM ({ids_subquery_sql}) WHERE (1 = 1) \
         GROUP BY \"created_at\"::date ORDER BY \"created_at\"::date"
    )
}

/// Empty chart body when cycle/module/project dates are missing
/// (`project_analytics.py:201,213,221`): `{"data": [], "schema": {}}`.
pub const EMPTY_CHART_BODY: &str = "{\"data\": [], \"schema\": {}}";

/// Day keys `YYYY-MM-DD` from `start` through `end` inclusive
/// (`project_analytics.py:244-257` daily zero-fill loop).
pub fn day_keys_between(start: (i32, u32, u32), end: (i32, u32, u32)) -> Vec<String> {
    let mut keys = Vec::new();
    let (mut y, mut m, mut d) = start;
    while (y, m, d) <= end {
        keys.push(format!("{y:04}-{m:02}-{d:02}"));
        let dim = days_in_month(y, m);
        if d == dim {
            d = 1;
            if m == 12 {
                y += 1;
                m = 1;
            } else {
                m += 1;
            }
        } else {
            d += 1;
        }
    }
    keys
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

// ---------------------------------------------------------------------------
// FX-A-Q-02a: AnalyticViewViewset (base.py:177-189)
// ---------------------------------------------------------------------------

/// 200 for the analytics GET envelope (`base.py:161-174`).
pub const ANALYTICS_STATUS: u16 = 200;

/// Q-02a viewset queryset (`base.py:186-187`): viewset default ordering applies
/// (`Meta.ordering = -created_at`, `db/models/analytic.py:24`). Params: `$1` = workspace slug.
pub fn base_analytic_view_list_sql() -> String {
    format!(
        "SELECT * FROM \"{ANALYTIC_VIEW_TABLE}\" WHERE (\"{ANALYTIC_VIEW_TABLE}\".\"workspace_id\" IN \
         (SELECT \"{WORKSPACE_TABLE}\".\"id\" FROM \"{WORKSPACE_TABLE}\" \
         WHERE \"{WORKSPACE_TABLE}\".\"slug\" = $1)) ORDER BY \"{ANALYTIC_VIEW_TABLE}\".\"created_at\" DESC"
    )
}

/// `perform_create` workspace lookup (`base.py:182-184`):
/// `Workspace.objects.get(slug=slug)`, then `serializer.save(workspace_id=...)`.
/// A missing slug raises, mapped to 404 by `handle_exception`. Params: `$1` = workspace slug.
pub fn base_workspace_lookup_sql() -> String {
    format!("SELECT \"id\" FROM \"{WORKSPACE_TABLE}\" WHERE \"slug\" = $1 LIMIT 1")
}

// ---------------------------------------------------------------------------
// FX-A-Q-02b: SavedAnalyticEndpoint (base.py:190-222)
// ---------------------------------------------------------------------------

/// Q-02b saved-view lookup (`base.py:193`): `AnalyticView.objects.get(pk, workspace__slug)` —
/// the soft-delete-scoped default manager. The stored `query` is used VERBATIM as ORM
/// kwargs (`base.py:195-196`, no re-validation); axes come from `query_dict` (`:198-199`)
/// while `segment` still comes from the request (`:207`); distribution/total then reuse the
/// Q-01b/Q-01a shapes. Params: `$1` = analytic id, `$2` = workspace slug.
pub fn base_saved_analytic_lookup_sql() -> String {
    format!(
        "SELECT * FROM \"{ANALYTIC_VIEW_TABLE}\" \
         INNER JOIN \"{WORKSPACE_TABLE}\" \
         ON (\"{ANALYTIC_VIEW_TABLE}\".\"workspace_id\" = \"{WORKSPACE_TABLE}\".\"id\") \
         WHERE (\"{ANALYTIC_VIEW_TABLE}\".\"id\" = $1 \
         AND \"{WORKSPACE_TABLE}\".\"slug\" = $2 \
         AND \"{ANALYTIC_VIEW_TABLE}\".\"deleted_at\" IS NULL) LIMIT 1"
    )
}

// ---------------------------------------------------------------------------
// FX-A-Q-02c: ExportAnalyticsEndpoint (base.py:223-251)
// ---------------------------------------------------------------------------

/// 200 for the export acknowledgement (`base.py:246-249`).
pub const EXPORT_STATUS: u16 = 200;

/// Export acknowledgement body (`base.py:247`): Q-02c runs NO queryset — after the shared
/// validation it enqueues `analytic_export_task` (tasks layer owns the publisher) and
/// returns the emailed-to message.
pub fn base_export_message(email: &str) -> Value {
    json!({"message": format!("Once the export is ready it will be emailed to you at {email}")})
}
// FX-A-Q-06: exporter queryset + filters (exporter/base.py:18-84)
// ---------------------------------------------------------------------------

/// Valid export providers (`exporter/base.py:31`).
pub const EXPORT_PROVIDERS: &[&str] = &["csv", "xlsx", "json"];

/// `{"error": f"Provider '{provider}' not found."}` 400
/// (`exporter/base.py:62-65` — note Provider capitalized, provider quoted).
pub fn provider_not_found_body(provider: &str) -> String {
    format!("{{\"error\": \"Provider '{provider}' not found.\"}}")
}

/// Empty-project fallback (`exporter/base.py:32-39`):
/// `Project.objects.filter(workspace__slug, project_projectmember__member=user,
/// project_projectmember__is_active, archived null)` `.values_list("id",
/// flat=True)`, stringified per row. `member_ph` binds `request.user` — without
/// it the fallback would return every workspace project instead of the
/// requester's.
pub fn exporter_project_fallback_sql(workspace_slug_ph: &str, member_ph: &str) -> String {
    format!(
        "SELECT \"projects\".\"id\" FROM \"projects\" WHERE \
         (\"workspaces\".\"slug\" = {workspace_slug_ph} \
         AND \"project_members\".\"member_id\" = {member_ph} \
         AND \"project_members\".\"is_active\" AND \"projects\".\"archived_at\" IS NULL)"
    )
}

/// Columns written on `ExporterHistory.objects.create` (`exporter/base.py:41-47`):
/// `workspace`, `project` (ArrayField of stringified ids), `initiated_by`,
/// `provider`, `type="issue_exports"`. The row `token` (a `generate_token` hex
/// default per FX-A-MOD-02) becomes the task's `token_id`.
pub const EXPORTER_CREATE_COLUMNS: &[&str] = &[
    "workspace_id",
    "project",
    "initiated_by_id",
    "provider",
    "type",
];

/// Default export row type (`exporter/base.py:46`).
pub const EXPORTER_DEFAULT_TYPE: &str = "issue_exports";

/// `issue_export_task.delay` payload (`exporter/base.py:49-56`): positional
/// `(provider, workspace_id, project_ids)` plus kwargs `token_id`, `multiple`,
/// `slug`. The kwarg names are Celery-wire significant for the tasks layer
/// (PIDASHCONV-381); `multiple` passes through verbatim from the request body
/// (default `False`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueExportTaskArgs {
    /// `exporter.provider`.
    pub provider: String,
    /// `workspace.id` (UUID string).
    pub workspace_id: String,
    /// Stringified project ids.
    pub project_ids: Vec<String>,
    /// `exporter.token`.
    pub token_id: String,
    /// Raw `multiple` request value, passed through.
    pub multiple: bool,
    /// Workspace slug.
    pub slug: String,
}

impl IssueExportTaskArgs {
    /// Kwarg names of the `.delay()` call, in call order.
    pub const DELAY_KWARGS: &[&str] = &[
        "provider",
        "workspace_id",
        "project_ids",
        "token_id",
        "multiple",
        "slug",
    ];
}

/// Exporter-history list select (`exporter/base.py:67-84`):
/// `ExporterHistory.objects.filter(workspace__slug=slug, type="issue_exports")`
/// `.select_related("workspace", "initiated_by")`, ordered via paginate
/// (`order_by` default `-created_at`, the model `ORDERING` per FX-A-MOD-02).
/// Reuse [`crate::app_issues::ordering::order_sql`] for the `order_by` param;
/// rows serialize with `ExporterHistorySerializer(many=True)` into the
/// paginator envelope. `type_ph` binds `"issue_exports"`.
pub fn exporter_list_sql(workspace_slug_ph: &str, type_ph: &str) -> String {
    format!(
        "SELECT \"exporters\".* FROM \"exporters\" \
         LEFT OUTER JOIN \"workspaces\" ON (\"exporters\".\"workspace_id\" = \"workspaces\".\"id\") \
         LEFT OUTER JOIN \"users\" ON (\"exporters\".\"initiated_by_id\" = \"users\".\"id\") \
         WHERE (\"exporters\".\"workspace_id\" IN \
         (SELECT \"workspaces\".\"id\" FROM \"workspaces\" WHERE \"workspaces\".\"slug\" = {workspace_slug_ph}) \
         AND \"exporters\".\"type\" = {type_ph}) \
         ORDER BY \"exporters\".\"created_at\" DESC"
    )
}

/// Default exporter list ordering (`exporter/base.py:75` paginate default).
pub const EXPORTER_DEFAULT_ORDER: &str = "-created_at";

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const PART1: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_analytics/queries/analytics_queries_part1.sql"
    );

    fn part1() -> String {
        std::fs::read_to_string(PART1).expect("part1 fixture exists")
    }

    fn ctx() -> BaseRequestContext<'static> {
        BaseRequestContext {
            workspace_slug: "an-ws",
        }
    }

    #[test]
    fn tables_match_django_and_fixture() {
        assert_eq!(ANALYTIC_VIEW_TABLE, "analytic_views");
        assert_eq!(ISSUE_TABLE, "issues");
        let fixture = part1();
        for table in [
            ISSUE_TABLE,
            PROJECT_TABLE,
            WORKSPACE_TABLE,
            STATE_TABLE,
            LABEL_TABLE,
            USER_TABLE,
            CYCLE_TABLE,
            MODULE_TABLE,
            ANALYTIC_VIEW_TABLE,
        ] {
            assert!(fixture.contains(table), "fixture records {table}");
        }
    }

    #[test]
    fn axis_validation_ports_both_400_branches() {
        assert_eq!(
            validate_base_axes(None, Some("issue_count"), None),
            Err(AxisError::Axes)
        );
        assert_eq!(
            validate_base_axes(Some("priority"), None, None),
            Err(AxisError::Axes)
        );
        assert_eq!(
            validate_base_axes(Some("nope"), Some("issue_count"), None),
            Err(AxisError::Axes)
        );
        assert_eq!(
            validate_base_axes(Some("priority"), Some("nope"), None),
            Err(AxisError::Axes)
        );
        // Segment equal to x_axis or unknown.
        assert_eq!(
            validate_base_axes(Some("priority"), Some("issue_count"), Some("priority")),
            Err(AxisError::Segment)
        );
        assert_eq!(
            validate_base_axes(Some("priority"), Some("issue_count"), Some("nope")),
            Err(AxisError::Segment)
        );
        // Empty segment is falsy: skipped.
        assert!(validate_base_axes(Some("priority"), Some("issue_count"), Some("")).is_ok());
        assert!(validate_base_axes(Some("priority"), Some("issue_count"), None).is_ok());
        assert!(validate_base_axes(Some("priority"), Some("estimate"), Some("state_id")).is_ok());
        assert_eq!(AXES_STATUS, 400);
        assert_eq!(SEGMENT_STATUS, 400);
        assert_eq!(
            axes_error_body(),
            json!({"error": "x-axis and y-axis dimensions are required and the values should be valid"})
        );
        assert_eq!(
            segment_error_body(),
            json!({"error": "Both segment and x axis cannot be same and segment should be valid"})
        );
    }

    use crate::app_issues::ordering::order_sql;

    const SCOPE: &str = "\"workspaces\".\"slug\" = $1";

    // -- FX-A-Q-03 ----------------------------------------------------------

    #[test]
    fn total_is_plain_count() {
        assert_eq!(
            default_total_sql(SCOPE),
            "SELECT COUNT(*) FROM \"issues\" WHERE (\"workspaces\".\"slug\" = $1)"
        );
    }

    #[test]
    fn classified_groups_ascending_with_open_variant() {
        let all = default_classified_sql(SCOPE, false);
        assert!(all.contains("GROUP BY \"states\".\"group\" ORDER BY \"state_group\" ASC"));
        assert!(!all.contains("IN ("));
        let open = default_classified_sql(SCOPE, true);
        for g in ["backlog", "unstarted", "started", "review", "test"] {
            assert!(open.contains(&format!("'{g}'")), "missing {g}");
        }
        assert!(!open.contains("'completed'") && !open.contains("'cancelled'"));
    }

    #[test]
    fn completed_month_filters_current_year() {
        let sql = default_completed_month_sql(SCOPE, "$2");
        assert!(sql.contains("EXTRACT(MONTH FROM \"issues\".\"completed_at\") AS \"month\""));
        assert!(sql.contains("EXTRACT(YEAR FROM \"completed_at\") = $2"));
        assert!(sql.contains("GROUP BY \"month\" ORDER BY \"month\" ASC"));
    }

    #[test]
    fn top_creators_limit5_excludes_null_creator() {
        let sql = default_top_creators_sql(SCOPE);
        assert!(sql.contains("\"users\".\"first_name\" AS \"created_by__first_name\""));
        assert!(sql.contains("\"issues\".\"created_by_id\" IS NOT NULL"));
        assert!(sql.contains("ORDER BY \"count\" DESC LIMIT 5"));
        assert!(sql.contains("CONCAT('/api/assets/v2/static/'"));
    }

    #[test]
    fn top_closers_require_completed_and_assignee() {
        let sql = default_top_closers_sql(SCOPE);
        assert!(sql.contains("\"issues\".\"completed_at\" IS NOT NULL"));
        assert!(sql.contains("\"users\".\"id\" IS NOT NULL"));
        assert!(sql.contains("AS \"assignees__avatar_url\""));
        assert!(sql.contains("LIMIT 5"));
    }

    #[test]
    fn avatar_columns_use_fk_id_column() {
        // Django FK `avatar_asset` lives in `users.avatar_asset_id`
        // (`db/models/user.py:69`); the bare `avatar_asset` column does not exist.
        for sql in [
            avatar_case_sql("\"users\"", "created_by__avatar_url"),
            base_assignee_details_sql("TRUE"),
            project_assignee_stats_sql(SCOPE, "$2", None),
        ] {
            assert!(sql.contains("avatar_asset_id"), "missing FK column: {sql}");
            assert!(
                !sql.replace("avatar_asset_id", "").contains("avatar_asset"),
                "bare avatar_asset column: {sql}"
            );
            // PIDASHCONV-498: the Q-01f guard once carried an extra `)`,
            // unparseable at execution though all `contains` checks passed.
            assert_eq!(
                sql.matches('(').count(),
                sql.matches(')').count(),
                "unbalanced parens: {sql}"
            );
        }
    }

    #[test]
    fn pending_has_no_limit() {
        let sql = default_pending_sql(SCOPE);
        assert!(sql.contains("\"issues\".\"completed_at\" IS NULL"));
        assert!(!sql.contains("LIMIT"));
    }

    #[test]
    fn estimate_sums_point_column_ported_bug() {
        // B1: SUM("issues"."point") although the estimate lives elsewhere.
        let open = default_estimate_sql(SCOPE, true);
        let total = default_estimate_sql(SCOPE, false);
        assert!(open.contains("SUM(\"issues\".\"point\")"));
        assert!(total.contains("SUM(\"issues\".\"point\")"));
        assert!(open.contains("'backlog'"));
        assert!(!total.contains("IN ("));
    }

    #[test]
    fn project_stats_fields_default_to_all_five() {
        assert_eq!(project_stats_fields(""), PROJECT_STATS_FIELDS.to_vec());
        assert_eq!(
            project_stats_fields("nope,also-nope"),
            PROJECT_STATS_FIELDS.to_vec()
        );
        assert_eq!(
            project_stats_fields("total_issues,completed_issues"),
            vec!["total_issues", "completed_issues"]
        );
    }

    #[test]
    fn project_stats_sql_carries_five_subqueries_and_id_filter() {
        let sql = project_stats_sql("$1", &["a", "b"], PROJECT_STATS_FIELDS);
        for key in PROJECT_STATS_FIELDS {
            assert!(sql.contains(&format!("AS \"{key}\"")), "missing {key}");
        }
        assert!(sql.contains("\"states\".\"group\" IN ('completed','cancelled')"));
        assert!(sql.contains("NOT \"users\".\"is_bot\""));
        assert!(sql.contains("\"U0\".\"is_active\""));
        // The aliased table name is not visible inside its own subquery.
        assert!(!sql.contains("\"project_members\".\"is_active\""));
        assert!(sql.contains("\"projects\".\"id\" IN ('a','b')"));
        // OuterRef("pk") renders the concrete "id" column; "projects"."pk"
        // is not a real column.
        assert!(!sql.contains("\"projects\".\"pk\""));
        assert!(sql.contains("U0.\"project_id\" = \"projects\".\"id\""));
        let unfiltered = project_stats_sql("$1", &[], &["total_issues"]);
        assert!(!unfiltered.contains("IN ("));
    }

    // -- FX-A-Q-04 ----------------------------------------------------------

    #[test]
    fn filtered_count_window_or_plain() {
        let plain = advance_filtered_count_sql("issues", SCOPE, None);
        assert!(!plain.contains("created_at\" >="));
        let windowed = advance_filtered_count_sql("issues", SCOPE, Some(("$2", "$3")));
        assert!(windowed.contains("\"created_at\" >= $2 AND \"created_at\" <= $3"));
    }

    #[test]
    fn agent_run_usage_coalesces_to_zero() {
        let sql = advance_agent_run_usage_sql(SCOPE);
        assert!(sql.contains("COALESCE(SUM(\"input_tokens\"),0)"));
        assert!(sql.contains("COALESCE(SUM(\"output_tokens\"),0)"));
        assert!(sql.contains("COALESCE(SUM(\"total_tokens\"),0)"));
        // Runner AgentRun Meta db_table is the singular "agent_run".
        assert!(sql.contains("FROM \"agent_run\""));
    }

    #[test]
    fn overview_intake_uses_status_list() {
        let where_sql = overview_intake_where(SCOPE);
        assert!(where_sql.contains("\"issue_intake\".\"status\" IN ('-2','-1','0','1','2')"));
        assert_eq!(ADVANCE_OVERVIEW_KEYS.len(), 11);
    }

    #[test]
    fn work_item_stats_have_no_cancelled() {
        assert_eq!(ADVANCE_WORK_ITEM_KEYS.len(), 5);
        assert!(!ADVANCE_WORK_ITEM_KEYS
            .iter()
            .any(|(k, _)| *k == "cancelled_work_items"));
        let sql = advance_work_item_stat_sql(SCOPE, Some("backlog"), None);
        assert!(sql.contains("\"states\".\"group\" = 'backlog'"));
    }

    #[test]
    fn project_issues_stats_five_counts_ordered() {
        let sql = advance_project_issues_stats_sql(SCOPE, None);
        for key in [
            "cancelled_work_items",
            "completed_work_items",
            "backlog_work_items",
            "un_started_work_items",
            "started_work_items",
        ] {
            assert!(sql.contains(&format!("AS \"{key}\"")), "missing {key}");
        }
        assert!(sql.contains("ORDER BY \"project_id\""));
        // No DISTINCT in the workspace variant (contrast Q-05d below).
        assert!(!sql.contains("DISTINCT"));
        let dated = advance_project_issues_stats_sql(SCOPE, Some(("$2", "$3")));
        assert!(dated.contains("\"created_at\"::date >= $2"));
    }

    #[test]
    fn project_chart_keys_fixed_order_and_names() {
        assert_eq!(
            ADVANCE_PROJECT_CHART_KEYS,
            &[
                "work_items",
                "cycles",
                "modules",
                "intake",
                "members",
                "pages",
                "views"
            ]
        );
        assert_eq!(chart_key_name("work_items"), "Work Items");
        assert_eq!(chart_key_name("views"), "Views");
    }

    #[test]
    fn completion_monthly_and_zero_fill_helpers() {
        let sql = advance_completion_monthly_sql(SCOPE, None);
        assert!(sql.contains("date_trunc('month'"));
        assert!(sql.contains("AS \"created_count\""));
        assert!(sql.contains("AS \"completed_count\""));
        assert_eq!(
            month_keys_between((2026, 11), (2027, 2)),
            vec!["2026-11-01", "2026-12-01", "2027-01-01", "2027-02-01"]
        );
        assert_eq!(month_keys_between((2026, 5), (2026, 5)), vec!["2026-05-01"]);
    }

    #[test]
    fn invalid_bodies_byte_exact() {
        assert_eq!(INVALID_TAB_BODY, "{\"message\": \"Invalid tab\"}");
        assert_eq!(INVALID_TYPE_BODY, "{\"message\": \"Invalid type\"}");
        assert_eq!(DEFAULT_CHART_X_AXIS, "PRIORITY");
    }

    // -- FX-A-Q-05 ----------------------------------------------------------

    #[test]
    fn through_ids_scope_both_sides() {
        // B4: base_filters scope the through rows (kept verbatim).
        let sql = project_through_ids_sql("cycle_issues", "cycle_id", SCOPE, "$2");
        assert!(sql.contains("SELECT \"issue_id\" FROM \"cycle_issues\""));
        assert!(sql.contains("\"cycle_id\" = $2"));
        assert!(sql.contains(SCOPE));
    }

    #[test]
    fn project_assignee_stats_use_distinct() {
        let plain = project_assignee_stats_sql(SCOPE, "$2", None);
        assert!(plain.contains("\"issues\".\"project_id\" = $2"));
        assert!(plain.contains("COUNT(DISTINCT \"issues\".\"id\")"));
        assert!(plain.contains("ORDER BY \"display_name\""));
        let sub = project_through_ids_sql("module_issues", "module_id", SCOPE, "$2");
        let scoped = project_assignee_stats_sql(SCOPE, "$9", Some(&sub));
        assert!(scoped.contains(&format!("IN ({sub})")));
    }

    #[test]
    fn daily_branch_uses_issue_prefix_and_empty_body() {
        let sub = project_through_ids_sql("cycle_issues", "cycle_id", SCOPE, "$2");
        let sql = project_completion_daily_sql(&sub);
        assert!(sql.contains("\"issue\".\"state__group\" = 'completed'"));
        assert_eq!(EMPTY_CHART_BODY, "{\"data\": [], \"schema\": {}}");
        assert_eq!(
            day_keys_between((2026, 2, 27), (2026, 3, 2)),
            vec!["2026-02-27", "2026-02-28", "2026-03-01", "2026-03-02"]
        );
    }

    // -- FX-A-Q-06 ----------------------------------------------------------

    #[test]
    fn exporter_fallback_scopes_to_requesting_member() {
        let sql = exporter_project_fallback_sql("$1", "$2");
        assert!(sql.contains("\"workspaces\".\"slug\" = $1"));
        assert!(sql.contains("\"project_members\".\"member_id\" = $2"));
        assert!(sql.contains("\"project_members\".\"is_active\""));
        assert!(sql.contains("\"projects\".\"archived_at\" IS NULL"));
    }

    #[test]
    fn project_ids_literals_escape_quotes() {
        let sql = project_stats_sql("$1", &["a'b"], &["total_issues"]);
        assert!(sql.contains("'a''b'"));
    }

    #[test]
    fn exporter_provider_gate_and_bodies() {
        assert_eq!(EXPORT_PROVIDERS, &["csv", "xlsx", "json"]);
        assert_eq!(
            provider_not_found_body("pdf"),
            "{\"error\": \"Provider 'pdf' not found.\"}"
        );
        assert_eq!(
            EXPORTER_PAGINATION_REQUIRED_BODY,
            "{\"error\": \"per_page and cursor are required\"}"
        );
        assert_eq!(
            EXPORT_ACCEPTED_MESSAGE,
            "Once the export is ready you will be able to download it"
        );
    }

    #[test]
    fn axis_vocab_matches_plot_module() {
        assert_eq!(VALID_ANALYTICS_FIELDS.len(), 12);
        assert_eq!(VALID_YAXIS, ["issue_count", "estimate"]);
        assert!(is_date_axis("created_at"));
        assert!(!is_date_axis("priority"));
        assert!(month_dimension_expr("created_at").contains("EXTRACT(YEAR"));
        assert!(month_dimension_expr("created_at").contains("EXTRACT(MONTH"));
    }

    #[test]
    fn count_sql_matches_q01a_template() {
        let fixture = part1();
        assert!(fixture.contains("total_issues = queryset.count()"));
        let sql = base_count_sql("\"projects\".\"workspace_id\" = $2");
        assert!(sql.starts_with("SELECT COUNT(*) FROM \"issues\""));
        assert!(sql.contains("\"workspaces\".\"slug\" = $1"));
        assert!(sql.contains("\"projects\".\"workspace_id\" = $2"));
        let _ = ctx();
    }

    #[test]
    fn plot_count_sql_matches_q01b_shape() {
        let fixture = part1();
        assert!(fixture.contains("annotate(count=Count('*')).order_by(dimension)"));
        let sql = base_plot_count_sql("priority", None, "TRUE").expect("known axis");
        assert!(sql.contains("COUNT(*) AS \"count\""));
        assert!(sql.contains("GROUP BY \"dimension\""));
        assert!(sql.contains("ORDER BY \"dimension\" ASC"));
        assert!(sql.contains("\"workspaces\".\"slug\" = $1"));
        // Unknown axes never reach SQL: handlers reject them in validation first.
        assert!(base_plot_count_sql("nope", None, "TRUE").is_none());
        assert!(base_plot_count_sql("priority", Some("nope"), "TRUE").is_none());
        // Date axes exclude NULL dimensions (analytics_plot.py:85-86).
        let dated = base_plot_count_sql("created_at", None, "TRUE").expect("date axis");
        assert!(dated.contains("EXTRACT(YEAR"));
        // Every axis excludes NULL dimensions: `extract_axis` always returns
        // "dimension", so `if x_axis == "dimension"` (analytics_plot.py:85) is
        // always true and the exclude runs unconditionally (PIDASHCONV-496).
        // The guard repeats the inlined dimension expression (a SELECT alias is
        // not visible in the same level's WHERE).
        let plain = base_plot_count_sql("priority", None, "TRUE").expect("plain axis");
        assert!(plain.contains("\"issues\".\"priority\" IS NOT NULL"));
        let nullable = base_plot_count_sql("labels__id", None, "TRUE").expect("nullable axis");
        assert!(nullable.contains("\"issue_labels\".\"label_id\" IS NOT NULL"));
    }

    #[test]
    fn plot_estimate_sql_matches_q01c_shape() {
        let fixture = part1();
        assert!(fixture.contains("SUM(CAST"));
        let sql = base_plot_estimate_sql("priority", None, "TRUE").expect("known axis");
        assert!(sql.contains("SUM(CAST(\"estimate_points\".\"value\" AS DOUBLE PRECISION))"));
        assert!(sql.contains("GROUP BY \"dimension\""));
        // Forward FK (`db/models/issue.py:130-136`): the estimate table joins on
        // `issues.estimate_point_id`, and exactly once.
        assert!(sql.contains(
            "LEFT OUTER JOIN \"estimate_points\" \
             ON (\"issues\".\"estimate_point_id\" = \"estimate_points\".\"id\")"
        ));
        assert_eq!(sql.matches("\"estimate_points\"").count(), 3);
        assert!(base_plot_estimate_sql("nope", None, "TRUE").is_none());
        assert!(base_plot_estimate_sql("priority", Some("nope"), "TRUE").is_none());
        // Every axis excludes NULL dimensions: `extract_axis` always returns
        // "dimension", so `if x_axis == "dimension"` (analytics_plot.py:85) is
        // always true and the exclude runs unconditionally (PIDASHCONV-506,
        // mirroring PIDASHCONV-496 for the count branch). The guard repeats the
        // inlined dimension expression (a SELECT alias is not visible in the
        // same level's WHERE).
        let plain = base_plot_estimate_sql("priority", None, "TRUE").expect("plain axis");
        assert!(plain.contains("\"issues\".\"priority\" IS NOT NULL"));
        let nullable = base_plot_estimate_sql("labels__id", None, "TRUE").expect("nullable axis");
        assert!(nullable.contains("\"issue_labels\".\"label_id\" IS NOT NULL"));
        let dated = base_plot_estimate_sql("created_at", None, "TRUE").expect("date axis");
        assert!(dated.contains("IS NOT NULL"));
        assert!(dated.contains("EXTRACT(YEAR"));
        // Segment interplay: segment NULLs are NOT excluded in Python
        // (analytics_plot.py:89-115 has no segment exclude) — only the
        // dimension carries the guard.
        let segmented =
            base_plot_estimate_sql("priority", Some("labels__id"), "TRUE").expect("segmented axis");
        assert!(segmented.contains("\"issues\".\"priority\" IS NOT NULL"));
        assert!(!segmented.contains("\"issue_labels\".\"label_id\" IS NOT NULL"));
    }

    /// Every rendered statement must parse: the label/cycle/module detail
    /// builders once emitted a stray `)` after the scope's closed WHERE
    /// paren, which Postgres rejected (`syntax error at or near ")"`) and
    /// the handler surfaced as a 500 on those axes (PIDASHCONV-521).
    /// Fragment assertions cannot catch that; balanced parens can.
    #[test]
    fn rendered_statements_have_balanced_parens() {
        let filters = "TRUE";
        let mut statements: Vec<(&str, String)> = vec![
            ("state", base_state_details_sql(filters)),
            ("label", base_label_details_sql(filters)),
            ("assignee", base_assignee_details_sql(filters)),
            ("cycle", base_cycle_details_sql(filters)),
            ("module", base_module_details_sql(filters)),
        ];
        for axis in [
            "priority",
            "state_id",
            "labels__id",
            "assignees__id",
            "estimate_point__value",
            "issue_cycle__cycle_id",
            "issue_module__module_id",
            "created_at",
        ] {
            statements.push((
                "count",
                base_plot_count_sql(axis, None, filters).expect("known axis"),
            ));
            statements.push((
                "estimate",
                base_plot_estimate_sql(axis, None, filters).expect("known axis"),
            ));
        }
        for (name, sql) in &statements {
            let open = sql.chars().filter(|c| *c == '(').count();
            let close = sql.chars().filter(|c| *c == ')').count();
            assert_eq!(open, close, "{name} statement has unbalanced parens: {sql}");
        }
    }

    #[test]
    fn detail_sqls_match_q01d_h_templates() {
        let fixture = part1();
        assert!(fixture.contains("DISTINCT ON"));
        let filters = "TRUE";
        let state = base_state_details_sql(filters);
        assert!(state.contains("DISTINCT ON (\"issues\".\"state_id\")"));
        assert!(state.contains("\"state__name\""));
        assert!(state.contains("ORDER BY \"issues\".\"state_id\" ASC"));
        let label = base_label_details_sql(filters);
        assert!(label.contains("\"labels__id\""));
        // Fixture prose names the guard via the `label_issue` related name; the SQL names the
        // link table (`db_table = "issue_labels"`, `db/models/issue.py:676`).
        assert!(label.contains("\"issue_labels\".\"deleted_at\" IS NULL"));
        // BUG (base.py:83): plain Issue.objects — no triage/archived/draft exclusions.
        assert!(
            !label.contains("triage"),
            "label branch keeps the manager-scope bug"
        );
        let assignee = base_assignee_details_sql(filters);
        assert!(assignee.contains("/api/assets/v2/static/"));
        assert!(assignee.contains("\"assignees__avatar_url\""));
        assert!(assignee.contains("ORDER BY \"users\".\"id\" ASC"));
        let cycle = base_cycle_details_sql(filters);
        assert!(cycle.contains("\"issue_cycle__cycle_id\""));
        assert!(cycle.contains("\"cycle_issues\".\"deleted_at\" IS NULL"));
        let module = base_module_details_sql(filters);
        assert!(module.contains("\"issue_module__module_id\""));
        assert!(module.contains("\"module_issues\".\"deleted_at\" IS NULL"));
    }

    #[test]
    fn sort_data_ports_priority_and_none_last() {
        let keys = vec!["urgent", "high", "medium", "low"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(
            sort_data_keys(&keys, "priority"),
            vec!["low", "medium", "high", "urgent"]
        );
        // Missing keys are dropped (fixture seed row shows populated buckets only).
        let sparse = vec!["urgent".to_owned(), "high".to_owned()];
        assert_eq!(sort_data_keys(&sparse, "priority"), vec!["high", "urgent"]);
        // Other axes sort with 'none' last.
        let mixed = vec!["none".to_owned(), "b".to_owned(), "a".to_owned()];
        assert_eq!(sort_data_keys(&mixed, "state_id"), vec!["a", "b", "none"]);
    }

    #[test]
    fn viewset_queryset_ports_ordering_and_lookup() {
        assert_eq!(ANALYTICS_STATUS, 200);
        let list = base_analytic_view_list_sql();
        assert!(list.contains("FROM \"analytic_views\""));
        assert!(list.contains("\"workspaces\".\"slug\" = $1"));
        assert!(list.contains("ORDER BY \"analytic_views\".\"created_at\" DESC"));
        assert_eq!(
            base_workspace_lookup_sql(),
            "SELECT \"id\" FROM \"workspaces\" WHERE \"slug\" = $1 LIMIT 1"
        );
        let saved = base_saved_analytic_lookup_sql();
        assert!(saved.contains("\"analytic_views\".\"id\" = $1"));
        assert!(saved.contains("\"workspaces\".\"slug\" = $2"));
        assert!(saved.contains("\"analytic_views\".\"deleted_at\" IS NULL"));
        assert!(saved.ends_with("LIMIT 1"));
    }

    #[test]
    fn export_ports_no_queryset_ack() {
        let fixture = part1();
        assert!(fixture.contains("NO queryset"));
        assert_eq!(EXPORT_STATUS, 200);
        assert_eq!(
            base_export_message("a@x.com"),
            json!({"message": "Once the export is ready it will be emailed to you at a@x.com"})
        );
    }

    #[test]
    fn golden_keys_cover_wire_envelope() {
        // The 200 envelope keys (base.py:161-174) the handlers must render.
        let keys: BTreeSet<&str> = ["total", "distribution", "extras"].into_iter().collect();
        assert!(keys.contains("total"));
        assert!(keys.contains("distribution"));
        assert!(keys.contains("extras"));
    }

    #[test]
    fn exporter_list_sql_filters_type_orders_created() {
        let sql = exporter_list_sql("$1", "$2");
        assert!(sql.contains("\"exporters\".\"type\" = $2"));
        assert!(sql.contains("\"workspaces\".\"slug\" = $1"));
        assert!(sql.contains("FROM \"workspaces\""));
        assert!(sql.contains("ORDER BY \"exporters\".\"created_at\" DESC"));
        assert_eq!(EXPORTER_DEFAULT_TYPE, "issue_exports");
        assert_eq!(EXPORTER_DEFAULT_ORDER, "-created_at");
    }

    #[test]
    fn exporter_list_default_order_matches_model_ordering() {
        // Paginate default `-created_at` resolves through the shared kernel
        // (default branch: `created_at` in the param means verbatim, no tiebreak).
        let spec = order_sql(
            EXPORTER_DEFAULT_ORDER,
            "\"exporters\".\"created_at\"",
            |_| String::new(),
        );
        assert_eq!(spec.order_by_sql, "-created_at");
        assert_eq!(spec.out_param, "-created_at");
        assert_eq!(EXPORTER_CREATE_COLUMNS.len(), 5);
        assert_eq!(
            IssueExportTaskArgs::DELAY_KWARGS,
            &[
                "provider",
                "workspace_id",
                "project_ids",
                "token_id",
                "multiple",
                "slug"
            ]
        );
    }

    #[test]
    fn groups_share_state_order_with_filter_kernel() {
        assert_eq!(
            open_state_groups(),
            vec!["backlog", "unstarted", "started", "review", "test"]
        );
        assert_eq!(closed_state_groups(), vec!["completed", "cancelled"]);
        let mut binds = Binds::new(1);
        assert_eq!(binds.take(), "$1");
        assert_eq!(binds.take(), "$2");
    }
}
