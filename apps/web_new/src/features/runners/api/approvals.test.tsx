// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-approval reads and the decide write (NEWFRONT-547,
// RUN-023/RUN-024). The queue key splits by scope, the queue polls on
// the fastest cadence, and deciding re-reads the area.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ApiClient, ApprovalRequest } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { listApprovals: vi.fn(), decideApproval: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { runnersKeys } = await import("./keys.js");
const { APPROVALS_POLL_INTERVAL_MS, approvalsQueryOptions, useApprovals, useDecideApproval } =
  await import("./approvals.js");

const client = {} as ApiClient;

const approval = (overrides: Partial<ApprovalRequest> = {}): ApprovalRequest =>
  ({ id: "ap-1", agent_run: "run-1", kind: "other", status: "pending", ...overrides }) as ApprovalRequest;

afterEach(() => {
  contractMocks.listApprovals.mockReset();
  contractMocks.decideApproval.mockReset();
  resetAppClient();
});

describe("cache keys", () => {
  it("separates the queue by scope inside the workspace subtree", () => {
    expect(runnersKeys.approvals("acme", null)).toEqual(["ws", "acme", "runners", "approvals", "workspace"]);
    expect(runnersKeys.approvals("acme", "p-1")).toEqual(["ws", "acme", "runners", "approvals", "p-1"]);
  });
});

describe("approvalsQueryOptions", () => {
  it("reads the workspace aggregate queue and polls fast", async () => {
    contractMocks.listApprovals.mockResolvedValue([approval()]);
    const options = approvalsQueryOptions(client, "acme", null);
    expect(options.queryKey).toEqual(runnersKeys.approvals("acme", null));
    expect(options.refetchInterval).toBe(APPROVALS_POLL_INTERVAL_MS);
    expect(options.staleTime).toBe(0);
    await options.queryFn?.({} as never);
    expect(contractMocks.listApprovals).toHaveBeenCalledWith(client, undefined);
  });

  it("narrows to the project scope", async () => {
    contractMocks.listApprovals.mockResolvedValue([]);
    const options = approvalsQueryOptions(client, "acme", "p-1");
    await options.queryFn?.({} as never);
    expect(contractMocks.listApprovals).toHaveBeenCalledWith(client, "p-1");
  });

  it("pins the two-second queue cadence", () => {
    expect(APPROVALS_POLL_INTERVAL_MS).toBe(2_000);
  });
});

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

describe("useApprovals", () => {
  it("mounts the queue read for the scope", async () => {
    contractMocks.listApprovals.mockResolvedValue([approval()]);
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const { wrapper } = harness();

    const queue = renderHook(() => useApprovals("acme", "p-1"), { wrapper });
    await waitFor(() => expect(queue.result.current.isSuccess).toBe(true));
    expect(contractMocks.listApprovals).toHaveBeenCalledWith(expect.anything(), "p-1");
  });
});

describe("useDecideApproval", () => {
  it("decides and re-reads the area so the card drops off", async () => {
    contractMocks.decideApproval.mockResolvedValue(approval({ status: "accepted" }));
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useDecideApproval("acme"), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ approvalId: "ap-1", decision: "accept" });
    });
    expect(contractMocks.decideApproval).toHaveBeenCalledWith(expect.anything(), "ap-1", "accept");
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") }));
  });

  it("rejects on failure so the caller keeps the card", async () => {
    contractMocks.decideApproval.mockRejectedValue(new Error("nope"));
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useDecideApproval("acme"), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync({ approvalId: "ap-1", decision: "decline" })).rejects.toThrow("nope");
    });
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") });
  });
});
