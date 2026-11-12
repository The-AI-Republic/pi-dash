// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics projects-chart query tests (parity/specs/analytics.md §16, ANL-006).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsChart: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import {
  analyticsCustomChartQueryOptions,
  analyticsProjectsChartQueryOptions,
  analyticsWorkItemsChartQueryOptions,
  useAnalyticsCustomChart,
  useAnalyticsProjectsChart,
  useAnalyticsWorkItemsChart,
} from "./charts.js";
import { analyticsKeys } from "./keys.js";

const CLIENT = { stub: true } as unknown as ApiClient;

function wrapper(client: QueryClient): React.FC<{ children: React.ReactNode }> {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("analyticsKeys charts", () => {
  it("roots charts under the workspace analytics subtree with the kind and scope in the key", () => {
    expect(analyticsKeys.charts("acme", "projects", ["a"])).toEqual([
      "ws",
      "acme",
      "analytics",
      "charts",
      "projects",
      ["a"],
    ]);
  });

  it("keys custom reads by scope and axis choice with a null split", () => {
    expect(analyticsKeys.customCharts("acme", ["a"], "PRIORITY", "WORK_ITEM_COUNT", null)).toEqual([
      "ws",
      "acme",
      "analytics",
      "charts",
      "custom-work-items",
      ["a"],
      "PRIORITY",
      "WORK_ITEM_COUNT",
      null,
    ]);
    expect(analyticsKeys.customCharts("acme", ["a"], "PRIORITY", "WORK_ITEM_COUNT", "STATES")).toContain("STATES");
  });
});

describe("analyticsProjectsChartQueryOptions", () => {
  it("normalizes the scope before it enters the key and the request", async () => {
    const rows = [{ key: "work_items", name: "Work Items", count: 2 }];
    contractMocks.getAnalyticsChart.mockResolvedValue(rows);
    const options = analyticsProjectsChartQueryOptions(CLIENT, "acme", ["b", "a", "b"]);
    expect(options.queryKey).toEqual(["ws", "acme", "analytics", "charts", "projects", ["b", "a"]]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(rows);
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(CLIENT, "acme", {
      type: "projects",
      projectIds: ["b", "a"],
    });
  });

  it("requests the graph under an empty scope", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue([]);
    const options = analyticsProjectsChartQueryOptions(CLIENT, "acme", []);
    expect(options.queryKey).toEqual(["ws", "acme", "analytics", "charts", "projects", []]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([]);
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(CLIENT, "acme", {
      type: "projects",
      projectIds: [],
    });
  });
});

describe("useAnalyticsProjectsChart", () => {
  it("reads the graph through the app client", async () => {
    const rows = [{ key: "cycles", name: "Cycles", count: 0 }];
    contractMocks.getAnalyticsChart.mockResolvedValue(rows);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useAnalyticsProjectsChart("acme", []), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(rows);
  });
});

describe("analyticsWorkItemsChartQueryOptions", () => {
  it("requests the work-items series under a normalized scope", async () => {
    const series = { data: [], schema: {} };
    contractMocks.getAnalyticsChart.mockResolvedValue(series);
    const options = analyticsWorkItemsChartQueryOptions(CLIENT, "acme", ["b", "a", "b"]);
    expect(options.queryKey).toEqual(["ws", "acme", "analytics", "charts", "work-items", ["b", "a"]]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(series);
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(CLIENT, "acme", {
      type: "work-items",
      projectIds: ["b", "a"],
    });
  });
});

describe("useAnalyticsWorkItemsChart", () => {
  it("reads the series through the app client", async () => {
    const series = { data: [], schema: {} };
    contractMocks.getAnalyticsChart.mockResolvedValue(series);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useAnalyticsWorkItemsChart("acme", []), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(series);
  });
});

describe("analyticsCustomChartQueryOptions", () => {
  it("requests the custom graph with axes and an absent split omitted", async () => {
    const graph = { data: [], schema: {} };
    contractMocks.getAnalyticsChart.mockResolvedValue(graph);
    const options = analyticsCustomChartQueryOptions(CLIENT, "acme", ["b", "a", "b"], {
      xAxis: "PRIORITY",
      yAxis: "WORK_ITEM_COUNT",
    });
    expect(options.queryKey).toEqual([
      "ws",
      "acme",
      "analytics",
      "charts",
      "custom-work-items",
      ["b", "a"],
      "PRIORITY",
      "WORK_ITEM_COUNT",
      null,
    ]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(graph);
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(CLIENT, "acme", {
      type: "custom-work-items",
      xAxis: "PRIORITY",
      yAxis: "WORK_ITEM_COUNT",
      projectIds: ["b", "a"],
    });
  });

  it("sends the split once chosen", async () => {
    contractMocks.getAnalyticsChart.mockResolvedValue({ data: [], schema: {} });
    const options = analyticsCustomChartQueryOptions(CLIENT, "acme", [], {
      xAxis: "PRIORITY",
      yAxis: "WORK_ITEM_COUNT",
      groupBy: "STATES",
    });
    expect(options.queryKey).toContain("STATES");
    await options.queryFn?.({} as never);
    expect(contractMocks.getAnalyticsChart).toHaveBeenCalledWith(CLIENT, "acme", {
      type: "custom-work-items",
      xAxis: "PRIORITY",
      yAxis: "WORK_ITEM_COUNT",
      groupBy: "STATES",
      projectIds: [],
    });
  });
});

describe("useAnalyticsCustomChart", () => {
  it("reads the custom graph through the app client", async () => {
    const graph = { data: [], schema: {} };
    contractMocks.getAnalyticsChart.mockResolvedValue(graph);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(
      () => useAnalyticsCustomChart("acme", [], { xAxis: "PRIORITY", yAxis: "WORK_ITEM_COUNT" }),
      { wrapper: wrapper(queryClient) }
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(graph);
  });
});
