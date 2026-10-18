// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners reads (NEWFRONT-540, RUN-002/RUN-004). The list, the rail and the
// pods share key builders; the list polls on the short cadence, pods slower,
// and each scope narrows server-side.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ApiClient } from "@pidash/api-client";
import { createElement, type ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({ contractMocks: { listRunners: vi.fn(), listPods: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const {
  PODS_POLL_INTERVAL_MS,
  RUNNERS_POLL_INTERVAL_MS,
  runnersPodsQueryOptions,
  runnersQueryOptions,
  useRunners,
  useRunnersPods,
} = await import("./queries.js");
const { runnersKeys } = await import("./keys.js");

const client = {} as ApiClient;

afterEach(() => {
  contractMocks.listRunners.mockReset();
  contractMocks.listPods.mockReset();
  resetAppClient();
});

describe("runnersKeys", () => {
  it("namespaces both scopes inside the workspace subtree", () => {
    expect(runnersKeys.list("acme", null)).toEqual(["ws", "acme", "runners", "list", "workspace"]);
    expect(runnersKeys.list("acme", "p-1")).toEqual(["ws", "acme", "runners", "list", "p-1"]);
    expect(runnersKeys.pods("acme", null)).toEqual(["ws", "acme", "runners", "pods", "workspace"]);
    expect(runnersKeys.all("acme")).toEqual(["ws", "acme", "runners"]);
  });
});

describe("runnersQueryOptions", () => {
  it("reads the workspace aggregate and polls on the short cadence", async () => {
    contractMocks.listRunners.mockResolvedValue([]);
    const options = runnersQueryOptions(client, "acme", "ws-id", null);
    expect(options.queryKey).toEqual(runnersKeys.list("acme", null));
    expect(options.refetchInterval).toBe(RUNNERS_POLL_INTERVAL_MS);
    expect(options.staleTime).toBe(0);
    await options.queryFn?.({} as never);
    expect(contractMocks.listRunners).toHaveBeenCalledWith(client, "ws-id", {});
  });

  it("narrows to the project scope", async () => {
    contractMocks.listRunners.mockResolvedValue([]);
    const options = runnersQueryOptions(client, "acme", "ws-id", "p-1");
    await options.queryFn?.({} as never);
    expect(contractMocks.listRunners).toHaveBeenCalledWith(client, "ws-id", { projectId: "p-1" });
  });
});

describe("runnersPodsQueryOptions", () => {
  it("reads workspace pods on the slower cadence", async () => {
    contractMocks.listPods.mockResolvedValue([]);
    const options = runnersPodsQueryOptions(client, "acme", "ws-id", null);
    expect(options.queryKey).toEqual(runnersKeys.pods("acme", null));
    expect(options.refetchInterval).toBe(PODS_POLL_INTERVAL_MS);
    await options.queryFn?.({} as never);
    expect(contractMocks.listPods).toHaveBeenCalledWith(client, { workspaceId: "ws-id" });
  });

  it("passes only the project id when scoped", async () => {
    contractMocks.listPods.mockResolvedValue([]);
    const options = runnersPodsQueryOptions(client, "acme", "ws-id", "p-1");
    await options.queryFn?.({} as never);
    expect(contractMocks.listPods).toHaveBeenCalledWith(client, { projectId: "p-1" });
  });

  it("orders the list read before the pod read", () => {
    expect(RUNNERS_POLL_INTERVAL_MS).toBeLessThan(PODS_POLL_INTERVAL_MS);
  });
});

describe("useRunners / useRunnersPods", () => {
  it("mounts the list read and passes the enabled flag through", async () => {
    contractMocks.listRunners.mockResolvedValue([]);
    contractMocks.listPods.mockResolvedValue([]);
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    const workspaceList = renderHook(() => useRunners("acme", "ws-id", null), { wrapper });
    await waitFor(() => expect(workspaceList.result.current.isSuccess).toBe(true));
    expect(contractMocks.listRunners).toHaveBeenCalledWith(expect.anything(), "ws-id", {});

    const projectList = renderHook(() => useRunners("acme", "ws-id", "p-1"), { wrapper });
    await waitFor(() => expect(projectList.result.current.isSuccess).toBe(true));
    expect(contractMocks.listRunners).toHaveBeenCalledWith(expect.anything(), "ws-id", { projectId: "p-1" });

    const pods = renderHook(() => useRunnersPods("acme", "ws-id", null), { wrapper });
    await waitFor(() => expect(pods.result.current.isSuccess).toBe(true));
    expect(contractMocks.listPods).toHaveBeenCalledWith(expect.anything(), { workspaceId: "ws-id" });

    const disabled = renderHook(() => useRunners("acme", "ws-id", null, false), { wrapper });
    expect(disabled.result.current.fetchStatus).toBe("idle");
  });
});
