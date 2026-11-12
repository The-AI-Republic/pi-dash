// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Single-runner read (NEWFRONT-555, RUN-042).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import { createElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({ contractMocks: { getRunner: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { RUNNER_DETAIL_POLL_INTERVAL_MS, runnerDetailQueryOptions, useRunnerDetail } = await import("./detail.js");
const { runnersKeys } = await import("./keys.js");

const client = {} as ApiClient;

function wrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

beforeEach(() => {
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  contractMocks.getRunner.mockReset();
  resetAppClient();
});

describe("runner detail read", () => {
  it("keys per runner and polls on the short cadence", async () => {
    contractMocks.getRunner.mockResolvedValue({ id: "r-1" });
    const options = runnerDetailQueryOptions(client, "acme", "r-1");
    expect(options.queryKey).toEqual(runnersKeys.runnerDetail("acme", "r-1"));
    expect(options.refetchInterval).toBe(RUNNER_DETAIL_POLL_INTERVAL_MS);
    expect(options.retry).toBe(false);
    await options.queryFn?.({} as never);
    expect(contractMocks.getRunner).toHaveBeenCalledWith(client, "r-1");
  });

  it("serves the runner through the hook", async () => {
    contractMocks.getRunner.mockResolvedValue({ id: "r-1" });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useRunnerDetail("acme", "r-1"), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current.data).toEqual({ id: "r-1" }));
    expect(contractMocks.getRunner).toHaveBeenCalledWith(expect.anything(), "r-1");
  });

  it("does not read when disabled", () => {
    const queryClient = new QueryClient();
    renderHook(() => useRunnerDetail("acme", "r-1", false), { wrapper: wrapper(queryClient) });
    expect(contractMocks.getRunner).not.toHaveBeenCalled();
  });
});
