// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics contracts: the workspace analytics screen's headline totals,
// charts, state breakdowns and the active-projects stats read. The project-
// scoped variants of the first three serve the analytics dialog.
//
// Every read below is workspace-scoped and all-time: the analytics screen
// never sends `date_filter`. The one exception is the estimates builder
// shorthand (`AnalyticsChartBuilderQuery`, EST-020), which forwards the
// server-side window the estimates area needs. The screen treats a failed
// read as an empty section (zeroed cards, no plot, no rows) rather than an
// error banner, so callers catch `ApiError` per read.
//
// Response shapes come from the Django views (apps/api/pi_dash/app/views/
// analytic/{advance,project_analytics,base}.py, utils/build_chart.py). Each
// object records what the screen needs versus what the server returns, and
// the optionals it must tolerate.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

/** `tab` values the totals read accepts; anything else answers 400. */
export const ANALYTICS_TABS = ["overview", "work-items"] as const;
export type AnalyticsTab = (typeof ANALYTICS_TABS)[number];

/**
 * One headline metric. The server returns `{count}` and nothing else — no
 * previous-period figure. The screen renders a missing metric, or one whose
 * `count` is missing/non-numeric, as `0`; `count` stays optional so a read
 * with an absent metric still parses. `count` can be null in the agent-token
 * aggregates when the sum has no rows.
 */
export const AnalyticsMetric = z.object({
  count: z.optional(z.nullable(z.number())),
});
export type AnalyticsMetric = z.infer<typeof AnalyticsMetric>;

/**
 * Overview tab totals. One read drives the eleven overview cards, in this
 * order: users, admins, members, guests, projects, work items, cycles,
 * intake, and the three agent-token sums. Every key is optional: the counts
 * endpoint omits nothing when it answers 200, but the screen must still show
 * `0` for an absent key (an older payload, or a per-project member read the
 * server resolves to zero rows). Keys beyond the eleven pass through.
 */
export const AnalyticsOverviewTotals = z.looseObject({
  total_users: z.optional(AnalyticsMetric),
  total_admins: z.optional(AnalyticsMetric),
  total_members: z.optional(AnalyticsMetric),
  total_guests: z.optional(AnalyticsMetric),
  total_projects: z.optional(AnalyticsMetric),
  total_work_items: z.optional(AnalyticsMetric),
  total_cycles: z.optional(AnalyticsMetric),
  total_intake: z.optional(AnalyticsMetric),
  agent_run_input_tokens: z.optional(AnalyticsMetric),
  agent_run_output_tokens: z.optional(AnalyticsMetric),
  agent_run_total_tokens: z.optional(AnalyticsMetric),
});
export type AnalyticsOverviewTotals = z.infer<typeof AnalyticsOverviewTotals>;

/**
 * Work-items tab totals: five cards, `total`, `started`, `backlog`,
 * `unstarted` (the server's `un_started_work_items` spelling) and
 * `completed`. Every key is optional for the same reason as the overview
 * map — an absent metric reads as `0`.
 */
export const AnalyticsWorkItemsTotals = z.looseObject({
  total_work_items: z.optional(AnalyticsMetric),
  started_work_items: z.optional(AnalyticsMetric),
  backlog_work_items: z.optional(AnalyticsMetric),
  un_started_work_items: z.optional(AnalyticsMetric),
  completed_work_items: z.optional(AnalyticsMetric),
});
export type AnalyticsWorkItemsTotals = z.infer<typeof AnalyticsWorkItemsTotals>;

export type AnalyticsTotals = AnalyticsOverviewTotals | AnalyticsWorkItemsTotals;

/**
 * Query shared by every workspace analytics read. `project_ids` is the
 * picker's scope: omit it for "all projects", otherwise a comma-separated
 * list. Only the project-stats read is documented to ignore it in the old
 * UI, but the screen scopes every section, so every call forwards it.
 */
export interface AnalyticsScope {
  projectIds?: readonly string[];
}

/**
 * Scope for the project-scoped dialog reads. The dialog passes the project
 * id in the path and additionally echoes it as `project_ids`, and may narrow
 * to a cycle or a module. When both `cycleId` and `moduleId` are present the
 * totals and stats reads prefer the cycle; the chart reads prefer the cycle
 * too. The screen sends at most one.
 */
export interface AnalyticsProjectScope extends AnalyticsScope {
  cycleId?: string;
  moduleId?: string;
}

type Query = Record<string, string | number | boolean | undefined>;

