// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-update mirror (DESK-018): the mount query, the announcement
// subscription, their race, and install request bookkeeping.
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { PendingUpdate, UpdaterApi } from "../../core/platform/index.js";
import { installFailureText, UPDATE_FAILED_FALLBACK_TEXT, UPDATE_NO_RESTART_TEXT, useUpdaterState } from "./state.js";

const WAITING: PendingUpdate = { version: "2.0.0", currentVersion: "1.9.4", installing: false };

type Listener = (update: PendingUpdate) => void;

function fakeUpdater(overrides: Partial<UpdaterApi> = {}): {
  api: UpdaterApi;
  listeners: Set<Listener>;
  install: ReturnType<typeof vi.fn>;
  pending: ReturnType<typeof vi.fn>;
  unsubscribed: ReturnType<typeof vi.fn>;
} {
  const listeners = new Set<Listener>();
  const unsubscribed = vi.fn();
  const pending = vi.fn(overrides.pending ?? (async () => null));
  const install = vi.fn(overrides.install ?? (async () => undefined));
  const onAvailable =
    overrides.onAvailable ??
    ((callback: Listener) => {
      listeners.add(callback);
      return () => {
        listeners.delete(callback);
        unsubscribed();
      };
    });
  return { api: { pending, install, onAvailable }, listeners, install, pending, unsubscribed };
}

function announce(api: { listeners: Set<Listener> }, update: PendingUpdate): void {
  act(() => {
    for (const listener of [...api.listeners]) listener(update);
  });
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
  });
}

