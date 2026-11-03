// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider sign-in mechanics (CLOUD-033): the reserved tab during the
// click, the blocked/closed/rejected starts, and the poll driver's settle,
// transient-failure, timeout and stop behavior.
import { afterEach, describe, expect, it, vi } from "vitest";

import { beginProviderSignIn, pollProviderLogin, pollOutcome, type PollClock, type PopupLike } from "./signIn.js";

function popup(overrides: Partial<PopupLike> = {}): PopupLike & { replaced: string[] } {
  const replaced: string[] = [];
  const node = {
    closed: false,
    opener: {},
    location: { replace: (url: string) => replaced.push(url) },
    close: vi.fn(),
    replaced,
    ...overrides,
  };
  return node as PopupLike & { replaced: string[] };
}

describe("beginProviderSignIn", () => {
  it("reserves the tab, detaches the opener, and points it at the authorize address", async () => {
    const tab = popup();
    const open = vi.fn(() => tab);
    const start = vi.fn(async () => ({ authorize_url: "https://auth.openai.com/x", state: "state-1" }));
    const answer = await beginProviderSignIn(open, start);
    expect(open).toHaveBeenCalledTimes(1);
    expect(tab.opener).toBeNull();
    expect(tab.replaced).toEqual(["https://auth.openai.com/x"]);
    expect(answer).toEqual({ result: { authorize_url: "https://auth.openai.com/x", state: "state-1" }, failure: null });
  });

  it("reports a blocked tab without starting the flow", async () => {
    const start = vi.fn(async () => ({ authorize_url: "https://auth", state: "s" }));
    await expect(beginProviderSignIn(() => null, start)).resolves.toEqual({
      result: null,
      failure: { kind: "blocked" },
    });
    expect(start).not.toHaveBeenCalled();
  });

  it("reports a tab closed before authorization", async () => {
    const tab = popup();
    const answer = await beginProviderSignIn(
      () => tab,
      async () => {
        tab.closed = true;
        return { authorize_url: "https://auth", state: "s" };
      }
    );
    expect(answer).toEqual({ result: null, failure: { kind: "closed" } });
    expect(tab.replaced).toEqual([]);
  });

  it("closes the empty tab when the start is rejected", async () => {
    const tab = popup();
    const error = new Error("nope");
    const answer = await beginProviderSignIn(
      () => tab,
      async () => {
        throw error;
      }
    );
    expect(answer).toEqual({ result: null, failure: { kind: "rejected", error } });
    expect(tab.close).toHaveBeenCalledTimes(1);
  });
});

describe("pollOutcome", () => {
  it("maps each server status", () => {
    expect(pollOutcome({ status: "pending" })).toEqual({ kind: "pending" });
    expect(pollOutcome({ status: "ok" })).toEqual({ kind: "confirmed" });
    expect(pollOutcome({ status: "error", detail: "provider said no" })).toEqual({
      kind: "failed",
      detail: "provider said no",
    });
    expect(pollOutcome({ status: "expired" })).toEqual({ kind: "failed", detail: undefined });
  });
});

/** A clock whose callbacks the test fires by hand. */
function manualClock() {
  let now = 0;
  const callbacks: Array<() => void> = [];
  const cleared: unknown[] = [];
  let next = 0;
  const clock: PollClock = {
    now: () => now,
    setInterval: (callback) => {
      callbacks.push(callback);
      next += 1;
      return next;
    },
    clearInterval: (handle) => cleared.push(handle),
  };
  return {
    clock,
    callbacks,
    cleared,
    advance(ms: number) {
      now += ms;
    },
    async fire() {
      const callback = callbacks[0];
      if (callback === undefined) throw new Error("no interval registered");
      callback();
      await Promise.resolve();
      await Promise.resolve();
    },
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("pollProviderLogin", () => {
  it("keeps polling while pending and finishes on confirmation", async () => {
    const time = manualClock();
    const onFinish = vi.fn();
    const read = vi
      .fn()
      .mockResolvedValueOnce({ status: "pending" })
      .mockResolvedValueOnce({ status: "pending" })
      .mockResolvedValueOnce({ status: "ok" });
    pollProviderLogin({ read, onFinish, clock: time.clock });
    await time.fire();
    await time.fire();
    expect(onFinish).not.toHaveBeenCalled();
    await time.fire();
    expect(onFinish).toHaveBeenCalledWith({ kind: "confirmed" });
    expect(time.cleared).toHaveLength(1);
  });

  it("ignores transient read failures until the deadline", async () => {
    const time = manualClock();
    const onFinish = vi.fn();
    const read = vi.fn().mockRejectedValueOnce(new Error("network")).mockResolvedValueOnce({ status: "error" });
    pollProviderLogin({ read, onFinish, clock: time.clock });
    await time.fire();
    expect(onFinish).not.toHaveBeenCalled();
    await time.fire();
    expect(onFinish).toHaveBeenCalledWith({ kind: "failed", detail: undefined });
  });

  it("times out once the deadline passes, without another read", async () => {
    const time = manualClock();
    const onFinish = vi.fn();
    const read = vi.fn(async () => ({ status: "pending" as const }));
    pollProviderLogin({ read, onFinish, clock: time.clock, timeoutMs: 5_000 });
    time.advance(5_001);
    await time.fire();
    expect(onFinish).toHaveBeenCalledWith({ kind: "timeout" });
    expect(read).not.toHaveBeenCalled();
  });

  it("stops without reporting when the caller cancels", async () => {
    const time = manualClock();
    const onFinish = vi.fn();
    const read = vi.fn(async () => ({ status: "ok" as const }));
    const handle = pollProviderLogin({ read, onFinish, clock: time.clock });
    handle.stop();
    handle.stop();
    await time.fire();
    expect(read).not.toHaveBeenCalled();
    expect(onFinish).not.toHaveBeenCalled();
    expect(time.cleared).toHaveLength(1);
  });

  it("drops a read that resolves after the caller already settled the flow", async () => {
    const time = manualClock();
    const onFinish = vi.fn();
    const holder: { resolve: ((value: { status: "ok" }) => void) | null } = { resolve: null };
    const read = vi.fn(
      () =>
        new Promise<{ status: "ok" }>((resolve) => {
          holder.resolve = resolve;
        })
    );
    const handle = pollProviderLogin({ read, onFinish, clock: time.clock });
    const callback = time.callbacks[0];
    const pending = (async () => {
      callback?.();
      await Promise.resolve();
    })();
    handle.stop();
    holder.resolve?.({ status: "ok" });
    await pending;
    await Promise.resolve();
    expect(onFinish).not.toHaveBeenCalled();
  });
});
