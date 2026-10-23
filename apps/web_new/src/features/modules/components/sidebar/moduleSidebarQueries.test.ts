// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ModuleDetail } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getModule: vi.fn(),
  getArchivedModule: vi.fn(),
  listProjectEstimates: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    getModule: mocks.getModule,
    getArchivedModule: mocks.getArchivedModule,
    listProjectEstimates: mocks.listProjectEstimates,
  };
});

import { LIST_STALE_TIME_MS } from "../../../../core/query/client.js";
import { estimatesKeys } from "../../../estimates/index.js";
import { moduleKeys } from "../../api/keys.js";
import {
  archivedModuleDetailQueryOptions,
  moduleDetailQueryOptions,
  moduleSidebarDetailQueryOptions,
  moduleSidebarEstimatesQueryOptions,
} from "./moduleSidebarQueries.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";

const detail = { id: MODULE, name: "Delivery" } as ModuleDetail;

describe("moduleSidebarQueries", () => {
  it("reads the active module under the shared detail key without focus refetch", async () => {
    mocks.getModule.mockResolvedValue(detail);
    const client = { stub: true } as never;
    const options = moduleDetailQueryOptions(client, "acme", PROJECT, MODULE);
    expect(options.queryKey).toEqual(moduleKeys.detail("acme", PROJECT, MODULE));
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(options.refetchOnWindowFocus).toBe(false);
    await expect(options.queryFn?.({} as never)).resolves.toBe(detail);
    expect(mocks.getModule).toHaveBeenCalledWith(client, "acme", PROJECT, MODULE);
  });

  it("reads an archived module through the archived detail endpoint and key", async () => {
    mocks.getArchivedModule.mockResolvedValue(detail);
    const options = archivedModuleDetailQueryOptions({} as never, "acme", PROJECT, MODULE);
    expect(options.queryKey).toEqual(moduleKeys.archivedDetail("acme", PROJECT, MODULE));
    await options.queryFn?.({} as never);
    expect(mocks.getArchivedModule).toHaveBeenCalledWith({}, "acme", PROJECT, MODULE);
  });

  it("selects the active or archived read from the sidebar's one factory", async () => {
    mocks.getModule.mockResolvedValue(detail);
    mocks.getArchivedModule.mockResolvedValue(detail);
    const active = moduleSidebarDetailQueryOptions({} as never, "acme", PROJECT, MODULE, false);
    expect(active.queryKey).toEqual(moduleKeys.detail("acme", PROJECT, MODULE));
    await active.queryFn?.({} as never);
    expect(mocks.getModule).toHaveBeenCalled();

    const archived = moduleSidebarDetailQueryOptions({} as never, "acme", PROJECT, MODULE, true);
    expect(archived.queryKey).toEqual(moduleKeys.archivedDetail("acme", PROJECT, MODULE));
    await archived.queryFn?.({} as never);
    expect(mocks.getArchivedModule).toHaveBeenCalled();
  });

  it("reuses the estimates feature read for the points gate", async () => {
    const options = moduleSidebarEstimatesQueryOptions({} as never, "acme", PROJECT);
    expect(options.queryKey).toEqual(estimatesKeys.project("acme", PROJECT));
    expect(options.staleTime).toBe(0);
    expect(options.refetchOnWindowFocus).toBe(true);
  });
});
