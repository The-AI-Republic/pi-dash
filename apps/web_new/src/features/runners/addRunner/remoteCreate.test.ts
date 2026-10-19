// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Remote creation loop (RUN-007): success, failure, timeout, transient read
// failures, cancellation and the offline/unreachable create errors.
import { describe, expect, it, vi } from "vitest";

import { DAEMON_TOO_OLD_TEXT, type RemoteState } from "./model.js";
import { runRemoteCreate, type RemoteCreateDeps } from "./remoteCreate.js";

type Status = Awaited<ReturnType<RemoteCreateDeps["readStatus"]>>;

function status(overrides: Partial<Status>): Status {
  return { request_id: "req-1", status: "pending", ...overrides } as Status;
}

function deps(overrides: Partial<RemoteCreateDeps> = {}): RemoteCreateDeps {
  return {
    createRunner: vi.fn(async () => ({ request_id: "req-1" })),
    readStatus: vi.fn(async () => status({})),
    sleep: vi.fn(async () => undefined),
    now: () => 0,
    isCancelled: () => false,
    update: vi.fn(),
    timeoutMs: 10,
    pollMs: 2,
    ...overrides,
  };
}

describe("runRemoteCreate", () => {
  it("reports success with the runner name", async () => {
    const d = deps({
      readStatus: vi.fn(async () => status({ status: "ok", runner_name: "new-runner" })),
    });
    const state = await runRemoteCreate({ machineId: "m1", machineLabel: "Laptop" }, d);
    expect(state).toEqual({ phase: "ok", machineLabel: "Laptop", runnerName: "new-runner" });
    expect(d.update).toHaveBeenCalledWith({ phase: "creating", machineLabel: "Laptop" });
    expect(d.update).toHaveBeenLastCalledWith(state);
  });

  it("reports a daemon error verbatim", async () => {
    const d = deps({ readStatus: vi.fn(async () => status({ status: "error", error: "boom" })) });
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, d)).toEqual({
      phase: "error",
      machineLabel: "L",
      error: "boom",
    });
  });

  it("maps the daemon's unknown-agent write-back to the upgrade hint", async () => {
    const d = deps({
      readStatus: vi.fn(async () => status({ status: "error", error: 'unknown agent kind "grok"' })),
    });
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, d)).toEqual({
      phase: "error",
      machineLabel: "L",
      error: DAEMON_TOO_OLD_TEXT,
    });
  });

  it("omits an absent success name and absent error text", async () => {
    const ok = deps({ readStatus: vi.fn(async () => status({ status: "ok" })) });
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, ok)).toEqual({
      phase: "ok",
      machineLabel: "L",
    });
    const failed = deps({ readStatus: vi.fn(async () => status({ status: "error" })) });
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, failed)).toEqual({
      phase: "error",
      machineLabel: "L",
    });
  });

  it("keeps polling through transient status failures", async () => {
    let reads = 0;
    const d = deps({
      readStatus: vi.fn(async () => {
        reads += 1;
        if (reads === 1) throw new Error("transient");
        return status({ status: "ok", runner_name: "r" });
      }),
    });
    const state = await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, d);
    expect(state?.phase).toBe("ok");
    expect(reads).toBe(2);
  });

  it("times out when the deadline passes with only pending answers", async () => {
    let clock = 0;
    const d = deps({
      now: () => clock,
      sleep: vi.fn(async () => {
        clock += 2;
      }),
    });
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, d)).toEqual({
      phase: "timeout",
      machineLabel: "L",
    });
  });

  it("cancels before the first poll and after it", async () => {
    let cancelled = false;
    const before = deps({ isCancelled: () => cancelled });
    cancelled = true;
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, before)).toBeNull();
    expect(before.readStatus).not.toHaveBeenCalled();

    let reads = 0;
    const afterRead = deps({
      isCancelled: () => reads > 0,
      readStatus: vi.fn(async () => {
        reads += 1;
        return status({});
      }),
    });
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, afterRead)).toBeNull();
  });

  it("cancels at the deadline before reporting timeout", async () => {
    let clock = 0;
    let cancelled = false;
    const d = deps({
      now: () => clock,
      isCancelled: () => cancelled,
      sleep: vi.fn(async () => {
        clock += 2;
        cancelled = true;
      }),
    });
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, d)).toBeNull();
    expect(d.update).not.toHaveBeenCalledWith({ phase: "timeout", machineLabel: "L" });
  });

  it("cancels when the create request was aborted", async () => {
    const d = deps({
      createRunner: vi.fn(async () => {
        throw Object.assign(new Error("aborted"), { code: "aborted" });
      }),
    });
    expect(await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, d)).toBeNull();
    expect(d.readStatus).not.toHaveBeenCalled();
  });

  it("reports offline and unreachable create failures", async () => {
    const offline = deps({
      createRunner: vi.fn(async () => {
        throw Object.assign(new Error("gone"), { code: "machine_offline" });
      }),
    });
    const offlineState = (await runRemoteCreate({ machineId: "m1", machineLabel: "L" }, offline)) as RemoteState;
    expect(offlineState.error).toContain("went offline");

    const unreachable = deps({
      createRunner: vi.fn(async () => {
        throw new Error("network down");
      }),
    });
    const unreachableState = (await runRemoteCreate(
      { machineId: "m1", machineLabel: "L" },
      unreachable
    )) as RemoteState;
    expect(unreachableState.error).toContain("Could not reach the cloud");
  });
});
