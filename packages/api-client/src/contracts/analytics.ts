// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project analytics chart query (EST-020): the custom work-item chart
// behind the chart builder, grouped by one axis. The estimates area
// needs the estimate axis only; the axis catalogue and the row shape are
// typed here so the analytics area extends this module instead of
// forking it.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

/**
 * Grouping axes the chart endpoint accepts, in the server's own names.
 * `ESTIMATE_POINTS` groups by the item's estimate value: the key is the
 * value's position number and the name its stored text. Unknown names
 * are a 400, so callers stay inside this list.
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

export interface AnalyticsChartQuery {
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

/**
 * One aggregated chart row. `key` is the grouping key — the estimate
 * position number on the estimate axis, `"None"` when the item carries
 * no value — `name` the displayed label and `count` the items in the
 * row. Grouped answers add one member per inner group on top of these
 * shared members.
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
 * GET .../projects/<pid>/advance-analytics-charts/ with
 * type=custom-work-items — the chart builder's aggregated rows for one
 * grouping axis. The y measure is always the work-item count; the
 * endpoint takes no y parameter, so there is no estimate metric to ask
 * for. On the estimate axis each row is one estimate value with its
 * item count.
 */
export async function getProjectAnalyticsChart(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: AnalyticsChartQuery
): Promise<AnalyticsChartResponse> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/advance-analytics-charts/`;
  const body = await client.getJson<unknown>(path, {
    query: {
      type: "custom-work-items",
      x_axis: query.xAxis,
      group_by: query.groupBy,
      cycle_id: query.cycleId,
      module_id: query.moduleId,
      date_filter: query.dateFilter,
    },
  });
  return client.parse(AnalyticsChartResponse, body, `GET ${path}`);
}
