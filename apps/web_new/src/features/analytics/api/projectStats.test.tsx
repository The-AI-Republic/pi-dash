// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics project-stats query tests (parity/specs/analytics.md §16, ANL-007).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getProjectStats: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { analyticsKeys } from "./keys.js";
import { analyticsProjectStatsQueryOptions, useAnalyticsProjectStats } from "./projectStats.js";

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

describe("analyticsKeys projectStats", () => {
  it("roots project-stats under the workspace analytics subtree with the scope in the key", () => {
    expect(analyticsKeys.projectStats("acme", ["a"])).toEqual(["ws", "acme", "analytics", "project-stats", ["a"]]);
  });
});

describe("analyticsProjectStatsQueryOptions", () => {
  it("normalizes the scope and requests the two consumed fields", async () => {
    const rows = [{ id: "a", total_issues: 2, completed_issues: 1 }];
    contractMocks.getProjectStats.mockResolvedValue(rows);
    const options = analyticsProjectStatsQueryOptions(CLIENT, "acme", ["b", "a", "b"]);
    expect(options.queryKey).toEqual(["ws", "acme", "analytics", "project-stats", ["b", "a"]]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(rows);
    expect(contractMocks.getProjectStats).toHaveBeenCalledWith(CLIENT, "acme", {
      projectIds: ["b", "a"],
      fields: ["total_issues", "completed_issues"],
    });
  });

  it("requests the stats under an empty scope", async () => {
    contractMocks.getProjectStats.mockResolvedValue([]);
    const options = analyticsProjectStatsQueryOptions(CLIENT, "acme", []);
    expect(options.queryKey).toEqual(["ws", "acme", "analytics", "project-stats", []]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([]);
    expect(contractMocks.getProjectStats).toHaveBeenCalledWith(CLIENT, "acme", {
      projectIds: [],
      fields: ["total_issues", "completed_issues"],
    });
  });
});

describe("useAnalyticsProjectStats", () => {
  it("reads the stats through the app client", async () => {
    const rows = [{ id: "a", total_issues: 0, completed_issues: 0 }];
    contractMocks.getProjectStats.mockResolvedValue(rows);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useAnalyticsProjectStats("acme", []), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(rows);
  });
});