function scopedQuery(scope: AnalyticsScope): Query {
  return scope.projectIds && scope.projectIds.length > 0 ? { project_ids: scope.projectIds.join(",") } : {};
}

function projectQuery(scope: AnalyticsProjectScope): Query {
  return {
    ...scopedQuery(scope),
    cycle_id: scope.cycleId,
    module_id: scope.moduleId,
  };
}

/**
 * `tab` picks the totals map. Only the two enum values answer 200; any other
 * tab is a 400 `{message: "Invalid tab"}`. Leave `tab` unset for the server
 * default (`overview`).
 */
export interface AnalyticsTotalsQuery extends AnalyticsScope {
  tab?: AnalyticsTab;
}

export function getAnalyticsTotals(
  client: ApiClient,
  slug: string,
  query: AnalyticsTotalsQuery & { tab: "work-items" }
): Promise<AnalyticsWorkItemsTotals>;
export function getAnalyticsTotals(
  client: ApiClient,
  slug: string,
  query?: AnalyticsTotalsQuery
): Promise<AnalyticsOverviewTotals>;
export async function getAnalyticsTotals(
  client: ApiClient,
  slug: string,
  query: AnalyticsTotalsQuery = {}
): Promise<AnalyticsTotals> {
  const path = `/api/workspaces/${slug}/advance-analytics/`;
  const body = await client.getJson<unknown>(path, {
    query: { ...scopedQuery(query), tab: query.tab },
  });
  return query.tab === "work-items"
    ? client.parse(AnalyticsWorkItemsTotals, body, `GET ${path}`)
    : client.parse(AnalyticsOverviewTotals, body, `GET ${path}`);
}

