// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics contract unit tests against a stub client: the request surface
// (paths, query keys, omitted parameters) and the parse branches the live
// suite cannot reach without a second backend state. Shapes that need Django
// are pinned in src/contract/analytics.contract.test.ts. The trailing block
// pins the estimates builder shorthand (EST-020): the custom work-item
// query, the estimate axis, and the aggregated rows.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  ANALYTICS_CHART_ESTIMATE_AXIS,
  AnalyticsCustomChart,
  AnalyticsOverviewTotals,
  AnalyticsWorkItemsChart,
  AnalyticsWorkItemsTotals,
  ProjectStats,
  getAnalyticsChart,
  getAnalyticsStats,
  getAnalyticsTotals,
  getProjectAnalyticsChart,
  getProjectAnalyticsStats,
  getProjectAnalyticsTotals,
  getProjectStats,
} from "./analytics.js";

interface Call {
  url?: string;
  query?: Record<string, unknown> | undefined;
}

function stubClient(body: unknown, calls: Call[] = []): ApiClient {
  return {
    getJson: (async (url: string, options?: { query?: Record<string, unknown> }) => {
      calls.push({ url, query: options?.query });
      return body;
    }) as ApiClient["getJson"],
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
  } as ApiClient;
}

describe("analytics totals", () => {
  it("reads the overview tab and omits an empty scope", async () => {
    const calls: Call[] = [];
    const client = stubClient({ total_projects: { count: 2 }, extra: { count: 1 } }, calls);
    const totals = await getAnalyticsTotals(client, "acme");
    expect(calls[0]?.url).toBe("/api/workspaces/acme/advance-analytics/");
    expect(calls[0]?.query).toEqual({ tab: undefined });
    expect(totals.total_projects?.count).toBe(2);
    expect(totals).toHaveProperty("extra");
  });

  it("sends the tab, the joined project ids and the project-scoped path", async () => {
    const calls: Call[] = [];
    const client = stubClient({ total_work_items: { count: 3 } }, calls);
    await getAnalyticsTotals(client, "acme", { tab: "work-items", projectIds: ["a", "b"] });
    expect(calls[0]).toEqual({
      url: "/api/workspaces/acme/advance-analytics/",
      query: { project_ids: "a,b", tab: "work-items" },
    });
    await getProjectAnalyticsTotals(client, "acme", "p1", { projectIds: ["p1"], cycleId: "c1" });
    expect(calls[1]?.url).toBe("/api/workspaces/acme/projects/p1/advance-analytics/");
    expect(calls[1]?.query).toEqual({ project_ids: "p1", cycle_id: "c1", module_id: undefined });
  });

  it("parses a metric-less payload, reading absent metrics as zero by the caller", async () => {
    expect(AnalyticsOverviewTotals.parse({})).toEqual({});
    expect(AnalyticsWorkItemsTotals.parse({ total_work_items: {} })).toEqual({ total_work_items: {} });
    expect(AnalyticsWorkItemsTotals.parse({ total_work_items: { count: null } })).toEqual({
      total_work_items: { count: null },
    });
  });
});

describe("analytics charts", () => {
  it("sends no chart-specific keys for the radar and series reads", async () => {
    const calls: Call[] = [];
    await getAnalyticsChart(stubClient([], calls), "acme", { type: "projects", projectIds: ["p1"] });
    expect(calls[0]?.query).toEqual({ type: "projects", project_ids: "p1" });
    await getAnalyticsChart(stubClient({ data: [], schema: {} }, calls), "acme", { type: "work-items" });
    expect(calls[1]?.query).toEqual({ type: "work-items" });
  });

  it("sends the custom axis, optional y axis and split", async () => {
    const calls: Call[] = [];
    const client = stubClient({ data: [], schema: {} }, calls);
    await getAnalyticsChart(client, "acme", {
      type: "custom-work-items",
      xAxis: "STATES",
      yAxis: "WORK_ITEM_COUNT",
      groupBy: "PRIORITY",
    });
    expect(calls[0]?.query).toEqual({
      type: "custom-work-items",
      x_axis: "STATES",
      y_axis: "WORK_ITEM_COUNT",
      group_by: "PRIORITY",
    });
    await getProjectAnalyticsChart(
      client,
      "acme",
      "p1",
      { type: "custom-work-items", xAxis: "LABELS" },
      { moduleId: "m1" }
    );
    expect(calls[1]?.url).toBe("/api/workspaces/acme/projects/p1/advance-analytics-charts/");
    expect(calls[1]?.query).toEqual({
      module_id: "m1",
      type: "custom-work-items",
      x_axis: "LABELS",
      y_axis: undefined,
      group_by: undefined,
    });
  });

  it("keeps stacked segment keys and preserves schema order", async () => {
    const parsed = AnalyticsCustomChart.parse({
      data: [{ key: "backlog", name: "backlog", count: 4, medium: 3, low: 1 }],
      schema: { medium: "medium", low: "low" },
    });
    expect(Object.keys(parsed.schema)).toEqual(["medium", "low"]);
    expect(parsed.data[0]?.["medium"]).toBe(3);
  });

  it("accepts the integer key the estimate-point axis answers", async () => {
    expect(AnalyticsCustomChart.parse({ data: [{ key: 2, name: "2", count: 1 }], schema: {} }).data).toEqual([
      { key: 2, name: "2", count: 1 },
    ]);
    const missing = AnalyticsCustomChart.parse({
      data: [{ key: "None", name: "None", count: 0 }],
      schema: {},
    });
    expect(missing.data[0]?.key).toBe("None");
  });

  it("accepts the empty series an undated scope answers", async () => {
    expect(AnalyticsWorkItemsChart.parse({ data: [], schema: {} })).toEqual({ data: [], schema: {} });
  });
});

