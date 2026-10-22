// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics chart contract against a stub client (no backend): the
// custom work-item query, the estimate axis, and the aggregated rows.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ANALYTICS_CHART_ESTIMATE_AXIS, getProjectAnalyticsChart } from "./analytics.js";

const WS = "acme";
const PROJECT = "123e4567-e89b-12d3-a456-426614174000";
const CYCLE = "223e4567-e89b-12d3-a456-426614174001";

function stubClient(body: unknown): { client: ApiClient; urls: string[]; queries: unknown[] } {
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

describe("getProjectAnalyticsChart", () => {
  it("queries the estimate axis and parses the aggregated rows", async () => {
    const { client, urls, queries } = stubClient({
      data: [
        { key: 1, name: "1", count: 2 },
        { key: 2, name: "3", count: 1 },
      ],
      schema: {},
    });
    const chart = await getProjectAnalyticsChart(client, WS, PROJECT, {
      xAxis: ANALYTICS_CHART_ESTIMATE_AXIS,
    });
    expect(urls).toEqual([`/api/workspaces/${WS}/projects/${PROJECT}/advance-analytics-charts/`]);
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
    const { client, queries } = stubClient({ data: [], schema: {} });
    await getProjectAnalyticsChart(client, WS, PROJECT, {
      xAxis: "PRIORITY",
      groupBy: ANALYTICS_CHART_ESTIMATE_AXIS,
      cycleId: CYCLE,
    });
    expect(queries).toEqual([
      {
        type: "custom-work-items",
        x_axis: "PRIORITY",
        group_by: "ESTIMATE_POINTS",
        cycle_id: CYCLE,
        module_id: undefined,
        date_filter: undefined,
      },
    ]);
  });

  it("rejects a payload without the row list", async () => {
    const { client } = stubClient({ schema: {} });
    await expect(
      getProjectAnalyticsChart(client, WS, PROJECT, { xAxis: ANALYTICS_CHART_ESTIMATE_AXIS })
    ).rejects.toThrow();
  });
});