export async function getProjectAnalyticsTotals(
  client: ApiClient,
  slug: string,
  projectId: string,
  scope: AnalyticsProjectScope = {}
): Promise<AnalyticsWorkItemsTotals> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/advance-analytics/`;
  const body = await client.getJson<unknown>(path, { query: projectQuery(scope) });
  return client.parse(AnalyticsWorkItemsTotals, body, `GET ${path}`);
}

/**
 * One radar/summary dimension. The projects chart always answers all seven
 * rows (work items, cycles, modules, intake, members, pages, views) in that
 * order, zeroed when the workspace or scope is empty; the screen must keep
 * zero rows rather than filter them. `key` is the request-time identifier,
 * `name` the title-cased display label, `count` the value.
 */
export const AnalyticsProjectsChartRow = z.object({
  key: z.string(),
  name: z.string(),
  count: z.number(),
});
export type AnalyticsProjectsChartRow = z.infer<typeof AnalyticsProjectsChartRow>;

export const AnalyticsProjectsChart = z.array(AnalyticsProjectsChartRow);
export type AnalyticsProjectsChart = z.infer<typeof AnalyticsProjectsChart>;

/**
 * One created-vs-resolved bucket. The workspace read is monthly, from the
 * workspace's creation month through the current month, so a member always
 * sees at least one point. A cycle/module-scoped read is daily over the
 * scope's range. The screen plots `created_issues` and `completed_issues`
 * and labels the point by `name` (`YYYY-MM-DD`); `count` repeats
 * `created_issues` for older consumers.
 */
export const AnalyticsWorkItemsChartPoint = z.object({
  key: z.string(),
  name: z.string(),
  count: z.number(),
  completed_issues: z.number(),
  created_issues: z.number(),
});
export type AnalyticsWorkItemsChartPoint = z.infer<typeof AnalyticsWorkItemsChartPoint>;

/**
 * A scope with no usable start date (an undated cycle or module) answers an
 * empty `data` list and an empty `schema` map; the screen shows its guidance
 * panel in that case. A successful response keeps both keys; `schema` maps a
 * series key to its display label (here it repeats the keys).
 */
export const AnalyticsWorkItemsChart = z.object({
  data: z.array(AnalyticsWorkItemsChartPoint),
  schema: z.record(z.string(), z.string()),
});
export type AnalyticsWorkItemsChart = z.infer<typeof AnalyticsWorkItemsChart>;

/**
 * One custom-chart row: the x bucket plus its total. With a split active the
 * row carries one extra numeric key per schema key; the screen stacks them
 * in schema order and sums nothing client-side. Without a split there are no
 * extra keys. `key` is the raw server value (nullable buckets become the
 * literal `"none"`/`"None"`), `name` its display value — both raw here; the
 * screen formats dates and capitalizes labels itself.
 */
export const AnalyticsCustomChartRow = z.looseObject({
  key: z.string(),
  name: z.string(),
  count: z.number(),
});
export type AnalyticsCustomChartRow = z.infer<typeof AnalyticsCustomChartRow>;

/**
 * `schema` maps each split key present in the data to its display label and
 * preserves server order (rows are ordered by descending total). It is empty
 * when no `group_by` was sent or when the grouped value is absent.
 */
export const AnalyticsCustomChart = z.object({
  data: z.array(AnalyticsCustomChartRow),
  schema: z.record(z.string(), z.string()),
});
export type AnalyticsCustomChart = z.infer<typeof AnalyticsCustomChart>;

/**
 * One aggregated chart row. `key` is the grouping key — the estimate
 * position number on the estimate axis, `"None"` when the item carries
 * no value — `name` the displayed label and `count` the items in the
 * row. Grouped answers add one member per inner group on top of these
 * shared members.
 *
 * This is the estimates area's strict row (EST-020): the analytics surface
 * parses the same endpoint with the loose `AnalyticsCustomChartRow`
 * superset, which keeps the stacked segment keys. Both accept the numeric
 * `ESTIMATE_POINTS` key.
 */
export const AnalyticsChartRow = z.object({
  key: z.union([z.string(), z.number()]),
  name: z.string(),
  count: z.number(),
});
export type AnalyticsChartRow = z.infer<typeof AnalyticsChartRow>;

/**
 * Custom chart answer: the aggregated rows plus the inner-group legend,
 * empty when no second grouping was asked for.
 */
export const AnalyticsChartResponse = z.object({
  data: z.array(AnalyticsChartRow),
  schema: z.record(z.string(), z.string()),
});
export type AnalyticsChartResponse = z.infer<typeof AnalyticsChartResponse>;

/**
 * Custom x/split dimensions the backend accepts, in the order the x picker
 * offers them. The pickers send this enum name, never the display label.
 * `TARGET_DATE` is the due date; `CREATED_BY` exists on the server but no
 * shipped picker offers it (neither does the spec).
 */
export const ANALYTICS_X_AXES = [
  "STATES",
  "STATE_GROUPS",
  "PRIORITY",
  "LABELS",
  "ASSIGNEES",
  "ESTIMATE_POINTS",
  "CYCLES",
  "MODULES",
  "COMPLETED_AT",
  "TARGET_DATE",
  "START_DATE",
  "CREATED_AT",
] as const;
export type AnalyticsXAxis = (typeof ANALYTICS_X_AXES)[number];

/** Count is the only y metric; the server defaults to `WORK_ITEM_COUNT`. */
export const ANALYTICS_Y_AXES = ["WORK_ITEM_COUNT"] as const;
export type AnalyticsYAxis = (typeof ANALYTICS_Y_AXES)[number];

/**
 * Grouping axes the chart endpoint accepts, in the server's own names.
 * `ESTIMATE_POINTS` groups by the item's estimate value: the key is the
 * value's position number and the name its stored text. Unknown names
 * are a 400, so callers stay inside this list.
 *
 * This is the estimates area's catalogue (EST-020): unlike the analytics
 * pickers' `ANALYTICS_X_AXES` it keeps the full server list, including
 * `CREATED_BY`, which no analytics picker offers. The builder shorthand
 * below is typed against this list.
 */
export const ANALYTICS_CHART_X_AXES = [
  "STATES",
  "STATE_GROUPS",
  "LABELS",
  "ASSIGNEES",
  "ESTIMATE_POINTS",
  "CYCLES",
  "MODULES",
  "PRIORITY",
  "START_DATE",
  "TARGET_DATE",
  "CREATED_AT",
  "COMPLETED_AT",
  "CREATED_BY",
] as const;
export type AnalyticsChartXAxis = (typeof ANALYTICS_CHART_X_AXES)[number];

/** The estimate grouping axis: one aggregated row per estimate value. */
export const ANALYTICS_CHART_ESTIMATE_AXIS: AnalyticsChartXAxis = "ESTIMATE_POINTS";

/**
 * `type=projects` answers the radar rows; `type=work-items` the series;
 * `type=custom-work-items` the bar data. Any other type is a 400
 * `{message: "Invalid type"}` and the screen treats that as "graph read
 * failed" / "series read failed" (no plot, guidance where the section has
 * one).
 */
export interface AnalyticsProjectsChartQuery extends AnalyticsScope {
  type: "projects";
}

export interface AnalyticsWorkItemsChartQuery extends AnalyticsScope {
  type: "work-items";
}

export interface AnalyticsCustomChartQuery extends AnalyticsScope {
  type: "custom-work-items";
  xAxis: AnalyticsXAxis;
  yAxis?: AnalyticsYAxis;
  /** Omit to render one series; an unknown value answers 400. */
  groupBy?: AnalyticsXAxis;
}

export type AnalyticsChartQuery =
  | AnalyticsProjectsChartQuery
  | AnalyticsWorkItemsChartQuery
  | AnalyticsCustomChartQuery;

/**
 * Estimates builder shorthand (EST-020): the project custom-work-item chart
 * behind the chart builder, grouped by one axis, without an explicit `type`
 * and with the cycle/module scope and the server-side date window inline.
 * An untyped query always reads `type=custom-work-items`; when both the
 * inline scope and the `scope` argument narrow a read, the inline ids win.
 * The unified `getProjectAnalyticsChart` below accepts this shape alongside
 * the typed dialog queries. The workspace read has no shorthand.
 */
export interface AnalyticsChartBuilderQuery {
  /** Grouping axis; the estimate breakdown uses the estimate axis. */
  xAxis: AnalyticsChartXAxis;
  /** Second grouping stacked inside each row; same catalogue. */
  groupBy?: AnalyticsChartXAxis;
  /** Scope the chart to one cycle's items. */
  cycleId?: string;
  /** Scope the chart to one module's items. */
  moduleId?: string;
  /** Server-side date window preset. */
  dateFilter?: string;
}

type ChartResponse<Q extends AnalyticsChartQuery | AnalyticsChartBuilderQuery> = Q extends {
  type: "projects";
}
  ? AnalyticsProjectsChart
  : Q extends { type: "work-items" }
    ? AnalyticsWorkItemsChart
    : AnalyticsCustomChart;

function chartQuery(query: AnalyticsChartQuery | AnalyticsChartBuilderQuery): Query {
  if (!("type" in query)) {
    return {
      type: "custom-work-items",
      x_axis: query.xAxis,
      group_by: query.groupBy,
    };
  }
  const base: Query = { ...scopedQuery(query), type: query.type };
  if (query.type === "custom-work-items") {
    base["x_axis"] = query.xAxis;
    base["y_axis"] = query.yAxis;
    base["group_by"] = query.groupBy;
  }
  return base;
}

function chartSchema(
  query: AnalyticsChartQuery | AnalyticsChartBuilderQuery,
  body: unknown,
  path: string,
  client: ApiClient
) {
  const type = "type" in query ? query.type : "custom-work-items";
  switch (type) {
    case "projects":
      return client.parse(AnalyticsProjectsChart, body, `GET ${path}`);
    case "work-items":
      return client.parse(AnalyticsWorkItemsChart, body, `GET ${path}`);
    case "custom-work-items":
      return client.parse(AnalyticsCustomChart, body, `GET ${path}`);
  }
}

export async function getAnalyticsChart<Q extends AnalyticsChartQuery>(
  client: ApiClient,
  slug: string,
  query: Q
): Promise<ChartResponse<Q>> {
  const path = `/api/workspaces/${slug}/advance-analytics-charts/`;
  const body = await client.getJson<unknown>(path, { query: chartQuery(query) });
  return chartSchema(query, body, path, client) as ChartResponse<Q>;
}

/**
 * GET .../projects/<pid>/advance-analytics-charts/ — the dialog's series and
 * bar data, plus the estimates builder shorthand (see
 * `AnalyticsChartBuilderQuery`). The y measure is always the work-item
 * count; the endpoint takes no y parameter.
 *
 * A shorthand call parses with the loose `AnalyticsCustomChart` superset
 * rather than the strict `AnalyticsChartResponse`: ungrouped answers parse
 * identically, and grouped answers keep their segment keys.
 */
export async function getProjectAnalyticsChart<Q extends AnalyticsChartQuery | AnalyticsChartBuilderQuery>(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: Q,
  scope: AnalyticsProjectScope = {}
): Promise<ChartResponse<Q>> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/advance-analytics-charts/`;
  const shaped: AnalyticsChartQuery | AnalyticsChartBuilderQuery = query;
  const inline =
    "type" in shaped
      ? {}
      : {
          cycle_id: shaped.cycleId ?? scope.cycleId,
          module_id: shaped.moduleId ?? scope.moduleId,
          date_filter: shaped.dateFilter,
        };
  const body = await client.getJson<unknown>(path, {
    query: { ...projectQuery(scope), ...chartQuery(query), ...inline },
  });
  return chartSchema(query, body, path, client) as ChartResponse<Q>;
}