describe("analytics stats and project stats", () => {
  it("sends the stats type and scope on both surfaces", async () => {
    const calls: Call[] = [];
    const client = stubClient([], calls);
    await getAnalyticsStats(client, "acme", { type: "work-items", projectIds: ["p1"] });
    expect(calls[0]).toEqual({
      url: "/api/workspaces/acme/advance-analytics-stats/",
      query: { project_ids: "p1", type: "work-items" },
    });
    await getProjectAnalyticsStats(client, "acme", "p1", { type: "work-items", cycleId: "c1" });
    expect(calls[1]?.url).toBe("/api/workspaces/acme/projects/p1/advance-analytics-stats/");
    expect(calls[1]?.query).toEqual({ cycle_id: "c1", module_id: undefined, type: "work-items" });
  });

  it("sends the joined field list and the scope for project stats", async () => {
    const calls: Call[] = [];
    const client = stubClient([], calls);
    await getProjectStats(client, "acme", {
      fields: ["total_issues", "completed_issues"],
      projectIds: ["p1"],
    });
    expect(calls[0]).toEqual({
      url: "/api/workspaces/acme/project-stats/",
      query: { project_ids: "p1", fields: "total_issues,completed_issues" },
    });
    await getProjectStats(client, "acme", { fields: [] });
    expect(calls[1]?.query).toEqual({ fields: undefined });
  });

  it("keeps absent project-stat fields absent", async () => {
    const parsed = ProjectStats.parse([{ id: "123e4567-e89b-12d3-a456-426614174000", total_issues: 4 }]);
    expect(parsed[0]).not.toHaveProperty("completed_issues");
  });
});

// Estimates builder shorthand (EST-020): the untyped project chart query
// keeps the query shape NEWFRONT-737 contracted.
const CHART_WS = "acme";
const CHART_PROJECT = "123e4567-e89b-12d3-a456-426614174000";
const CHART_CYCLE = "223e4567-e89b-12d3-a456-426614174001";

function stubChartClient(body: unknown): { client: ApiClient; urls: string[]; queries: unknown[] } {
  const urls: string[] = [];
  const queries: unknown[] = [];
  const client = {
    parse: ((schema: { parse: (value: unknown) => unknown }, value: unknown) =>
      schema.parse(value)) as ApiClient["parse"],
    getJson: (async (url: string, options?: { query?: unknown }) => {
      urls.push(url);
      queries.push(options?.query);
      return body;
    }) as ApiClient["getJson"],
  } as ApiClient;
  return { client, urls, queries };
}

describe("getProjectAnalyticsChart builder shorthand", () => {
  it("queries the estimate axis and parses the aggregated rows", async () => {
    const { client, urls, queries } = stubChartClient({
      data: [
        { key: 1, name: "1", count: 2 },
        { key: 2, name: "3", count: 1 },
      ],
      schema: {},
    });
    const chart = await getProjectAnalyticsChart(client, CHART_WS, CHART_PROJECT, {
      xAxis: ANALYTICS_CHART_ESTIMATE_AXIS,
    });
    expect(urls).toEqual([`/api/workspaces/${CHART_WS}/projects/${CHART_PROJECT}/advance-analytics-charts/`]);
    expect(queries).toEqual([
      {
        type: "custom-work-items",
        x_axis: "ESTIMATE_POINTS",
        group_by: undefined,
        cycle_id: undefined,
        module_id: undefined,
        date_filter: undefined,
      },
    ]);
    expect(chart.data.map((row) => row.count)).toEqual([2, 1]);
  });

  it("passes the second grouping and scopes through", async () => {
    const { client, queries } = stubChartClient({ data: [], schema: {} });
    await getProjectAnalyticsChart(client, CHART_WS, CHART_PROJECT, {
      xAxis: "PRIORITY",
      groupBy: ANALYTICS_CHART_ESTIMATE_AXIS,
      cycleId: CHART_CYCLE,
    });
    expect(queries).toEqual([
      {
        type: "custom-work-items",
        x_axis: "PRIORITY",
        group_by: "ESTIMATE_POINTS",
        cycle_id: CHART_CYCLE,
        module_id: undefined,
        date_filter: undefined,
      },
    ]);
  });

  it("rejects a payload without the row list", async () => {
    const { client } = stubChartClient({ schema: {} });
    await expect(
      getProjectAnalyticsChart(client, CHART_WS, CHART_PROJECT, { xAxis: ANALYTICS_CHART_ESTIMATE_AXIS })
    ).rejects.toThrow();
  });
});
