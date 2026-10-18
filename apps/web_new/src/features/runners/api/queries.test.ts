// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners reads (NEWFRONT-540, RUN-002/RUN-004). The list, the rail and the
// pods share key builders; the list polls on the short cadence, pods slower,
// and each scope narrows server-side.
import type { ApiClient } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({ contractMocks: { listRunners: vi.fn(), listPods: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

const { resetAppClient } = await import("../../../core/api/client.js");
const { PODS_POLL_INTERVAL_MS, RUNNERS_POLL_INTERVAL_MS, runnersPodsQueryOptions, runnersQueryOptions } =
  await import("./queries.js");
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