/**
 * One state-breakdown row, per project. Row order is the server's (project
 * id ascending); the screen does not re-sort. `project__name` is the server
 * fallback the CSV export reads when the id no longer resolves. A workspace
 * or scope with no work items answers an empty list, not zero rows.
 */
export const AnalyticsStatsRow = z.object({
  project_id: uuid(),
  project__name: z.string(),
  cancelled_work_items: z.number(),
  completed_work_items: z.number(),
  backlog_work_items: z.number(),
  un_started_work_items: z.number(),
  started_work_items: z.number(),
});
export type AnalyticsStatsRow = z.infer<typeof AnalyticsStatsRow>;

export const AnalyticsStats = z.array(AnalyticsStatsRow);
export type AnalyticsStats = z.infer<typeof AnalyticsStats>;

/**
 * One project-scoped row, grouped by assignee. The unassigned group comes
 * back with `display_name`, `assignee_id` and `avatar_url` all null; an
 * assigned row's `avatar_url` may be the empty string when the member has no
 * avatar. Counts are distinct per group.
 */
export const AnalyticsProjectStatsRow = z.object({
  display_name: z.nullable(z.string()),
  assignee_id: z.nullable(uuid()),
  avatar_url: z.nullable(z.string()),
  cancelled_work_items: z.number(),
  completed_work_items: z.number(),
  backlog_work_items: z.number(),
  un_started_work_items: z.number(),
  started_work_items: z.number(),
});
export type AnalyticsProjectStatsRow = z.infer<typeof AnalyticsProjectStatsRow>;

