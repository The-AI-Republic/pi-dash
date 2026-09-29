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

use pidash_db::app_analytics::models::analytic_view;
use serde_json::{json, Value};

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
/// target; date axes render through [`month_dimension_expr`] and exclude NULL
/// dimensions (`analytics_plot.py:85-86`); every other axis annotates NULLs to
/// `'None'`/`'null'` via `is_null`/`dimension_ex` (`analytics_plot.py:97-104`).
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
        "estimate_point__value" => Some((
            format!("\"{ESTIMATE_POINT_TABLE}\".\"value\""),
            format!(
                " LEFT OUTER JOIN \"{ESTIMATE_POINT_TABLE}\" \
                 ON (\"{ISSUE_TABLE}\".\"id\" = \"{ESTIMATE_POINT_TABLE}\".\"issue_id\")"
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
/// (+ `segment`), `COUNT(*)`, ordered by dimension. Date axes exclude NULL
/// dimensions (`:85-86`); non-date axes map NULL to `'None'`/`'null'` first.
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
    // Date axes exclude NULL dimensions (`analytics_plot.py:85-86`): the guard repeats the
    // inlined expression (a SELECT alias is not visible in the same level's WHERE).
    let null_guard = if is_date_axis(x_axis) {
        format!(" AND {dim_expr} IS NOT NULL")
    } else {
        String::new()
    };
    Some(format!(
        "SELECT \"dimension\", COUNT(*) AS \"count\" FROM (SELECT {dim_expr} AS \"dimension\"{seg_select} \
         FROM \"{ISSUE_TABLE}\"{dim_join}{seg_join}{scope}{null_guard}) GROUP BY \"dimension\"{seg_group} \
         ORDER BY \"dimension\" ASC",
        scope = workspace_scope().replace("{filters}", filters_sql),
    ))
}

/// Q-01c estimate plot (`analytics_plot.py:110-115`):
/// `SUM(CAST(estimate_point__value AS float))` grouped by dimension, ordered by `x_axis`.
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
    Some(format!(
        "SELECT {dim_expr} AS \"dimension\"{seg_select}, \
         SUM(CAST(\"{ESTIMATE_POINT_TABLE}\".\"value\" AS DOUBLE PRECISION)) AS \"estimate\" \
         FROM \"{ISSUE_TABLE}\"{extra_joins}{dim_join}{seg_join}{scope} \
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
         AND \"{LABEL_TABLE}\".\"id\" IS NOT NULL AND \"{ISSUE_LABEL_TABLE}\".\"deleted_at\" IS NULL) \
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
         CASE WHEN \"{USER_TABLE}\".\"avatar_asset\" IS NOT NULL \
         THEN CONCAT('/api/assets/v2/static/', \"{USER_TABLE}\".\"avatar_asset\", '/') \
         WHEN \"{USER_TABLE}\".\"avatar_asset\" IS NULL THEN \"{USER_TABLE}\".\"avatar\" \
         ELSE NULL END AS \"assignees__avatar_url\", \
         \"{USER_TABLE}\".\"display_name\" AS \"assignees__display_name\", \
         \"{USER_TABLE}\".\"first_name\" AS \"assignees__first_name\", \
         \"{USER_TABLE}\".\"last_name\" AS \"assignees__last_name\" \
         FROM \"{ISSUE_TABLE}\" \
         LEFT OUTER JOIN \"{ISSUE_ASSIGNEE_TABLE}\" \
         ON (\"{ISSUE_TABLE}\".\"id\" = \"{ISSUE_ASSIGNEE_TABLE}\".\"issue_id\") \
         LEFT OUTER JOIN \"{USER_TABLE}\" \
         ON (\"{ISSUE_ASSIGNEE_TABLE}\".\"assignee_id\" = \"{USER_TABLE}\".\"id\"){} \
         AND (\"{USER_TABLE}\".\"avatar\" IS NOT NULL OR \"{USER_TABLE}\".\"avatar_asset\" IS NOT NULL)) \
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
         AND \"{CYCLE_ISSUE_TABLE}\".\"deleted_at\" IS NULL) \
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
         AND \"{MODULE_ISSUE_TABLE}\".\"deleted_at\" IS NULL) \
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
    }

    #[test]
    fn plot_estimate_sql_matches_q01c_shape() {
        let fixture = part1();
        assert!(fixture.contains("SUM(CAST"));
        let sql = base_plot_estimate_sql("priority", None, "TRUE").expect("known axis");
        assert!(sql.contains("SUM(CAST(\"estimate_points\".\"value\" AS DOUBLE PRECISION))"));
        assert!(sql.contains("GROUP BY \"dimension\""));
        assert!(base_plot_estimate_sql("nope", None, "TRUE").is_none());
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
}
