// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics state-breakdown query tests (parity/specs/analytics.md §16.3,
// ANL-013).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsStats: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { analyticsKeys } from "./keys.js";
import { analyticsStatsQueryOptions, useAnalyticsStats } from "./stats.js";

const CLIENT = { stub: true } as unknown as ApiClient;

const ROW = {
  project_id: "00000000-0000-4000-8000-000000000001",
  project__name: "Alpha",
  cancelled_work_items: 0,
  completed_work_items: 1,
  backlog_work_items: 2,
  un_started_work_items: 3,
  started_work_items: 4,
};

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

describe("analyticsKeys stats", () => {
  it("keys the work-items breakdown by normalized scope", () => {
    expect(analyticsKeys.stats("acme", ["a"])).toEqual(["ws", "acme", "analytics", "stats", "work-items", ["a"]]);
  });
});

describe("analyticsStatsQueryOptions", () => {
  it("normalizes scope and requests work-items stats", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([ROW]);
    const options = analyticsStatsQueryOptions(CLIENT, "acme", ["b", "a", "b"]);
    expect(options.queryKey).toEqual(["ws", "acme", "analytics", "stats", "work-items", ["b", "a"]]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([ROW]);
    expect(contractMocks.getAnalyticsStats).toHaveBeenCalledWith(CLIENT, "acme", {
      type: "work-items",
      projectIds: ["b", "a"],
    });
  });

  it("requests the all-projects scope under an empty selection", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([]);
    const options = analyticsStatsQueryOptions(CLIENT, "acme", []);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([]);
    expect(contractMocks.getAnalyticsStats).toHaveBeenCalledWith(CLIENT, "acme", {
      type: "work-items",
      projectIds: [],
    });
  });
});

describe("useAnalyticsStats", () => {
  it("reads through the app client", async () => {
    contractMocks.getAnalyticsStats.mockResolvedValue([ROW]);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useAnalyticsStats("acme", []), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual([ROW]);
  });
});
