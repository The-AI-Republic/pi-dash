// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics totals query tests (parity/specs/analytics.md §16, ANL-005, ANL-008).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getAnalyticsTotals: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { analyticsKeys } from "./keys.js";
import { analyticsTotalsQueryOptions, useAnalyticsTotals } from "./totals.js";

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

describe("analyticsKeys", () => {
  it("roots totals under the workspace analytics subtree with the scope in the key", () => {
    expect(analyticsKeys.all("acme")).toEqual(["ws", "acme", "analytics"]);
    expect(analyticsKeys.totals("acme", "overview", ["a"])).toEqual([
      "ws",
      "acme",
      "analytics",
      "totals",
      "overview",
      ["a"],
    ]);
  });
});

describe("analyticsTotalsQueryOptions", () => {
  it("normalizes the scope before it enters the key and the request", async () => {
    contractMocks.getAnalyticsTotals.mockResolvedValue({ total_users: { count: 2 } });
    const options = analyticsTotalsQueryOptions(CLIENT, "acme", "overview", ["b", "a", "b"]);
    expect(options.queryKey).toEqual(["ws", "acme", "analytics", "totals", "overview", ["b", "a"]]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ total_users: { count: 2 } });
    expect(contractMocks.getAnalyticsTotals).toHaveBeenCalledWith(CLIENT, "acme", {
      tab: "overview",
      projectIds: ["b", "a"],
    });
  });

  it("requests the work-items tab under an empty scope", async () => {
    contractMocks.getAnalyticsTotals.mockResolvedValue({ total_work_items: { count: 1 } });
    const options = analyticsTotalsQueryOptions(CLIENT, "acme", "work-items", []);
    expect(options.queryKey).toEqual(["ws", "acme", "analytics", "totals", "work-items", []]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ total_work_items: { count: 1 } });
    expect(contractMocks.getAnalyticsTotals).toHaveBeenCalledWith(CLIENT, "acme", {
      tab: "work-items",
      projectIds: [],
    });
  });
});

describe("useAnalyticsTotals", () => {
  it("reads one tab's totals through the app client", async () => {
    contractMocks.getAnalyticsTotals.mockResolvedValue({ total_projects: { count: 4 } });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useAnalyticsTotals("acme", "overview", []), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual({ total_projects: { count: 4 } });
  });
});
