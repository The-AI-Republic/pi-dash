// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail read and transfer write for the completed-cycle prompt
// (ISS-224): the read asks the detail route and never refreshes on a
// window focus, the write posts the target and refreshes only the target
// plus the cycle picker.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const mocks = vi.hoisted(() => ({ getCycle: vi.fn(), transfer: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  getCycle: mocks.getCycle,
  transferCycleIssues: mocks.transfer,
}));

import { cycleDetailQueryOptions, transferKeys, useTransferCycleIssues } from "./api.js";

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("cycleDetailQueryOptions", () => {
  it("reads the cycle detail under the workspace picker scope and stays off focus/reconnect", async () => {
    mocks.getCycle.mockResolvedValue({ id: "cycle-1", name: "Sprint" });
    const client = { stub: true } as unknown as ApiClient;
    const options = cycleDetailQueryOptions(client, "acme", "project-1", "cycle-1");
    expect(options.queryKey).toEqual(transferKeys.cycle("acme", "project-1", "cycle-1"));
    expect(options.queryKey.slice(0, 2)).toEqual(["ws", "acme"]);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    const queryFn = options.queryFn as () => Promise<unknown>;
    await expect(queryFn()).resolves.toMatchObject({ id: "cycle-1" });
    expect(mocks.getCycle).toHaveBeenCalledWith(client, "acme", "project-1", "cycle-1");
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
    expect(keys).toContainEqual(transferKeys.cycle("acme", "project-1", "target"));
    expect(keys).not.toContainEqual(transferKeys.cycle("acme", "project-1", "source"));
    expect(keys.some((key) => key.includes("cycles"))).toBe(true);
  });
});
