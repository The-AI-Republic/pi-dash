// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules timeline writes (parity/specs/modules.md §6, MOD-021): bar moves
// persist the new dates, reorders persist normalized ranks for the rows
// that move, every write paints the list first and rolls it back with an
// error notice on refusal, and a no-op drop writes nothing at all.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ updateModule: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, updateModule: mocks.updateModule };
});

vi.mock("../../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn() }));

import { notifyError } from "../../../../shared/shell/toaster.js";
import { moduleKeys } from "../../api/keys.js";
import { makeModuleRow, MODULES_PROJECT_ID } from "../../testing.js";
import { MODULE_TIMELINE_REORDER_FAILED_BODY, MODULE_TIMELINE_REORDER_FAILED_TITLE } from "./moduleTimelineModel.js";
import { useModuleTimelineMutations } from "./useModuleTimelineMutations.js";

interface PatchCall {
  moduleId: string;
  patch: unknown;
}

function setup(update: (moduleId: string, patch: unknown) => Promise<unknown>) {
  mocks.updateModule.mockImplementation(
    async (_client: unknown, _slug: string, _projectId: string, moduleId: string, patch: unknown): Promise<unknown> =>
      update(moduleId, patch)
  );
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const rows = [
    makeModuleRow({
      id: "m-1",
      name: "Alpha",
      start_date: "2026-09-01",
      target_date: "2026-09-03",
      sort_order: 0,
    }),
    makeModuleRow({ id: "m-2", name: "Bravo", sort_order: 0 }),
    makeModuleRow({ id: "m-3", name: "Charlie", sort_order: 0 }),
  ];
  queryClient.setQueryData(moduleKeys.list("acme", MODULES_PROJECT_ID), rows);
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(
    () => useModuleTimelineMutations({ client: {} as never, workspaceSlug: "acme", projectId: MODULES_PROJECT_ID }),
    { wrapper }
  );
  return { result, queryClient };
}

function listRows(queryClient: QueryClient): ReturnType<typeof makeModuleRow>[] {
  return queryClient.getQueryData(moduleKeys.list("acme", MODULES_PROJECT_ID)) as ReturnType<typeof makeModuleRow>[];
}

describe("useModuleTimelineMutations moveBar", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("paints the new dates, then persists both ends in one PATCH", async () => {
    const calls: PatchCall[] = [];
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { result, queryClient } = setup(async (moduleId, patch) => {
      calls.push({ moduleId, patch });
      await gate;
      return {};
    });
    act(() => {
      result.current.moveBar("m-1", { startDate: "2026-09-04", endDate: "2026-09-06" });
    });
    // The paint lands before the write resolves.
    await waitFor(() =>
      expect(listRows(queryClient)[0]).toMatchObject({ start_date: "2026-09-04", target_date: "2026-09-06" })
    );
    await act(async () => {
      release();
    });
    await waitFor(() =>
      expect(calls).toEqual([{ moduleId: "m-1", patch: { start_date: "2026-09-04", target_date: "2026-09-06" } }])
    );
    await waitFor(() =>
      expect(queryClient.getQueryState(moduleKeys.list("acme", MODULES_PROJECT_ID))?.isInvalidated).toBe(true)
    );
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("rolls the dates back and reports the server sentence on refusal", async () => {
    const { result, queryClient } = setup(async () => {
      throw new ApiError({ code: "http_error", message: "The module is archived.", status: 400 });
    });
    await act(async () => {
      result.current.moveBar("m-1", { startDate: "2026-09-04", endDate: "2026-09-06" });
    });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't update the module", "The module is archived.")
    );
    expect(listRows(queryClient)[0]).toMatchObject({ start_date: "2026-09-01", target_date: "2026-09-03" });
  });
});

describe("useModuleTimelineMutations reorder", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("normalizes every moved rank and paints the new order first", async () => {
    const calls: PatchCall[] = [];
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { result, queryClient } = setup(async (moduleId, patch) => {
      calls.push({ moduleId, patch });
      await gate;
      return {};
    });
    act(() => {
      result.current.reorder(listRows(queryClient), "m-3", "m-1", "before");
    });
    // Fresh modules share rank 0, so the whole visible order normalizes.
    await waitFor(() =>
      expect(listRows(queryClient).map((row) => [row.id, row.sort_order])).toEqual([
        ["m-1", 2000],
        ["m-2", 3000],
        ["m-3", 1000],
      ])
    );
    await act(async () => {
      release();
    });
    await waitFor(() => expect(calls).toHaveLength(3));
    expect(calls).toEqual([
      { moduleId: "m-1", patch: { sort_order: 2000 } },
      { moduleId: "m-2", patch: { sort_order: 3000 } },
      { moduleId: "m-3", patch: { sort_order: 1000 } },
    ]);
    await waitFor(() =>
      expect(queryClient.getQueryState(moduleKeys.list("acme", MODULES_PROJECT_ID))?.isInvalidated).toBe(true)
    );
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("patches only the rows whose rank moves", async () => {
    const calls: PatchCall[] = [];
    const { result, queryClient } = setup(async (moduleId, patch) => {
      calls.push({ moduleId, patch });
      return {};
    });
    act(() => {
      queryClient.setQueryData(
        moduleKeys.list("acme", MODULES_PROJECT_ID),
        listRows(queryClient).map((row, index) => ({ ...row, sort_order: (index + 1) * 1000 }))
      );
    });
    await act(async () => {
      // Bravo ahead of Alpha renanks only the two swapped rows; Charlie's
      // last-place rank already reads 3000, so it keeps it.
      result.current.reorder(listRows(queryClient), "m-2", "m-1", "before");
    });
    await waitFor(() => expect(calls).toHaveLength(2));
    expect(calls).toEqual([
      { moduleId: "m-1", patch: { sort_order: 2000 } },
      { moduleId: "m-2", patch: { sort_order: 1000 } },
    ]);
  });

  it("rolls the ranks back and reports the refusal", async () => {
    const { result, queryClient } = setup(async () => {
      throw new ApiError({ code: "http_error", message: "Locked.", status: 400 });
    });
    await act(async () => {
      result.current.reorder(listRows(queryClient), "m-3", "m-1", "before");
    });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(
        MODULE_TIMELINE_REORDER_FAILED_TITLE,
        MODULE_TIMELINE_REORDER_FAILED_BODY
      )
    );
    expect(listRows(queryClient).map((row) => row.sort_order)).toEqual([0, 0, 0]);
  });

  it("writes nothing for a drop onto the dragged row itself", async () => {
    const calls: PatchCall[] = [];
    const { result, queryClient } = setup(async (moduleId, patch) => {
      calls.push({ moduleId, patch });
      return {};
    });
    await act(async () => {
      result.current.reorder(listRows(queryClient), "m-1", "m-1", "before");
    });
    expect(calls).toEqual([]);
    expect(notifyError).not.toHaveBeenCalled();
    expect(queryClient.getQueryState(moduleKeys.list("acme", MODULES_PROJECT_ID))?.isInvalidated).toBe(false);
  });
});
