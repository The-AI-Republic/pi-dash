// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The cycle detail read keys under the workspace scope and fetches the
// stored row through the api-client detail contract (NEWFRONT-859).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { CycleRow } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ getCycle: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, getCycle: mocks.getCycle };
});

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { REFERENCE_STALE_TIME_MS } from "../../../core/query/index.js";
import { cycleDetailKeys, cycleDetailQueryOptions, useCycleDetail } from "./cycleDetail.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

function row(): CycleRow {
  return {
    id: CYCLE,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: PROJECT,
    name: "Cycle 7",
    start_date: "2026-11-19T00:00:00.000Z",
    end_date: "2026-11-26T00:00:00.000Z",
    is_favorite: false,
  };
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

describe("cycleDetailKeys", () => {
  it("scopes the detail read under the workspace", () => {
    expect(cycleDetailKeys.detail("acme", PROJECT, CYCLE)).toEqual(["ws", "acme", "cycles", "detail", PROJECT, CYCLE]);
  });
});

describe("cycleDetailQueryOptions", () => {
  it("fetches the stored row once and keeps it as reference data", async () => {
    mocks.getCycle.mockResolvedValue(row());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = cycleDetailQueryOptions({ stub: true } as never, "acme", PROJECT, CYCLE);
    expect(options.queryKey).toEqual(["ws", "acme", "cycles", "detail", PROJECT, CYCLE]);
    expect(options.staleTime).toBe(REFERENCE_STALE_TIME_MS);
    const fetched = await queryClient.fetchQuery(options);
    expect(mocks.getCycle).toHaveBeenCalledTimes(1);
    expect(mocks.getCycle).toHaveBeenCalledWith({ stub: true }, "acme", PROJECT, CYCLE);
    expect(fetched.name).toBe("Cycle 7");
    expect(fetched.start_date).toBe("2026-11-19T00:00:00.000Z");
  });
});

describe("useCycleDetail", () => {
  it("exposes the stored row to components", async () => {
    mocks.getCycle.mockResolvedValue(row());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useCycleDetail("acme", PROJECT, CYCLE), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.data).toBeDefined());
    expect(result.current.data?.end_date).toBe("2026-11-26T00:00:00.000Z");
  });
});