export const AnalyticsProjectStats = z.array(AnalyticsProjectStatsRow);
export type AnalyticsProjectStats = z.infer<typeof AnalyticsProjectStats>;

export async function getAnalyticsStats(
  client: ApiClient,
  slug: string,
  scope: AnalyticsScope & { type?: "work-items" } = {}
): Promise<AnalyticsStats> {
  const path = `/api/workspaces/${slug}/advance-analytics-stats/`;
  const body = await client.getJson<unknown>(path, {
    query: { ...scopedQuery(scope), type: scope.type },
  });
  return client.parse(AnalyticsStats, body, `GET ${path}`);
}

export async function getProjectAnalyticsStats(
  client: ApiClient,
  slug: string,
  projectId: string,
  scope: AnalyticsProjectScope & { type?: "work-items" } = {}
): Promise<AnalyticsProjectStats> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/advance-analytics-stats/`;
  const body = await client.getJson<unknown>(path, {
    query: { ...projectQuery(scope), type: scope.type },
  });
  return client.parse(AnalyticsProjectStats, body, `GET ${path}`);
}

/**
 * Fields `project-stats` understands. Any name outside this set is dropped;
 * when nothing valid survives (or `fields` is absent) the server answers all
 * five, and every returned row then carries all five keys. The screen asks
 * for `total_issues,completed_issues` and reads exactly those.
 */
export const PROJECT_STAT_FIELDS = [
  "total_issues",
  "completed_issues",
  "total_members",
  "total_cycles",
  "total_modules",
] as const;
export type ProjectStatField = (typeof PROJECT_STAT_FIELDS)[number];

/**
 * One row per workspace project (or per `project_ids` match), `id` plus the
 * requested fields. `completed_issues` counts the closed state groups, so it
 * is not limited to the `completed` group the breakdown table uses. A
 * project with no work items reports 0, never an absent key, for the fields
 * it was asked for.
 */
export const ProjectStatsRow = z.looseObject({
  id: uuid(),
  total_issues: z.optional(z.nullable(z.number())),
  completed_issues: z.optional(z.nullable(z.number())),
  total_members: z.optional(z.nullable(z.number())),
  total_cycles: z.optional(z.nullable(z.number())),
  total_modules: z.optional(z.nullable(z.number())),
});
export type ProjectStatsRow = z.infer<typeof ProjectStatsRow>;

export const ProjectStats = z.array(ProjectStatsRow);
export type ProjectStats = z.infer<typeof ProjectStats>;

export interface ProjectStatsQuery extends AnalyticsScope {
  fields?: readonly ProjectStatField[];
}

export async function getProjectStats(
  client: ApiClient,
  slug: string,
  query: ProjectStatsQuery = {}
): Promise<ProjectStats> {
  const path = `/api/workspaces/${slug}/project-stats/`;
  const body = await client.getJson<unknown>(path, {
    query: {
      ...scopedQuery(query),
      fields: query.fields && query.fields.length > 0 ? query.fields.join(",") : undefined,
    },
  });
  return client.parse(ProjectStats, body, `GET ${path}`);
}
