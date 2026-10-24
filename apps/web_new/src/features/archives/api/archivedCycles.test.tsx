// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived-cycle reads: one archived cycle through the archived
// detail endpoint under the archived key, and the tab's filters in
// their own namespaced slot, never the live tab's values.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ArchivedCycleDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ getArchivedCycle: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, getArchivedCycle: mocks.getArchivedCycle };
});

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

vi.mock("../../auth/index.js", () => ({ useProjectRow: () => ({ data: { timezone: "UTC" } }) }));

import { REFERENCE_STALE_TIME_MS } from "../../../core/query/index.js";
import { cycleKeys, useCycleFilterStore } from "../../cycles/index.js";
import { makeCycleRow } from "../../cycles/testing.js";
import {
  archivedCycleDetailQueryOptions,
  archivedCycleStoreSlot,
  useArchivedCycleDetail,
  useArchivedCycleListFilter,
} from "./archivedCycles.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

function detail(): ArchivedCycleDetail {
  return {
    id: CYCLE,
    workspace_id: "123e4567-e89b-12d3-a456-426614174000",
    project_id: PROJECT,
    name: "Archived cycle",
    description: "",
    start_date: "2026-01-01T00:00:00Z",
    end_date: "2026-02-01T00:00:00Z",
    owned_by_id: "123e4567-e89b-12d3-a456-426614174010",
    status: "COMPLETED",
    archived_at: "2026-03-01T00:00:00Z",
    is_favorite: false,
    total_issues: 4,
    completed_issues: 4,
    started_issues: 0,
    unstarted_issues: 0,
    cancelled_issues: 0,
    backlog_issues: 0,
    assignee_ids: [],
    sub_issues: 0,
    completed_estimate_points: 0,
    total_estimate_points: 0,
    estimate_distribution: {},
    distribution: { assignees: [], labels: [], completion_chart: null },
  };
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

afterEach(() => {
  useCycleFilterStore.setState({ filters: {} });
  vi.restoreAllMocks();
});

describe("archivedCycleStoreSlot", () => {
  it("namespaces the archived tab away from the live tab's values", () => {
    expect(archivedCycleStoreSlot(PROJECT)).toBe(`archived/${PROJECT}`);
    expect(archivedCycleStoreSlot(PROJECT)).not.toBe(PROJECT);
  });
});

describe("archivedCycleDetailQueryOptions", () => {
  it("reads the archived detail under the archived key as reference data", async () => {
    mocks.getArchivedCycle.mockResolvedValue(detail());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = archivedCycleDetailQueryOptions({ stub: true } as never, "acme", PROJECT, CYCLE);
    expect(options.queryKey).toEqual(cycleKeys.archivedDetail("acme", PROJECT, CYCLE));
    expect(options.staleTime).toBe(REFERENCE_STALE_TIME_MS);
    const fetched = await queryClient.fetchQuery(options);
    expect(mocks.getArchivedCycle).toHaveBeenCalledTimes(1);
    expect(mocks.getArchivedCycle).toHaveBeenCalledWith({ stub: true }, "acme", PROJECT, CYCLE);
    expect(fetched.name).toBe("Archived cycle");
    expect(fetched.archived_at).toBe("2026-03-01T00:00:00Z");
  });
});

describe("useArchivedCycleDetail", () => {
  it("exposes the archived record to the peek", async () => {
    mocks.getArchivedCycle.mockResolvedValue(detail());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useArchivedCycleDetail("acme", PROJECT, CYCLE), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.data).toBeDefined());
    expect(result.current.data?.status).toBe("COMPLETED");
  });
});

describe("useArchivedCycleListFilter", () => {
  function hook() {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    return renderHook(() => useArchivedCycleListFilter("acme", PROJECT), { wrapper: wrapper(queryClient) });
  }

  it("starts inactive with a predicate that keeps every row", () => {
    const { result } = hook();
    expect(result.current.active).toBe(false);
    expect(result.current.predicate(makeCycleRow())).toBe(true);
  });

  it("narrows from the archived slot while the live slot stays empty", () => {
    useCycleFilterStore
      .getState()
      .setDateValues(archivedCycleStoreSlot(PROJECT), "startDate", ["1_weeks;after;fromnow"]);
    const { result } = hook();
    expect(result.current.active).toBe(true);
    expect(useCycleFilterStore.getState().filtersFor(PROJECT)).toEqual({ status: [], startDate: [], endDate: [] });
    // The archived fixture starts in the past, so the future window drops it.
    expect(result.current.predicate(makeCycleRow({ start_date: "2026-01-01T00:00:00Z" }))).toBe(false);
  });

  it("ignores the live tab's values", () => {
    useCycleFilterStore.getState().toggleStatus(PROJECT, "current");
    const { result } = hook();
    expect(result.current.active).toBe(false);
    expect(result.current.predicate(makeCycleRow({ status: "completed" }))).toBe(true);
  });

  it("clears the archived slot without touching the live tab", () => {
    useCycleFilterStore
      .getState()
      .setDateValues(archivedCycleStoreSlot(PROJECT), "startDate", ["1_weeks;after;fromnow"]);
    useCycleFilterStore.getState().toggleStatus(PROJECT, "current");
    const { result } = hook();
    expect(result.current.active).toBe(true);
    result.current.clearAll();
    expect(useCycleFilterStore.getState().filtersFor(archivedCycleStoreSlot(PROJECT))).toEqual({
      status: [],
      startDate: [],
      endDate: [],
    });
    expect(useCycleFilterStore.getState().filtersFor(PROJECT).status).toEqual(["current"]);
  });
});
