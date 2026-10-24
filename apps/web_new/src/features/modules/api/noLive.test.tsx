// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules never live-update (parity/specs/modules.md §8.5, MOD-039; bug
// NEWFRONT-332's intended behavior): the list, the detail the page and the
// peek share, the archived detail, the sidebar's estimate read and the
// project facts all fetch on mount and then stay put across a focus, a
// reconnect and a timer tick. The request spy counts calls on the
// api-client seam, so an options-only pin cannot pass while a read still
// refetches. The acting user's own invalidation is the one thing that does
// refresh a module view.
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, render } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  listProjectModules: vi.fn(),
  getModule: vi.fn(),
  getArchivedModule: vi.fn(),
  listProjectEstimates: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    listProjectModules: mocks.listProjectModules,
    getModule: mocks.getModule,
    getArchivedModule: mocks.getArchivedModule,
    listProjectEstimates: mocks.listProjectEstimates,
  };
});

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { projectEstimatesQueryOptions } from "../../estimates/index.js";
import { invalidateAfterModuleWrite } from "./invalidate.js";
import { moduleProjectFactsQueryOptions } from "./projectFacts.js";
import { modulesListQueryOptions } from "./queries.js";
import {
  archivedModuleDetailQueryOptions,
  moduleDetailQueryOptions,
  moduleSidebarEstimatesQueryOptions,
} from "../components/sidebar/moduleSidebarQueries.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const CLIENT = { name: "spy-client" } as never;

const MODULE_DETAIL = { id: MODULE, name: "Delivery", link_module: [] };
const ESTIMATES = { estimates: [] };
const MODULE_ROWS: unknown[] = [];

/** Every module-owned read the area mounts, keyed for the failure message. */
function everyModuleRead() {
  return {
    list: modulesListQueryOptions(CLIENT, WS, PROJECT),
    detail: moduleDetailQueryOptions(CLIENT, WS, PROJECT, MODULE),
    archivedDetail: archivedModuleDetailQueryOptions(CLIENT, WS, PROJECT, MODULE),
    estimates: moduleSidebarEstimatesQueryOptions(CLIENT, WS, PROJECT),
    projectFacts: moduleProjectFactsQueryOptions(CLIENT, WS, PROJECT),
  };
}

/** One mounted observer over the read under test. */
function Probe({ read }: { read: { queryKey: readonly unknown[] } }): React.ReactElement {
  useQuery(read as never);
  return <span>read</span>;
}

function newClient(): QueryClient {
  // Noisy defaults: the read itself must supply the quiet triggers.
  return new QueryClient({
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: true, refetchOnReconnect: true, staleTime: 0 } },
  });
}

async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  mocks.listProjectModules.mockResolvedValue(MODULE_ROWS);
  mocks.getModule.mockResolvedValue(MODULE_DETAIL);
  mocks.getArchivedModule.mockResolvedValue(MODULE_DETAIL);
  mocks.listProjectEstimates.mockResolvedValue(ESTIMATES);
});

afterEach(() => {
  vi.clearAllMocks();
  vi.useRealTimers();
});

describe("module reads fetch on mount only (MOD-039)", () => {
  it("states the list freshness and all three quiet triggers on every read", () => {
    expect(projectEstimatesQueryOptions(CLIENT, WS, PROJECT).refetchOnWindowFocus).toBe(true);
    for (const [name, read] of Object.entries(everyModuleRead())) {
      expect(read.staleTime, `${name}.staleTime`).toBe(LIST_STALE_TIME_MS);
      expect(read.refetchOnWindowFocus, `${name}.refetchOnWindowFocus`).toBe(false);
      expect(read.refetchOnReconnect, `${name}.refetchOnReconnect`).toBe(false);
      expect(read.refetchInterval, `${name}.refetchInterval`).toBe(false);
    }
  });

  it("fetches each read once while focus, reconnect and time pass", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const client = newClient();
    for (const read of Object.values(everyModuleRead())) {
      render(
        <QueryClientProvider client={client}>
          <Probe read={read} />
        </QueryClientProvider>
      );
    }
    await settle();
    expect(mocks.listProjectModules).toHaveBeenCalledTimes(1);
    expect(mocks.getModule).toHaveBeenCalledTimes(1);
    expect(mocks.getArchivedModule).toHaveBeenCalledTimes(1);
    expect(mocks.listProjectEstimates).toHaveBeenCalledTimes(1);

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      window.dispatchEvent(new Event("online"));
      await vi.advanceTimersByTimeAsync(LIST_STALE_TIME_MS * 6);
    });
    await settle();
    expect(mocks.listProjectModules).toHaveBeenCalledTimes(1);
    expect(mocks.getModule).toHaveBeenCalledTimes(1);
    expect(mocks.getArchivedModule).toHaveBeenCalledTimes(1);
    expect(mocks.listProjectEstimates).toHaveBeenCalledTimes(1);
  });

  it("shares the estimates cache entry with the estimates screen without inheriting its focus refetch", async () => {
    const reads = everyModuleRead();
    expect(reads.estimates.queryKey).toEqual(projectEstimatesQueryOptions(CLIENT, WS, PROJECT).queryKey);
    const client = newClient();
    render(
      <QueryClientProvider client={client}>
        <Probe read={reads.estimates} />
      </QueryClientProvider>
    );
    await settle();
    const entry = client.getQueryCache().find({ queryKey: reads.estimates.queryKey });
    // The mounted, quieter observer wins while it is the only subscriber.
    expect(entry?.observers.length).toBe(1);
    expect(mocks.listProjectEstimates).toHaveBeenCalledTimes(1);
  });

  it("refreshes only the touched module's reads on the acting user's own write", async () => {
    const client = newClient();
    for (const read of Object.values(everyModuleRead())) {
      render(
        <QueryClientProvider client={client}>
          <Probe read={read} />
        </QueryClientProvider>
      );
    }
    await settle();
    mocks.listProjectModules.mockClear();
    mocks.getModule.mockClear();
    mocks.getArchivedModule.mockClear();

    await act(async () => {
      await invalidateAfterModuleWrite(client, WS, PROJECT, MODULE);
    });
    await settle();
    expect(mocks.listProjectModules).toHaveBeenCalledTimes(1);
    expect(mocks.getModule).toHaveBeenCalledTimes(1);
    // The archived detail is keyed under the archived list prefix, which
    // the write refreshes too; the module's own detail does not.
    expect(mocks.getArchivedModule).toHaveBeenCalledTimes(1);
  });
});

describe("project facts (MOD-039)", () => {
  it("reads through the module-read options without a focus refetch", async () => {
    const read = moduleProjectFactsQueryOptions(CLIENT, WS, PROJECT);
    expect(read.queryKey).toEqual(["ws", WS, "modules", "project-facts", PROJECT]);
    expect(read.refetchOnWindowFocus).toBe(false);
    expect(read.refetchInterval).toBe(false);
  });
});
