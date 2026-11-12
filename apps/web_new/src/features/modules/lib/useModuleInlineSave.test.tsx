// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row and card inline writes (parity/specs/modules.md §4, MOD-013/014):
// dates PATCH both sides (empty sides included), a status pick PATCHes
// the lifecycle value, every write paints the list cache first and rolls
// it back with the pinned error notice on refusal.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  updateModule: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifySuccess: mocks.notifySuccess,
  notifyError: mocks.notifyError,
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  updateModule: mocks.updateModule,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { moduleKeys } from "../api/keys.js";
import { makeModuleRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../testing.js";
import { applyModuleInlinePatch, useModuleInlineSave } from "./useModuleInlineSave.js";

const SLUG = "acme";

function mount() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const wrapper = ({ children }: { children: React.ReactNode }): React.ReactElement =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const view = renderHook(() => useModuleInlineSave({ workspaceSlug: SLUG, projectId: MODULES_PROJECT_ID }), {
    wrapper,
  });
  return { ...view, queryClient };
}

function seedList(queryClient: QueryClient, row: ModuleRow): void {
  queryClient.setQueryData(moduleKeys.list(SLUG, MODULES_PROJECT_ID), [row]);
}

function cachedRow(queryClient: QueryClient): ModuleRow {
  const rows = queryClient.getQueryData<ModuleRow[]>(moduleKeys.list(SLUG, MODULES_PROJECT_ID));
  if (rows === undefined || rows[0] === undefined) throw new Error("expected a cached module row");
  return rows[0];
}

/** Hold the PATCH until the test proves the optimistic paint, then settle it. */
function gateWrite(): { release: () => void; fail: (error: unknown) => void } {
  let release!: () => void;
  let fail!: (error: unknown) => void;
  mocks.updateModule.mockImplementationOnce(
    () =>
      new Promise<unknown>((resolve, reject) => {
        release = () => resolve({ id: MODULES_MODULE_ID });
        fail = reject;
      })
  );
  return {
    release: () => release(),
    fail: (error: unknown) => fail(error),
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.updateModule.mockResolvedValue({ id: MODULES_MODULE_ID });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("applyModuleInlinePatch", () => {
  const row = makeModuleRow();

  it("leaves an absent cache absent", () => {
    expect(applyModuleInlinePatch(undefined, MODULES_MODULE_ID, { status: "planned" })).toBeUndefined();
  });

  it("touches only the matching row", () => {
    const other = makeModuleRow({ id: "623e4567-e89b-12d3-a456-426614174005", status: "backlog" });
    const next = applyModuleInlinePatch([row, other], MODULES_MODULE_ID, { status: "planned" });
    expect(next?.[0]?.status).toBe("planned");
    expect(next?.[1]).toBe(other);
  });

  it("merges the patch over the row", () => {
    const next = applyModuleInlinePatch([row], MODULES_MODULE_ID, {
      start_date: "2026-11-04",
      target_date: null,
    });
    expect(next?.[0]).toMatchObject({ start_date: "2026-11-04", target_date: null, status: "backlog" });
  });
});

describe("useModuleInlineSave", () => {
  it("saves both date sides at once with the pinned notice", async () => {
    const { result, queryClient } = mount();
    seedList(queryClient, makeModuleRow());
    const write = gateWrite();

    act(() => {
      result.current.saveDates(MODULES_MODULE_ID, { startDate: "2026-11-04", targetDate: null });
    });
    // The list cache paints before the write lands.
    await waitFor(() => {
      expect(cachedRow(queryClient)).toMatchObject({ start_date: "2026-11-04", target_date: null });
    });

    await act(async () => {
      write.release();
    });
    await waitFor(() => {
      expect(mocks.notifySuccess).toHaveBeenCalledTimes(1);
    });
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), SLUG, MODULES_PROJECT_ID, MODULES_MODULE_ID, {
      start_date: "2026-11-04",
      target_date: null,
    });
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", "Module updated successfully.");
    expect(mocks.notifyError).not.toHaveBeenCalled();
    expect(cachedRow(queryClient)).toMatchObject({ start_date: "2026-11-04", target_date: null });
  });

  it("saves a status pick with the pinned notice", async () => {
    const { result, queryClient } = mount();
    seedList(queryClient, makeModuleRow({ status: "backlog" }));
    const write = gateWrite();

    act(() => {
      result.current.saveStatus(MODULES_MODULE_ID, "in-progress");
    });
    await waitFor(() => {
      expect(cachedRow(queryClient).status).toBe("in-progress");
    });

    await act(async () => {
      write.release();
    });
    await waitFor(() => {
      expect(mocks.notifySuccess).toHaveBeenCalledTimes(1);
    });
    expect(mocks.updateModule).toHaveBeenCalledWith(expect.anything(), SLUG, MODULES_PROJECT_ID, MODULES_MODULE_ID, {
      status: "in-progress",
    });
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Success!", "Module updated successfully.");
  });

  it("rolls the cache back with an error notice when the server refuses", async () => {
    const { result, queryClient } = mount();
    seedList(queryClient, makeModuleRow({ status: "backlog" }));
    const write = gateWrite();

    act(() => {
      result.current.saveStatus(MODULES_MODULE_ID, "planned");
    });
    await waitFor(() => {
      expect(cachedRow(queryClient).status).toBe("planned");
    });

    await act(async () => {
      write.fail(new Error("start_date must not be later than target_date"));
    });
    await waitFor(() => {
      expect(mocks.notifyError).toHaveBeenCalledTimes(1);
    });
    expect(mocks.notifyError).toHaveBeenCalledWith("Error!", "start_date must not be later than target_date");
    expect(mocks.notifySuccess).not.toHaveBeenCalled();
    expect(cachedRow(queryClient).status).toBe("backlog");
  });
});
