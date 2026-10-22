// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dispatch hooks for issue surfaces (NEWFRONT-547, RUN-046/RUN-048).
// Triggering toasts started or queued and never writes the executor;
// abort refreshes the area; re-tick toasts the grant, the refusal, or
// the transport failure each in its own tier.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { AgentRun, AgentRunQueued, ReTickResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  client: { baseUrl: "https://api.test" },
  runtime: { kind: "desktop" },
  triggerAgentRun: vi.fn(),
  abortAgentRun: vi.fn(),
  requestReTick: vi.fn(),
  notifyError: vi.fn(),
  notifyInfo: vi.fn(),
  notifySuccess: vi.fn(),
}));

vi.mock("../../../core/api/client.js", () => ({
  getAppClient: () => harness.client,
}));

vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return { agentRuntime: harness.runtime };
  },
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return {
    ...original,
    notifyError: (...args: Array<unknown>) => harness.notifyError(...args),
    notifyInfo: (...args: Array<unknown>) => harness.notifyInfo(...args),
    notifySuccess: (...args: Array<unknown>) => harness.notifySuccess(...args),
  };
});

vi.mock("./dispatch.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./dispatch.js")>();
  return {
    ...original,
    triggerAgentRun: (...args: Array<unknown>) => harness.triggerAgentRun(...args),
    abortAgentRun: (...args: Array<unknown>) => harness.abortAgentRun(...args),
    requestReTick: (...args: Array<unknown>) => harness.requestReTick(...args),
  };
});

const { runnersKeys } = await import("../api/keys.js");
const {
  RE_TICK_FAILED_TITLE,
  RE_TICK_GRANTED_TITLE,
  RE_TICK_REFUSED_TITLE,
  RUN_ABORT_FAILED_TITLE,
  RUN_QUEUED_TITLE,
  RUN_START_FAILED_TITLE,
  RUN_STARTED_TITLE,
  useAbortAgentRun,
  useReTick,
  useTriggerAgentRun,
} = await import("./useDispatch.js");

const run = { id: "run-1", status: "queued" } as AgentRun;
const queued = { queued: true, reason: "active", detail: "one already runs" } as AgentRunQueued;
const grantedNow = { granted: true, reason: "exhausted", run_id: "run-2" } as ReTickResponse;
const grantedQueued = { granted: true, reason: "exhausted", run_id: null } as ReTickResponse;
const refused = { granted: false, reason: "budget_not_exhausted", run_id: null } as ReTickResponse;

function wrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  return (
    <QueryClientProvider client={queryClient}>
      <React.Fragment>{children}</React.Fragment>
    </QueryClientProvider>
  );
}

afterEach(() => {
  harness.triggerAgentRun.mockReset();
  harness.abortAgentRun.mockReset();
  harness.requestReTick.mockReset();
  harness.notifyError.mockReset();
  harness.notifyInfo.mockReset();
  harness.notifySuccess.mockReset();
});

const triggerArgs = {
  workspaceId: "ws-id",
  workspaceSlug: "acme",
  projectId: "p-1",
  issueId: "issue-1",
  mode: "run_ai",
} as const;

describe("useTriggerAgentRun", () => {
  it("toasts the start, refreshes, and hands back the run", async () => {
    harness.triggerAgentRun.mockResolvedValue(run);
    const onSettledRefresh = vi.fn(async () => undefined);
    const { result } = renderHook(() => useTriggerAgentRun({ onSettledRefresh }), { wrapper });
    expect(result.current.isSubmitting).toBe(false);

    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.triggerRun(triggerArgs);
    });
    expect(harness.triggerAgentRun).toHaveBeenCalledWith({
      client: harness.client,
      runtime: harness.runtime,
      ...triggerArgs,
    });
    expect(harness.notifySuccess).toHaveBeenCalledWith(RUN_STARTED_TITLE, expect.any(String));
    expect(onSettledRefresh).toHaveBeenCalledTimes(1);
    expect(outcome).toBe(run);
    expect(result.current.isSubmitting).toBe(false);
  });

  it("toasts a queued dispatch informationally", async () => {
    harness.triggerAgentRun.mockResolvedValue(queued);
    const { result } = renderHook(() => useTriggerAgentRun(), { wrapper });
    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.triggerRun({ ...triggerArgs, mode: "comment_and_run" });
    });
    expect(harness.notifyInfo).toHaveBeenCalledWith(RUN_QUEUED_TITLE, expect.any(String));
    expect(harness.notifySuccess).not.toHaveBeenCalled();
    expect(outcome).toBe(queued);
  });

  it("surfaces the failure message and skips the refresh", async () => {
    harness.triggerAgentRun.mockRejectedValue(new Error("daemon down"));
    const onSettledRefresh = vi.fn(async () => undefined);
    const { result } = renderHook(() => useTriggerAgentRun({ onSettledRefresh }), { wrapper });
    let outcome: unknown = "unset";
    await act(async () => {
      outcome = await result.current.triggerRun(triggerArgs);
    });
    expect(harness.notifyError).toHaveBeenCalledWith(RUN_START_FAILED_TITLE, "daemon down");
    expect(onSettledRefresh).not.toHaveBeenCalled();
    expect(outcome).toBeNull();
    expect(result.current.isSubmitting).toBe(false);
  });

  it("falls back when the failure carries no message", async () => {
    harness.triggerAgentRun.mockRejectedValueOnce("boom").mockRejectedValueOnce(new Error(""));
    const { result } = renderHook(() => useTriggerAgentRun(), { wrapper });
    await act(async () => {
      await result.current.triggerRun(triggerArgs);
    });
    await act(async () => {
      await result.current.triggerRun(triggerArgs);
    });
    expect(harness.notifyError).toHaveBeenCalledWith(RUN_START_FAILED_TITLE, "Try again in a moment.");
    expect(harness.notifyError).toHaveBeenCalledTimes(2);
  });

  it("keeps a successful trigger when the refresh fails", async () => {
    harness.triggerAgentRun.mockResolvedValue(run);
    const onSettledRefresh = vi.fn(async () => {
      throw new Error("stale");
    });
    const { result } = renderHook(() => useTriggerAgentRun({ onSettledRefresh }), { wrapper });
    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.triggerRun(triggerArgs);
    });
    expect(outcome).toBe(run);
    expect(harness.notifyError).not.toHaveBeenCalled();
  });
});

