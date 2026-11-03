// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The shell reads key under the cycle keys and fetch through the
// api-client contracts; the archived read never refetches on window
// focus (CYC-045).

import { QueryClient } from "@tanstack/react-query";
import type { ArchivedCycleList, CycleRows } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ listCycleRows: vi.fn(), listArchivedCycles: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, listCycleRows: mocks.listCycleRows, listArchivedCycles: mocks.listArchivedCycles };
});

import { LIST_STALE_TIME_MS } from "../../../core/query/index.js";
import { archivedCyclesQueryOptions, cyclesListQueryOptions } from "./queries.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

function liveRows(): CycleRows {
  return [
    {
      id: CYCLE,
      workspace_id: "123e4567-e89b-12d3-a456-426614174000",
      project_id: PROJECT,
      name: "Cycle 7",
      start_date: null,
      end_date: null,
      is_favorite: false,
    },
  ];
}

function archivedRows(): ArchivedCycleList {
  return [
    {
      id: CYCLE,
      workspace_id: "123e4567-e89b-12d3-a456-426614174000",
      project_id: PROJECT,
      name: "Cycle 6",
      description: "",
      start_date: null,
      end_date: null,
      owned_by_id: "123e4567-e89b-12d3-a456-426614174000",
      status: "COMPLETED",
      archived_at: "2026-09-01T00:00:00.000Z",
      is_favorite: false,
      total_issues: 0,
      completed_issues: 0,
      started_issues: 0,
      unstarted_issues: 0,
      cancelled_issues: 0,
      backlog_issues: 0,
      assignee_ids: [],
    },
  ];
}

describe("cyclesListQueryOptions", () => {
  it("fetches the bare live array under the list key", async () => {
    mocks.listCycleRows.mockResolvedValue(liveRows());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = cyclesListQueryOptions({ stub: true } as never, "acme", PROJECT);
    expect(options.queryKey).toEqual(["ws", "acme", "cycles", "list", PROJECT]);
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    const fetched = await queryClient.fetchQuery(options);
    expect(mocks.listCycleRows).toHaveBeenCalledWith({ stub: true }, "acme", PROJECT);
    expect(fetched).toHaveLength(1);
  });
});

describe("archivedCyclesQueryOptions", () => {
  it("fetches the archived list and never refetches on window focus", async () => {
    mocks.listArchivedCycles.mockResolvedValue(archivedRows());
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = archivedCyclesQueryOptions({ stub: true } as never, "acme", PROJECT);
    expect(options.queryKey).toEqual(["ws", "acme", "cycles", "archived", PROJECT]);
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(options.refetchOnWindowFocus).toBe(false);
    const fetched = await queryClient.fetchQuery(options);
    expect(mocks.listArchivedCycles).toHaveBeenCalledWith({ stub: true }, "acme", PROJECT);
    expect(fetched).toHaveLength(1);
  });
});
