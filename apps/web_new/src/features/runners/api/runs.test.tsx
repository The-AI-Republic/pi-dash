// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs reads and cancel (NEWFRONT-545, RUN-016–019/RUN-021). The list key
// carries the scope and page, the detail polls only while live, and the
// cancel write refreshes the run and every list page.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { AgentRun, AgentRunPage, ApiClient } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { listRuns: vi.fn(), getRun: vi.fn(), cancelRun: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { runnersKeys } = await import("./keys.js");
const {
  isTerminalRun,
  RUN_DETAIL_POLL_INTERVAL_MS,
  RUNS_PAGE_SIZE,
  RUNS_POLL_INTERVAL_MS,
  runQueryOptions,
  runsQueryOptions,
  TERMINAL_RUN_STATUSES,
  useCancelRun,
  useRun,
  useRuns,
} = await import("./runs.js");

const client = {} as ApiClient;

const run = (overrides: Partial<AgentRun> = {}): AgentRun =>
  ({ id: "run-1", status: "running", ...overrides }) as AgentRun;

const page = (): AgentRunPage => ({
  results: [],
  count: 0,
  total_count: 0,
  total_pages: 1,
  page: 1,
  per_page: RUNS_PAGE_SIZE,
});

afterEach(() => {
  contractMocks.listRuns.mockReset();
  contractMocks.getRun.mockReset();
  contractMocks.cancelRun.mockReset();
  resetAppClient();
});

describe("cache keys", () => {
  it("separates pages and scopes inside the workspace subtree", () => {
    expect(runnersKeys.runs("acme", null, 1)).toEqual(["ws", "acme", "runners", "runs", "workspace", 1]);
    expect(runnersKeys.runs("acme", "p-1", 2)).toEqual(["ws", "acme", "runners", "runs", "p-1", 2]);
    expect(runnersKeys.run("acme", "run-1")).toEqual(["ws", "acme", "runners", "run", "run-1"]);
  });
});

describe("terminal statuses", () => {
  it("treats exactly the settled five as terminal", () => {
    expect(TERMINAL_RUN_STATUSES).toEqual(["blocked", "completed", "failed", "cancelled", "refused"]);
    expect(isTerminalRun("completed")).toBe(true);
    expect(isTerminalRun("blocked")).toBe(true);
    expect(isTerminalRun("running")).toBe(false);
    expect(isTerminalRun("cancel_requested")).toBe(false);
  });
});

describe("runsQueryOptions", () => {
  it("reads the workspace aggregate page and polls", async () => {
    contractMocks.listRuns.mockResolvedValue(page());
    const options = runsQueryOptions(client, "acme", "ws-id", null, 2);
    expect(options.queryKey).toEqual(runnersKeys.runs("acme", null, 2));
    expect(options.refetchInterval).toBe(RUNS_POLL_INTERVAL_MS);
    expect(options.staleTime).toBe(0);
    await options.queryFn?.({} as never);
    expect(contractMocks.listRuns).toHaveBeenCalledWith(client, {
      workspaceId: "ws-id",
      page: 2,
      perPage: RUNS_PAGE_SIZE,
    });
  });

  it("narrows to the project scope", async () => {
    contractMocks.listRuns.mockResolvedValue(page());
    const options = runsQueryOptions(client, "acme", "ws-id", "p-1", 1);
    await options.queryFn?.({} as never);
    expect(contractMocks.listRuns).toHaveBeenCalledWith(client, {
      workspaceId: "ws-id",
      page: 1,
      perPage: RUNS_PAGE_SIZE,
      projectId: "p-1",
    });
  });
});

describe("runQueryOptions", () => {
  it("fetches the detail with events and never retries", async () => {
    contractMocks.getRun.mockResolvedValue(run());
    const options = runQueryOptions(client, "acme", "run-1");
    expect(options.queryKey).toEqual(runnersKeys.run("acme", "run-1"));
    expect(options.retry).toBe(false);
    expect(options.staleTime).toBe(0);
    await options.queryFn?.({} as never);
    expect(contractMocks.getRun).toHaveBeenCalledWith(client, "run-1", { includeEvents: true });
  });

  it("polls only while the latest status is live", () => {
    const options = runQueryOptions(client, "acme", "run-1");
    const interval = options.refetchInterval as (query: { state: { data?: AgentRun } }) => number | false;
    expect(interval({ state: {} })).toBe(RUN_DETAIL_POLL_INTERVAL_MS);
    expect(interval({ state: { data: run({ status: "running" }) } })).toBe(RUN_DETAIL_POLL_INTERVAL_MS);
    expect(interval({ state: { data: run({ status: "completed" }) } })).toBe(false);
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
  return { invalidate, wrapper, queryClient };
}

describe("useRuns / useRun", () => {
  it("mounts the list read and the enabled detail read", async () => {
    contractMocks.listRuns.mockResolvedValue(page());
    contractMocks.getRun.mockResolvedValue(run());
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const { wrapper } = harness();

    const list = renderHook(() => useRuns("acme", "ws-id", null, 3), { wrapper });
    await waitFor(() => expect(list.result.current.isSuccess).toBe(true));
    expect(contractMocks.listRuns).toHaveBeenCalledWith(expect.anything(), expect.objectContaining({ page: 3 }));

    const detail = renderHook(() => useRun("acme", "run-1"), { wrapper });
    await waitFor(() => expect(detail.result.current.isSuccess).toBe(true));
    expect(contractMocks.getRun).toHaveBeenCalledWith(expect.anything(), "run-1", { includeEvents: true });
  });

  it("leaves the detail disabled with no selection", () => {
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const { wrapper } = harness();
    const detail = renderHook(() => useRun("acme", null), { wrapper });
    expect(detail.result.current.fetchStatus).toBe("idle");
    expect(contractMocks.getRun).not.toHaveBeenCalled();
  });
});

describe("useCancelRun", () => {
  it("cancels, seeds the fresh run and re-reads the area", async () => {
    const cancelled = run({ status: "cancelled" });
    contractMocks.cancelRun.mockResolvedValue(cancelled);
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const { invalidate, wrapper, queryClient } = harness();
    const { result } = renderHook(() => useCancelRun("acme"), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({ runId: "run-1", reason: "user" });
    });
    expect(contractMocks.cancelRun).toHaveBeenCalledWith(expect.anything(), "run-1", "user");
    expect(queryClient.getQueryData(runnersKeys.run("acme", "run-1"))).toEqual(cancelled);
    await waitFor(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") }));
  });

  it("rejects on failure so the caller keeps its dialog", async () => {
    contractMocks.cancelRun.mockRejectedValue(new Error("nope"));
    createAppClient("https://api.test", () => new Promise<never>(() => {}));
    const { invalidate, wrapper } = harness();
    const { result } = renderHook(() => useCancelRun("acme"), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync({ runId: "run-1" })).rejects.toThrow("nope");
    });
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") });
  });
});
