// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scoped dialog read tests (parity/specs/analytics.md §13, ANL-014).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: {
    getProjectAnalyticsTotals: vi.fn(),
    getProjectAnalyticsChart: vi.fn(),
    getProjectAnalyticsStats: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { analyticsKeys } from "./keys.js";
import {
  dialogCustomChartQueryOptions,
  dialogRequestScope,
  dialogStatsQueryOptions,
  dialogTotalsQueryOptions,
  dialogWorkItemsChartQueryOptions,
  useDialogStats,
  useDialogTotals,
} from "./projectScope.js";

const CLIENT = { stub: true } as unknown as ApiClient;
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";
const MODULE = "423e4567-e89b-12d3-a456-426614174003";

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

describe("dialogRequestScope", () => {
  it("echoes the path project as project_ids", () => {
    expect(dialogRequestScope({ projectId: PROJECT })).toEqual({
      projectIds: [PROJECT],
      cycleId: undefined,
      moduleId: undefined,
    });
  });

  it("carries the cycle and module narrowing", () => {
    expect(dialogRequestScope({ projectId: PROJECT, cycleId: CYCLE, moduleId: MODULE })).toEqual({
      projectIds: [PROJECT],
      cycleId: CYCLE,
      moduleId: MODULE,
    });
  });
});

describe("dialog query keys", () => {
  it("key each read by project, cycle and module", () => {
    const scope = { projectId: PROJECT, cycleId: CYCLE };
    expect(analyticsKeys.dialog("acme", "totals", scope)).toEqual([
      "ws",
      "acme",
      "analytics",
      "dialog",
      "totals",
      [PROJECT, CYCLE, ""],
    ]);
    expect(analyticsKeys.dialog("acme", "stats", scope)).toEqual([
      "ws",
      "acme",
      "analytics",
      "dialog",
      "stats",
      [PROJECT, CYCLE, ""],
    ]);
  });

  it("separate scopes never share an entry", () => {
    const bare = analyticsKeys.dialog("acme", "totals", { projectId: PROJECT });
    const narrowed = analyticsKeys.dialog("acme", "totals", { projectId: PROJECT, cycleId: CYCLE });
    expect(bare).not.toEqual(narrowed);
  });

  it("key the custom read by the axis choice on top of the scope", () => {
    expect(
      analyticsKeys.dialogCustomChart("acme", { projectId: PROJECT }, "PRIORITY", "WORK_ITEM_COUNT", null)
    ).toEqual([
      "ws",
      "acme",
      "analytics",
      "dialog",
      "charts",
      "custom-work-items",
      [PROJECT, "", ""],
      "PRIORITY",
      "WORK_ITEM_COUNT",
      null,
    ]);
  });
});

describe("dialog reads", () => {
  it("read the project totals endpoint with the scope", async () => {
    contractMocks.getProjectAnalyticsTotals.mockResolvedValue({ total_work_items: { count: 2 } });
    await dialogTotalsQueryOptions(CLIENT, "acme", { projectId: PROJECT, cycleId: CYCLE }).queryFn!({} as never);
    expect(contractMocks.getProjectAnalyticsTotals).toHaveBeenCalledWith(CLIENT, "acme", PROJECT, {
      projectIds: [PROJECT],
      cycleId: CYCLE,
      moduleId: undefined,
    });
  });

  it("read the project work-items chart with the module narrowing", async () => {
    contractMocks.getProjectAnalyticsChart.mockResolvedValue({ data: [], schema: {} });
    await dialogWorkItemsChartQueryOptions(CLIENT, "acme", { projectId: PROJECT, moduleId: MODULE }).queryFn!(
      {} as never
    );
    expect(contractMocks.getProjectAnalyticsChart).toHaveBeenCalledWith(
      CLIENT,
      "acme",
      PROJECT,
      { type: "work-items" },
      { projectIds: [PROJECT], cycleId: undefined, moduleId: MODULE }
    );
  });

  it("read the project custom chart with the axis choice", async () => {
    contractMocks.getProjectAnalyticsChart.mockResolvedValue({ data: [], schema: {} });
    await dialogCustomChartQueryOptions(
      CLIENT,
      "acme",
      { projectId: PROJECT },
      {
        xAxis: "PRIORITY",
        yAxis: "WORK_ITEM_COUNT",
        groupBy: "STATE_GROUPS",
      }
    ).queryFn!({} as never);
    expect(contractMocks.getProjectAnalyticsChart).toHaveBeenCalledWith(
      CLIENT,
      "acme",
      PROJECT,
      {
        type: "custom-work-items",
        xAxis: "PRIORITY",
        yAxis: "WORK_ITEM_COUNT",
        groupBy: "STATE_GROUPS",
      },
      { projectIds: [PROJECT], cycleId: undefined, moduleId: undefined }
    );
  });

  it("read the project stats endpoint", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([]);
    await dialogStatsQueryOptions(CLIENT, "acme", { projectId: PROJECT }).queryFn!({} as never);
    expect(contractMocks.getProjectAnalyticsStats).toHaveBeenCalledWith(CLIENT, "acme", PROJECT, {
      projectIds: [PROJECT],
      cycleId: undefined,
      moduleId: undefined,
    });
  });
});

describe("dialog hooks", () => {
  it("share one cache entry for the same scope", async () => {
    contractMocks.getProjectAnalyticsTotals.mockResolvedValue({ total_work_items: { count: 5 } });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useDialogTotals("acme", { projectId: PROJECT }), {
      wrapper: wrapper(client),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.getProjectAnalyticsTotals).toHaveBeenCalledTimes(1);
  });

  it("read the stats once per scope", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useDialogStats("acme", { projectId: PROJECT, cycleId: CYCLE }), {
      wrapper: wrapper(client),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.getProjectAnalyticsStats).toHaveBeenCalledWith(
      expect.anything(),
      "acme",
      PROJECT,
      expect.objectContaining({ cycleId: CYCLE })
    );
  });
});
