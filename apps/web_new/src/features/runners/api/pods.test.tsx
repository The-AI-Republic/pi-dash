// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod writers and the project picker read (NEWFRONT-542, RUN-013–015). Each
// write goes through the contract, refreshes the runners subtree on success
// and rejects so the dialog can toast and stay open; the picker only reads
// while its dialog is open.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { createPod: vi.fn(), updatePod: vi.fn(), deletePod: vi.fn(), listProjects: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { projectKeys } = await import("../../projects/index.js");

import { runnersKeys } from "./keys.js";
import { useCreatePod, useDeletePod, usePodProjects, useUpdatePod } from "./pods.js";

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
  for (const mock of Object.values(contractMocks)) mock.mockReset();
  resetAppClient();
});

describe("usePodProjects", () => {
  it("reads the workspace projects through the shared project key", async () => {
    contractMocks.listProjects.mockResolvedValue([{ id: "prj-1", name: "Web" }]);
    const { wrapper } = harness();
    const { result } = renderHook(() => usePodProjects("acme"), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.listProjects).toHaveBeenCalledWith(expect.anything(), "acme");
    expect(result.current.data).toEqual([{ id: "prj-1", name: "Web" }]);
    expect(projectKeys.list("acme")).toEqual(["ws", "acme", "projects", "list"]);
  });

  it("reports a failed read", async () => {
    contractMocks.listProjects.mockRejectedValue(new Error("down"));
    const { wrapper } = harness();
    const { result } = renderHook(() => usePodProjects("acme"), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });
});

describe("useCreatePod", () => {
  it("creates through the contract and refreshes the area", async () => {
    contractMocks.createPod.mockResolvedValue({ id: "pod-1" });
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useCreatePod("acme"), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ project: "prj-1", name: "beefy", description: "desc" });
    });
    expect(contractMocks.createPod).toHaveBeenCalledWith(expect.anything(), {
      project: "prj-1",
      name: "beefy",
      description: "desc",
    });
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") }));
  });

  it("rejects on failure without refreshing", async () => {
    contractMocks.createPod.mockRejectedValue(new Error("nope"));
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useCreatePod("acme"), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync({ project: "prj-1", name: "beefy" })).rejects.toThrow("nope");
    });
    expect(invalidate).not.toHaveBeenCalled();
  });
});

describe("useUpdatePod", () => {
  it("patches the given pod and refreshes the area", async () => {
    contractMocks.updatePod.mockResolvedValue({ id: "pod-1" });
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useUpdatePod("acme"), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ podId: "pod-1", patch: { is_default: true } });
    });
    expect(contractMocks.updatePod).toHaveBeenCalledWith(expect.anything(), "pod-1", { is_default: true });
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") }));
  });

  it("rejects on failure", async () => {
    contractMocks.updatePod.mockRejectedValue(new Error("conflict"));
    const { wrapper } = harness();
    const { result } = renderHook(() => useUpdatePod("acme"), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync({ podId: "pod-1", patch: { name: "x" } })).rejects.toThrow("conflict");
    });
  });
});

describe("useDeletePod", () => {
  it("deletes through the contract and refreshes the area", async () => {
    contractMocks.deletePod.mockResolvedValue(204);
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useDeletePod("acme"), { wrapper });
    await act(async () => {
      await result.current.mutateAsync("pod-1");
    });
    expect(contractMocks.deletePod).toHaveBeenCalledWith(expect.anything(), "pod-1");
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") }));
  });

  it("rejects when the server refuses", async () => {
    contractMocks.deletePod.mockRejectedValue(new Error("guard"));
    const { wrapper } = harness();
    const { result } = renderHook(() => useDeletePod("acme"), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync("pod-1")).rejects.toThrow("guard");
    });
  });
});
