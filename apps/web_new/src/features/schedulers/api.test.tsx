// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError, type Scheduler } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts } = vi.hoisted(() => ({
  contracts: {
    createScheduler: vi.fn(),
    deleteScheduler: vi.fn(),
    listSchedulers: vi.fn(),
    updateScheduler: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

vi.mock("../../shared/shell/index.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import {
  schedulerFailureMessage,
  schedulerFieldError,
  schedulerKeys,
  schedulersQueryOptions,
  useCreateScheduler,
  useDeleteScheduler,
  useSchedulers,
  useUpdateScheduler,
} from "./api.js";

const SLUG = "acme";

function scheduler(): Scheduler {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    workspace: "223e4567-e89b-12d3-a456-426614174000",
    slug: "weekly-report",
    name: "Weekly report",
    description: "Summary",
    prompt: "Review the week",
    color: "#5e6ad2",
    source: "builtin",
    is_enabled: true,
    active_binding_count: 2,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
  };
}

function setup<T>(hook: () => T) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const wrapper: React.ComponentType<{ children?: unknown }> = ({ children }) => (
    <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>
  );
  return { ...renderHook(hook, { wrapper }), queryClient, invalidate };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("scheduler data", () => {
  it("uses one workspace-scoped list key and contract", async () => {
    contracts.listSchedulers.mockResolvedValue([scheduler()]);
    const options = schedulersQueryOptions({} as never, SLUG);
    expect(options.queryKey).toEqual(["ws", SLUG, "schedulers", "list"]);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([scheduler()]);
    expect(contracts.listSchedulers).toHaveBeenCalledWith(expect.anything(), SLUG);
    expect(schedulerKeys.all(SLUG)).toEqual(["ws", SLUG, "schedulers"]);
  });

  it("reads the list through the connected hook", async () => {
    contracts.listSchedulers.mockResolvedValue([scheduler()]);
    const { result } = setup(() => useSchedulers(SLUG));
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual([scheduler()]);
  });

  it("normalizes field and notice details", () => {
    const detailed = new ApiError({
      status: 400,
      code: "http",
      message: "Rejected",
      fields: { slug: ["Already used.", "Pick another."], name: [] },
    });
    expect(schedulerFailureMessage(detailed)).toBe("Rejected");
    expect(schedulerFieldError(detailed, "slug")).toBe("Already used. Pick another.");
    expect(schedulerFieldError(detailed, "name")).toBeUndefined();
    expect(schedulerFieldError(detailed, "prompt")).toBeUndefined();
    expect(schedulerFieldError(new Error("plain"), "slug")).toBeUndefined();
    expect(schedulerFailureMessage(new ApiError({ status: 500, code: "http", message: " " }))).toBe(
      "Check your connection and try again."
    );
    expect(schedulerFailureMessage(new Error("plain"))).toBe("Check your connection and try again.");
  });

  it("creates, invalidates, and reports success", async () => {
    contracts.createScheduler.mockResolvedValue(scheduler());
    const { result, invalidate } = setup(() => useCreateScheduler(SLUG));
    await act(() => result.current.mutateAsync({ name: "Weekly report", slug: "weekly-report", prompt: "Review" }));
    expect(contracts.createScheduler).toHaveBeenCalledWith(
      expect.anything(),
      SLUG,
      expect.objectContaining({
        slug: "weekly-report",
      })
    );
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulerKeys.list(SLUG) });
    expect(notifySuccess).toHaveBeenCalledWith("Scheduler created", expect.any(String));
  });

  it("updates without a handle and invalidates", async () => {
    contracts.updateScheduler.mockResolvedValue(scheduler());
    const { result, invalidate } = setup(() => useUpdateScheduler(SLUG));
    await act(() => result.current.mutateAsync({ schedulerId: "s1", input: { name: "Renamed" } }));
    expect(contracts.updateScheduler).toHaveBeenCalledWith(expect.anything(), SLUG, "s1", { name: "Renamed" });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulerKeys.list(SLUG) });
    expect(notifySuccess).toHaveBeenCalledWith("Scheduler updated");
  });

  it("deletes, invalidates, and reports the freed handle", async () => {
    contracts.deleteScheduler.mockResolvedValue(204);
    const { result, invalidate } = setup(() => useDeleteScheduler(SLUG));
    await act(() => result.current.mutateAsync("s1"));
    expect(contracts.deleteScheduler).toHaveBeenCalledWith(expect.anything(), SLUG, "s1");
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulerKeys.list(SLUG) });
    expect(notifySuccess).toHaveBeenCalledWith("Scheduler deleted", expect.stringContaining("handle"));
  });

  it.each([
    ["create", useCreateScheduler, contracts.createScheduler, { name: "Name", slug: "name", prompt: "Prompt" }],
    ["update", useUpdateScheduler, contracts.updateScheduler, { schedulerId: "s1", input: { name: "Name" } }],
    ["delete", useDeleteScheduler, contracts.deleteScheduler, "s1"],
  ] as const)("notices on %s failure", async (_label, useHook, contract, input) => {
    contract.mockRejectedValue(new ApiError({ status: 500, code: "http", message: "Server detail" }));
    const { result } = setup(() => useHook(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync(input as never)).rejects.toThrow("Server detail");
    });
    expect(notifyError).toHaveBeenCalledWith(expect.stringContaining("Could not"), "Server detail");
  });
});