describe("useAbortAgentRun", () => {
  it("aborts, seeds the fresh run, and refreshes the area plus the caller", async () => {
    const aborted = { ...run, status: "cancelled" };
    harness.abortAgentRun.mockResolvedValue(aborted);
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const abortWrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>
        <React.Fragment>{children}</React.Fragment>
      </QueryClientProvider>
    );
    const onSettledRefresh = vi.fn(async () => undefined);
    const { result } = renderHook(() => useAbortAgentRun("acme", { onSettledRefresh }), { wrapper: abortWrapper });

    await act(async () => {
      await result.current.mutateAsync({ runId: "run-1", reason: "stale" });
    });
    expect(harness.abortAgentRun).toHaveBeenCalledWith(harness.client, "run-1", "stale");
    expect(queryClient.getQueryData(runnersKeys.run("acme", "run-1"))).toEqual(aborted);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: runnersKeys.all("acme") });
    expect(onSettledRefresh).toHaveBeenCalledTimes(1);
    expect(harness.notifyError).not.toHaveBeenCalled();
  });

  it("toasts when the abort fails", async () => {
    harness.abortAgentRun.mockRejectedValue(new Error("nope"));
    const { result } = renderHook(() => useAbortAgentRun("acme"), { wrapper });
    await act(async () => {
      await expect(result.current.mutateAsync({ runId: "run-1" })).rejects.toThrow("nope");
    });
    expect(harness.notifyError).toHaveBeenCalledWith(RUN_ABORT_FAILED_TITLE, expect.any(String));
  });

  it("keeps a settled abort when the caller refresh fails", async () => {
    harness.abortAgentRun.mockResolvedValue(run);
    const onSettledRefresh = vi.fn(async () => {
      throw new Error("stale");
    });
    const { result } = renderHook(() => useAbortAgentRun("acme", { onSettledRefresh }), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ runId: "run-1" });
    });
    expect(onSettledRefresh).toHaveBeenCalledTimes(1);
    expect(harness.notifyError).not.toHaveBeenCalled();
  });
});

describe("useReTick", () => {
  it("toasts a grant that starts now and refreshes", async () => {
    harness.requestReTick.mockResolvedValue(grantedNow);
    const onSettledRefresh = vi.fn(async () => undefined);
    const { result } = renderHook(() => useReTick({ onSettledRefresh }), { wrapper });
    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.reTick("issue-1");
    });
    expect(harness.requestReTick).toHaveBeenCalledWith(harness.client, "issue-1");
    expect(harness.notifySuccess).toHaveBeenCalledWith(RE_TICK_GRANTED_TITLE, expect.stringContaining("starting now"));
    expect(outcome).toBe(grantedNow);
    expect(onSettledRefresh).toHaveBeenCalledTimes(1);
    expect(result.current.isSubmitting).toBe(false);
  });

  it("toasts a grant that queues behind the active run", async () => {
    harness.requestReTick.mockResolvedValue(grantedQueued);
    const { result } = renderHook(() => useReTick(), { wrapper });
    await act(async () => {
      await result.current.reTick("issue-1");
    });
    expect(harness.notifySuccess).toHaveBeenCalledWith(
      RE_TICK_GRANTED_TITLE,
      expect.stringContaining("active run ends")
    );
  });

  it("toasts a refusal informationally and still refreshes", async () => {
    harness.requestReTick.mockResolvedValue(refused);
    const onSettledRefresh = vi.fn(async () => undefined);
    const { result } = renderHook(() => useReTick({ onSettledRefresh }), { wrapper });
    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.reTick("issue-1");
    });
    expect(harness.notifyInfo).toHaveBeenCalledWith(RE_TICK_REFUSED_TITLE, expect.any(String));
    expect(outcome).toBe(refused);
    expect(onSettledRefresh).toHaveBeenCalledTimes(1);
  });

  it("toasts only transport failures as errors", async () => {
    harness.requestReTick.mockRejectedValue(new Error("offline"));
    const { result } = renderHook(() => useReTick(), { wrapper });
    let outcome: unknown = "unset";
    await act(async () => {
      outcome = await result.current.reTick("issue-1");
    });
    expect(harness.notifyError).toHaveBeenCalledWith(RE_TICK_FAILED_TITLE, "offline");
    expect(outcome).toBeNull();
  });

  it("keeps the outcome when the refresh fails", async () => {
    harness.requestReTick.mockResolvedValue(refused);
    const onSettledRefresh = vi.fn(async () => {
      throw new Error("stale");
    });
    const { result } = renderHook(() => useReTick({ onSettledRefresh }), { wrapper });
    let outcome: unknown;
    await act(async () => {
      outcome = await result.current.reTick("issue-1");
    });
    expect(outcome).toBe(refused);
    expect(harness.notifyError).not.toHaveBeenCalled();
  });
});
