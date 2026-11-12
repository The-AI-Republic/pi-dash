// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner row writers (NEWFRONT-540, RUN-010). Both actions call the
// contract, re-read the workspace's runner subtree on success, and reject
// so the caller can toast and keep its confirmation open.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({ contractMocks: { deleteRunner: vi.fn(), revokeRunner: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");

import { runnersKeys } from "./keys.js";
import { useDeleteRunner, useRevokeRunner } from "./mutations.js";

function harness() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <React.Fragment>{children}</React.Fragment>
    </QueryClientProvider>
  );
  return { invalidate, wrapper };
}

beforeEach(() => {
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  contractMocks.deleteRunner.mockReset();
  contractMocks.revokeRunner.mockReset();
  resetAppClient();
});

describe("useDeleteRunner", () => {
  it("deletes through the contract and re-reads the area", async () => {
    contractMocks.deleteRunner.mockResolvedValue(204);
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useDeleteRunner("acme"), { wrapper });
    await act(async () => {
      await result.current.mutateAsync("r-1");
    });
    expect(contractMocks.deleteRunner).toHaveBeenCalledWith(expect.anything(), "r-1");
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") }));
  });

  it("rejects on failure so the caller keeps its dialog", async () => {
    contractMocks.deleteRunner.mockRejectedValue(new Error("nope"));
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useDeleteRunner("acme"), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync("r-1")).rejects.toThrow("nope");
    });
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") });
  });
});

describe("useRevokeRunner", () => {
  it("revokes through the contract and re-reads the area", async () => {
    contractMocks.revokeRunner.mockResolvedValue({ id: "r-1" });
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useRevokeRunner("acme"), { wrapper });
    await act(async () => {
      await result.current.mutateAsync("r-1");
    });
    expect(contractMocks.revokeRunner).toHaveBeenCalledWith(expect.anything(), "r-1");
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") }));
  });

  it("rejects on failure", async () => {
    contractMocks.revokeRunner.mockRejectedValue(new Error("nope"));
    const { wrapper } = harness();
    const { result } = renderHook(() => useRevokeRunner("acme"), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync("r-1")).rejects.toThrow("nope");
    });
  });
});
