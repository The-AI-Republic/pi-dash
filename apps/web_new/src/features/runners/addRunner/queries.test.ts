// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-runner reads (RUN-006/RUN-007): machine polling, scoped projects and
// pods, and the create/status transport wiring.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const contractMocks = vi.hoisted(() => ({
  listDevMachines: vi.fn(),
  listProjects: vi.fn(),
  listPods: vi.fn(),
  createRunnerOnMachine: vi.fn(),
  getCreateRunnerStatus: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import type { ApiClient } from "@pidash/api-client";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  addRunnerKeys,
  createRunnerTransport,
  machinesQueryOptions,
  podsForRunnerQueryOptions,
  projectsForRunnerQueryOptions,
  useAddRunnerMachines,
  useAddRunnerPods,
  useAddRunnerProjects,
} from "./queries.js";
import { MACHINE_REFRESH_MS } from "./model.js";

const client = {} as ApiClient;

afterEach(() => {
  for (const mock of Object.values(contractMocks)) mock.mockReset();
  resetAppClient();
});

describe("addRunnerKeys", () => {
  it("nests machines inside the workspace runners subtree", () => {
    expect(addRunnerKeys.machines("acme", "ws-id")).toEqual(["ws", "acme", "runners", "dev-machines", "ws-id"]);
  });
});

describe("machinesQueryOptions", () => {
  it("poll while open and read the workspace's machines", async () => {
    contractMocks.listDevMachines.mockResolvedValue([]);
    const options = machinesQueryOptions(client, "acme", "ws-id");
    expect(options.queryKey).toEqual(addRunnerKeys.machines("acme", "ws-id"));
    expect(options.refetchInterval).toBe(MACHINE_REFRESH_MS);
    await options.queryFn?.({} as never);
    expect(contractMocks.listDevMachines).toHaveBeenCalledWith(client, "ws-id");
  });
});

describe("projectsForRunnerQueryOptions", () => {
  it("reads the workspace project list", async () => {
    contractMocks.listProjects.mockResolvedValue([]);
    const options = projectsForRunnerQueryOptions(client, "acme");
    await options.queryFn?.({} as never);
    expect(contractMocks.listProjects).toHaveBeenCalledWith(client, "acme");
  });
});

describe("podsForRunnerQueryOptions", () => {
  it("scopes by project when one is given", async () => {
    contractMocks.listPods.mockResolvedValue([]);
    const options = podsForRunnerQueryOptions(client, "acme", "ws-id", "p-1");
    await options.queryFn?.({} as never);
    expect(contractMocks.listPods).toHaveBeenCalledWith(client, { projectId: "p-1" });
  });

  it("falls back to the workspace on the aggregate scope", async () => {
    contractMocks.listPods.mockResolvedValue([]);
    const options = podsForRunnerQueryOptions(client, "acme", "ws-id", null);
    await options.queryFn?.({} as never);
    expect(contractMocks.listPods).toHaveBeenCalledWith(client, { workspaceId: "ws-id" });
  });
});

describe("hook wrappers", () => {
  it("query the workspace while enabled", async () => {
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: React.ReactNode }) =>
      React.createElement(QueryClientProvider, { client: queryClient }, children);
    contractMocks.listDevMachines.mockResolvedValue([]);
    const machines = renderHook(() => useAddRunnerMachines("acme", "ws-id", true), { wrapper });
    await vi.waitFor(() => expect(machines.result.current.data).toEqual([]));
    expect(contractMocks.listDevMachines).toHaveBeenCalledWith(expect.anything(), "ws-id");
    machines.unmount();

    contractMocks.listProjects.mockResolvedValue([]);
    const projects = renderHook(() => useAddRunnerProjects("acme", true), { wrapper });
    await vi.waitFor(() => expect(projects.result.current.data).toEqual([]));
    expect(contractMocks.listProjects).toHaveBeenCalledWith(expect.anything(), "acme");
    projects.unmount();

    contractMocks.listPods.mockResolvedValue([]);
    const pods = renderHook(() => useAddRunnerPods("acme", "ws-id", null, true), { wrapper });
    await vi.waitFor(() => expect(pods.result.current.data).toEqual([]));
    expect(contractMocks.listPods).toHaveBeenCalledWith(expect.anything(), { workspaceId: "ws-id" });
    pods.unmount();
  });
});

describe("createRunnerTransport", () => {
  it("passes the machine, input and workspace through", async () => {
    const appClient = createAppClient("https://api.test", () => new Promise<never>(() => {}));
    contractMocks.createRunnerOnMachine.mockResolvedValue({ request_id: "req" });
    contractMocks.getCreateRunnerStatus.mockResolvedValue({ request_id: "req", status: "pending" });
    const transport = createRunnerTransport();
    await transport.createRunner("m-1", { workspace: "ws", project: "ACME" });
    expect(contractMocks.createRunnerOnMachine).toHaveBeenCalledWith(appClient, "m-1", {
      workspace: "ws",
      project: "ACME",
    });
    await transport.readStatus("m-1", "req", "ws");
    expect(contractMocks.getCreateRunnerStatus).toHaveBeenCalledWith(appClient, "m-1", "req", "ws");
  });
});
