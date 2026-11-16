// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run dispatch service (NEWFRONT-547, RUN-046/RUN-048). Dispatch
// prepares the managed runner before posting, sends ids plus the
// trigger only, and reads the queued shape; abort and re-tick post
// through the same module.
import type { AgentRun, AgentRunQueued, ApiClient, ReTickResponse } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

const { contractMocks, prepareAgentRun } = vi.hoisted(() => ({
  contractMocks: { cancelRun: vi.fn(), commentAndRun: vi.fn(), reTickIssue: vi.fn(), runAi: vi.fn() },
  prepareAgentRun: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../runtime/service.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../runtime/service.js")>();
  return { ...original, prepareAgentRun: (...args: Array<unknown>) => prepareAgentRun(...args) };
});

const { abortAgentRun, isQueuedDispatch, requestReTick, triggerAgentRun } = await import("./dispatch.js");

const client = {} as ApiClient;
const run = { id: "run-1", status: "queued" } as AgentRun;
const queued = { queued: true, reason: "active", detail: "one already runs" } as AgentRunQueued;

afterEach(() => {
  contractMocks.cancelRun.mockReset();
  contractMocks.commentAndRun.mockReset();
  contractMocks.reTickIssue.mockReset();
  contractMocks.runAi.mockReset();
  prepareAgentRun.mockReset();
});

describe("isQueuedDispatch", () => {
  it("tells a queued dispatch from a fresh run", () => {
    expect(isQueuedDispatch(queued)).toBe(true);
    expect(isQueuedDispatch(run)).toBe(false);
  });
});

describe("triggerAgentRun", () => {
  const base = {
    client,
    runtime: { kind: "desktop" } as never,
    workspaceId: "ws-id",
    workspaceSlug: "acme",
    projectId: "p-1",
    issueId: "issue-1",
  };

  it("prepares the managed runner, then posts Run AI with ids only", async () => {
    prepareAgentRun.mockResolvedValue(undefined);
    contractMocks.runAi.mockResolvedValue(run);
    const heartbeatPoll = { maxAttempts: 1 };
    const result = await triggerAgentRun({ ...base, mode: "run_ai", heartbeatPoll });
    expect(prepareAgentRun).toHaveBeenCalledWith({
      client,
      runtime: base.runtime,
      target: { workspaceSlug: "acme", projectId: "p-1", issueId: "issue-1" },
      heartbeatPoll,
    });
    expect(contractMocks.runAi).toHaveBeenCalledWith(client, { workspace: "ws-id", work_item: "issue-1" });
    expect(contractMocks.commentAndRun).not.toHaveBeenCalled();
    expect(result).toBe(run);
  });

  it("posts Comment & Run without a runtime and reads the queued shape", async () => {
    prepareAgentRun.mockResolvedValue(undefined);
    contractMocks.commentAndRun.mockResolvedValue(queued);
    const { runtime: _dropped, ...webBase } = base;
    const result = await triggerAgentRun({ ...webBase, mode: "comment_and_run" });
    expect(prepareAgentRun).toHaveBeenCalledWith({
      client,
      runtime: undefined,
      target: { workspaceSlug: "acme", projectId: "p-1", issueId: "issue-1" },
      heartbeatPoll: undefined,
    });
    expect(contractMocks.commentAndRun).toHaveBeenCalledWith(client, { workspace: "ws-id", work_item: "issue-1" });
    expect(contractMocks.runAi).not.toHaveBeenCalled();
    expect(isQueuedDispatch(result)).toBe(true);
  });

  it("never posts when preparation fails", async () => {
    prepareAgentRun.mockRejectedValue(new Error("daemon down"));
    await expect(triggerAgentRun({ ...base, mode: "run_ai" })).rejects.toThrow("daemon down");
    expect(contractMocks.runAi).not.toHaveBeenCalled();
    expect(contractMocks.commentAndRun).not.toHaveBeenCalled();
  });
});

describe("abortAgentRun", () => {
  it("cancels with the reason verbatim", async () => {
    contractMocks.cancelRun.mockResolvedValue(run);
    await expect(abortAgentRun(client, "run-1", "stale")).resolves.toBe(run);
    expect(contractMocks.cancelRun).toHaveBeenCalledWith(client, "run-1", "stale");
  });

  it("omits the reason when the caller passes none", async () => {
    contractMocks.cancelRun.mockResolvedValue(run);
    await abortAgentRun(client, "run-1");
    expect(contractMocks.cancelRun).toHaveBeenCalledWith(client, "run-1", undefined);
  });
});

describe("requestReTick", () => {
  it("posts the work item and returns the grant-or-refuse envelope", async () => {
    const envelope = { granted: false, reason: "budget_not_exhausted", run_id: null } as ReTickResponse;
    contractMocks.reTickIssue.mockResolvedValue(envelope);
    await expect(requestReTick(client, "issue-1")).resolves.toBe(envelope);
    expect(contractMocks.reTickIssue).toHaveBeenCalledWith(client, { work_item: "issue-1" });
  });
});