describe("useUpdaterState", () => {
  it("renders nothing when the platform has no updates capability", () => {
    const { result } = renderHook(() => useUpdaterState(undefined));
    expect(result.current.update).toBeNull();
    // The install action is inert without the capability.
    act(() => result.current.install());
    expect(result.current.requesting).toBe(false);
  });

  it("asks the shell on mount and shows the deferred update", async () => {
    const api = fakeUpdater({ pending: async () => WAITING });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    expect(api.pending).toHaveBeenCalledTimes(1);
    expect(result.current.update).toEqual(WAITING);
  });

  it("shows nothing when no update waits", async () => {
    const api = fakeUpdater();
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    expect(result.current.update).toBeNull();
  });

  it("keeps the control hidden when the query is refused", async () => {
    const api = fakeUpdater({
      pending: async () => {
        throw new Error("native store unavailable");
      },
    });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    expect(result.current.update).toBeNull();
    expect(result.current.failure).toBeNull();
  });

  it("takes an announcement that lands while running", async () => {
    const api = fakeUpdater();
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    announce(api, WAITING);
    expect(result.current.update).toEqual(WAITING);
  });

  it("does not let a late query answer overwrite an announcement", async () => {
    let resolvePending: ((update: PendingUpdate | null) => void) | undefined;
    const api = fakeUpdater({
      pending: () =>
        new Promise<PendingUpdate | null>((resolve) => {
          resolvePending = resolve;
        }),
    });
    const { result } = renderHook(() => useUpdaterState(api.api));
    announce(api, { version: "2.1.0", currentVersion: "1.9.4", installing: false });
    resolvePending?.(WAITING);
    await flush();
    expect(result.current.update).toMatchObject({ version: "2.1.0" });
  });

  it("unsubscribes on unmount and ignores late events", async () => {
    const api = fakeUpdater();
    const { result, unmount } = renderHook(() => useUpdaterState(api.api));
    await flush();
    unmount();
    expect(api.unsubscribed).toHaveBeenCalledTimes(1);
    announce(api, WAITING);
    expect(result.current.update).toBeNull();
  });

  it("ignores an announcement that lands after unmount even if the listener lingers", async () => {
    // A bridge that fails to detach must not reach a torn-down control.
    const listeners = new Set<Listener>();
    const api = fakeUpdater({
      onAvailable: (callback) => {
        listeners.add(callback);
        return () => undefined;
      },
    });
    const { result, unmount } = renderHook(() => useUpdaterState(api.api));
    await flush();
    unmount();
    announce({ listeners }, WAITING);
    expect(result.current.update).toBeNull();
  });

  it("forgets the previous release when the capability goes away", async () => {
    const api = fakeUpdater({ pending: async () => WAITING });
    const { result, rerender } = renderHook(({ capability }) => useUpdaterState(capability), {
      initialProps: { capability: api.api as UpdaterApi | undefined },
    });
    await flush();
    expect(result.current.update).toEqual(WAITING);
    rerender({ capability: undefined });
    expect(result.current.update).toBeNull();
  });

  it("asks the shell to install the waiting release and reports busy", async () => {
    let finish: (() => void) | undefined;
    const api = fakeUpdater({
      pending: async () => WAITING,
      install: () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    act(() => result.current.install());
    expect(api.install).toHaveBeenCalledTimes(1);
    expect(result.current.requesting).toBe(true);
    finish?.();
    await flush();
    expect(result.current.requesting).toBe(false);
  });

  it("reports a failed install in place", async () => {
    const api = fakeUpdater({
      pending: async () => WAITING,
      install: async () => {
        throw new Error("signature mismatch");
      },
    });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    act(() => result.current.install());
    await flush();
    expect(result.current.requesting).toBe(false);
    expect(result.current.failure).toBe("signature mismatch");
    expect(result.current.update).toEqual(WAITING);
  });

  it("reports a resolve that never restarted the app", async () => {
    const api = fakeUpdater({ pending: async () => WAITING });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    act(() => result.current.install());
    await flush();
    expect(result.current.failure).toBe(UPDATE_NO_RESTART_TEXT);
  });

  it("clears the failure when retrying", async () => {
    const api = fakeUpdater({
      pending: async () => WAITING,
      install: async () => {
        throw new Error("disk full");
      },
    });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    act(() => result.current.install());
    await flush();
    expect(result.current.failure).toBe("disk full");
    act(() => result.current.install());
    expect(result.current.failure).toBeNull();
    await flush();
    expect(api.install).toHaveBeenCalledTimes(2);
  });

  it("drops an in-flight install when the capability changes", async () => {
    let finish: (() => void) | undefined;
    const api = fakeUpdater({
      pending: async () => WAITING,
      install: () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    });
    const { result, rerender } = renderHook(({ capability }) => useUpdaterState(capability), {
      initialProps: { capability: api.api as UpdaterApi | undefined },
    });
    await flush();
    act(() => result.current.install());
    expect(result.current.requesting).toBe(true);
    rerender({ capability: undefined });
    expect(result.current.requesting).toBe(false);
    finish?.();
    await flush();
    expect(result.current.failure).toBeNull();
  });

  it("ignores an older attempt's late failure after a retry started", async () => {
    const attempts: Array<{ reject: (error: Error) => void }> = [];
    const api = fakeUpdater({
      pending: async () => WAITING,
      install: () =>
        new Promise<void>((_resolve, reject) => {
          attempts.push({ reject });
        }),
    });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    act(() => result.current.install());
    act(() => result.current.install());
    act(() => attempts[0]?.reject(new Error("stale failure")));
    await flush();
    expect(result.current.failure).toBeNull();
    expect(result.current.requesting).toBe(true);
    act(() => attempts[1]?.reject(new Error("fresh failure")));
    await flush();
    expect(result.current.failure).toBe("fresh failure");
  });

  it("clears a failure when the shell announces a different release", async () => {
    const api = fakeUpdater({
      pending: async () => WAITING,
      install: async () => {
        throw new Error("offline");
      },
    });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    act(() => result.current.install());
    await flush();
    expect(result.current.failure).toBe("offline");
    announce(api, { version: "2.1.0", currentVersion: "1.9.4", installing: false });
    expect(result.current.failure).toBeNull();
  });

  it("keeps a failure when the shell re-announces the same release", async () => {
    const api = fakeUpdater({
      pending: async () => WAITING,
      install: async () => {
        throw new Error("offline");
      },
    });
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    act(() => result.current.install());
    await flush();
    announce(api, WAITING);
    expect(result.current.failure).toBe("offline");
  });

  it("reads the installing flag from the announcement", async () => {
    const api = fakeUpdater();
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    announce(api, { ...WAITING, installing: true });
    expect(result.current.update).toMatchObject({ installing: true });
  });

  it("does nothing when install is asked for with no update", async () => {
    const api = fakeUpdater();
    const { result } = renderHook(() => useUpdaterState(api.api));
    await flush();
    act(() => result.current.install());
    expect(api.install).not.toHaveBeenCalled();
  });
});

describe("installFailureText", () => {
  it("prefers the error message and trims it", () => {
    expect(installFailureText(new Error("  network down "))).toBe("network down");
    expect(installFailureText("denied")).toBe("denied");
  });

  it("falls back when the reason is empty or not an error", () => {
    expect(installFailureText(new Error("   "))).toBe(UPDATE_FAILED_FALLBACK_TEXT);
    expect(installFailureText("")).toBe(UPDATE_FAILED_FALLBACK_TEXT);
    expect(installFailureText(undefined)).toBe(UPDATE_FAILED_FALLBACK_TEXT);
    expect(installFailureText({ code: 3 })).toBe(UPDATE_FAILED_FALLBACK_TEXT);
  });
});
