// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The apply-and-poll loop: refusal, the daemon's answer, transient read
// failures, timeout and cancellation.
import { ApiError, type RunnerReconfigureStatus } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import { refusalReason, runReconfigure, type ReconfigureDeps } from "./reconfigure.js";

const REQ = "33333333-3333-4333-8333-333333333333";

function deps(
  reads: Array<RunnerReconfigureStatus | Error>,
  overrides: Partial<ReconfigureDeps> = {}
): ReconfigureDeps {
  let clock = 0;
  return {
    send: vi.fn(async () => ({ request_id: REQ })),
    readStatus: vi.fn(async () => {
      const next = reads.shift() ?? { request_id: REQ, status: "pending" as const };
      if (next instanceof Error) throw next;
      return next;
    }),
    sleep: vi.fn(async (ms: number) => {
      clock += ms;
    }),
    now: () => clock,
    isCancelled: () => false,
    timeoutMs: 10,
    pollMs: 2,
    ...overrides,
  };
}

describe("runReconfigure", () => {
  it("answers ok once the daemon confirms, riding out a failed read", async () => {
    const d = deps([new Error("network"), { request_id: REQ, status: "pending" }, { request_id: REQ, status: "ok" }]);
    await expect(runReconfigure("r-1", { model: "m" }, d)).resolves.toEqual({ kind: "ok" });
    expect(d.send).toHaveBeenCalledWith("r-1", { model: "m" });
  });

  it("passes the daemon's error through", async () => {
    const d = deps([{ request_id: REQ, status: "error", error: "model does not apply" }]);
    await expect(runReconfigure("r-1", { model: "m" }, d)).resolves.toEqual({
      kind: "error",
      reason: "model does not apply",
    });
  });

  it("reports the server's refusal reason", async () => {
    const refusal = new ApiError({ status: 409, code: "http", message: "x", body: { error: "machine_offline" } });
    const d = deps([], { send: vi.fn(async () => Promise.reject(refusal)) });
    await expect(runReconfigure("r-1", { model: "m" }, d)).resolves.toEqual({
      kind: "error",
      reason: "machine_offline",
    });
    expect(refusalReason(new Error("plain"))).toBeUndefined();
  });

  it("times out when the daemon never answers, and stops when cancelled", async () => {
    await expect(runReconfigure("r-1", { model: "m" }, deps([]))).resolves.toEqual({ kind: "timeout" });
    await expect(runReconfigure("r-1", { model: "m" }, deps([], { isCancelled: () => true }))).resolves.toEqual({
      kind: "cancelled",
    });
  });
});
