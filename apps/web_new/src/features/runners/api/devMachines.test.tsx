// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machine reads and writes (NEWFRONT-555, RUN-037–040).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import { createElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: {
    listDevMachines: vi.fn(),
    rotateDevMachine: vi.fn(),
    revokeDevMachine: vi.fn(),
    deleteDevMachine: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const {
  DEV_MACHINES_POLL_INTERVAL_MS,
  devMachinesQueryOptions,
  useDeleteDevMachine,
  useDevMachines,
  useRevokeDevMachine,
  useRotateDevMachine,
} = await import("./devMachines.js");
const { runnersKeys } = await import("./keys.js");

const client = { id: "client" } as unknown as ApiClient;

function wrapper(queryClient: QueryClient) {
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
}

beforeEach(() => {
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  for (const mock of Object.values(contractMocks)) mock.mockReset();
  resetAppClient();
});

describe("devMachinesQueryOptions", () => {
  it("keys inside the runners subtree and polls on the short cadence", async () => {
    contractMocks.listDevMachines.mockResolvedValue([]);
    const options = devMachinesQueryOptions(client, "acme", "ws-id");
    expect(options.queryKey).toEqual(runnersKeys.devMachines("acme"));
    expect(options.refetchInterval).toBe(DEV_MACHINES_POLL_INTERVAL_MS);
    expect(options.staleTime).toBe(0);
    // A 500 must reach the error UI; the poll is the recovery path.
    expect(options.retry).toBe(false);
    await options.queryFn?.({} as never);
    expect(contractMocks.listDevMachines).toHaveBeenCalledWith(client, "ws-id");
  });
});

describe("useDevMachines", () => {
  it("reads the workspace machines through the hook", async () => {
    contractMocks.listDevMachines.mockResolvedValue([{ id: "m-1" }]);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useDevMachines("acme", "ws-id"), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current.data).toEqual([{ id: "m-1" }]));
    expect(contractMocks.listDevMachines).toHaveBeenCalledWith(expect.anything(), "ws-id");
  });

  it("waits for the workspace id and honours the enabled flag", () => {
    const queryClient = new QueryClient();
    renderHook(() => useDevMachines("acme", undefined), { wrapper: wrapper(queryClient) });
    renderHook(() => useDevMachines("acme", "ws-id", false), { wrapper: wrapper(queryClient) });
    expect(contractMocks.listDevMachines).not.toHaveBeenCalled();
  });
});

describe("dev machine writers", () => {
  it("writes through the workspace-scoped id and invalidates the subtree", async () => {
    contractMocks.rotateDevMachine.mockResolvedValue({ id: "m-1" });
    contractMocks.revokeDevMachine.mockResolvedValue({ id: "m-1" });
    contractMocks.deleteDevMachine.mockResolvedValue(204);
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(
      () => ({
        rotate: useRotateDevMachine("acme", "ws-id"),
        revoke: useRevokeDevMachine("acme", "ws-id"),
        remove: useDeleteDevMachine("acme", "ws-id"),
      }),
      { wrapper: wrapper(queryClient) }
    );
    await result.current.rotate.mutateAsync("m-1");
    await result.current.revoke.mutateAsync("m-1");
    await result.current.remove.mutateAsync("m-1");
    expect(contractMocks.rotateDevMachine).toHaveBeenCalledWith(expect.anything(), "m-1", "ws-id");
    expect(contractMocks.revokeDevMachine).toHaveBeenCalledWith(expect.anything(), "m-1", "ws-id");
    expect(contractMocks.deleteDevMachine).toHaveBeenCalledWith(expect.anything(), "m-1", "ws-id");
    expect(invalidate).toHaveBeenCalledTimes(3);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") });
  });

  it("rejects on a failed write so the caller can toast", async () => {
    contractMocks.deleteDevMachine.mockRejectedValue(new Error("nope"));
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const { result } = renderHook(() => useDeleteDevMachine("acme", "ws-id"), { wrapper: wrapper(queryClient) });
    await expect(result.current.mutateAsync("m-1")).rejects.toThrow("nope");
    await waitFor(() => expect(result.current.isError).toBe(true));
  });
});
