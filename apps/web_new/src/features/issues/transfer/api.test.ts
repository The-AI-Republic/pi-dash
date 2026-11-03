// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle progress read and transfer write for the completed-cycle prompt
// (ISS-224): the read asks the progress route and never refreshes on a
// window focus, the write posts the target and refreshes only the target
// plus the cycle picker.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const mocks = vi.hoisted(() => ({ getCycleProgress: vi.fn(), transfer: vi.fn(), success: vi.fn(), error: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  getCycleProgress: mocks.getCycleProgress,
  transferCycleIssues: mocks.transfer,
}));

vi.mock("../../../shared/shell/index.js", () => ({ notifyError: mocks.error, notifySuccess: mocks.success }));

import { cycleProgressQueryOptions, transferKeys, useTransferCycleIssues } from "./api.js";

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("cycleProgressQueryOptions", () => {
  it("reads the progress route under the workspace picker scope and stays off focus/reconnect", async () => {
    const counts = { total_issues: 2, backlog_issues: 2, unstarted_issues: 0, started_issues: 0 };
    mocks.getCycleProgress.mockResolvedValue(counts);
    const client = { stub: true } as unknown as ApiClient;
    const options = cycleProgressQueryOptions(client, "acme", "project-1", "cycle-1");
    expect(options.queryKey).toEqual(transferKeys.progress("acme", "project-1", "cycle-1"));
    expect(options.queryKey.slice(0, 2)).toEqual(["ws", "acme"]);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    const queryFn = options.queryFn as () => Promise<unknown>;
    await expect(queryFn()).resolves.toEqual(counts);
    expect(mocks.getCycleProgress).toHaveBeenCalledWith(client, "acme", "project-1", "cycle-1");
  });
});

describe("useTransferCycleIssues side effects", () => {
  it("refreshes the target and the picker, never the source, and notices both outcomes", async () => {
    createAppClient("https://api.test", (() => Promise.reject(new Error("no network"))) as never);
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useTransferCycleIssues("acme", "project-1", "source"), {
      wrapper: ({ children }: { children: React.ReactNode }) =>
        React.createElement(QueryClientProvider, { client: queryClient }, children),
    });
    mocks.transfer.mockResolvedValue({ message: "ok" });
    result.current.mutate("target");
    await waitFor(() => expect(invalidate).toHaveBeenCalled());
    const keys = invalidate.mock.calls.map((call) => (call[0] as { queryKey: readonly unknown[] }).queryKey);
    expect(keys).toContainEqual(transferKeys.progress("acme", "project-1", "target"));
    expect(keys).not.toContainEqual(transferKeys.progress("acme", "project-1", "source"));
    expect(keys.some((key) => key.includes("cycles"))).toBe(true);
    expect(mocks.success).toHaveBeenCalledTimes(1);
  });

  it("notices a refused move", async () => {
    createAppClient("https://api.test", (() => Promise.reject(new Error("no network"))) as never);
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const { result } = renderHook(() => useTransferCycleIssues("acme", "project-1", "source"), {
      wrapper: ({ children }: { children: React.ReactNode }) =>
        React.createElement(QueryClientProvider, { client: queryClient }, children),
    });
    mocks.transfer.mockRejectedValue(new Error("refused"));
    result.current.mutate("target");
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(mocks.error).toHaveBeenCalledTimes(1);
    expect(mocks.success).not.toHaveBeenCalled();
  });
});
