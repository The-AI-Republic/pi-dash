// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop updater bindings (DESK-016/017/018 web half): the pending-update
// query, the install invoke, and the available-event subscription the
// sidebar control drives.
import { afterEach, describe, expect, it, vi } from "vitest";

import type { PendingUpdate, Unsubscribe } from "./index.js";
import { platform, UPDATE_AVAILABLE_EVENT, toPendingUpdate } from "./tauri.js";

type Handler = (event: { payload: unknown }) => void;

const NATIVE_ORIGIN = "http://localhost:8000";
const waiting: PendingUpdate = { version: "1.4.0", currentVersion: "1.3.2", installing: false };

function removeBridge(): void {
  const win = globalThis.window as unknown as Record<string, unknown>;
  delete win["__PIDASH_NATIVE_HTTP__"];
  delete win["__TAURI__"];
}

function installBridge(options: {
  invoke?: (command: string, args?: unknown) => Promise<unknown>;
  listen?: (event: string, handler: Handler) => Promise<Unsubscribe>;
}): { invoke: ReturnType<typeof vi.fn>; listen: ReturnType<typeof vi.fn> } {
  const invoke = vi.fn(options.invoke ?? (async () => null));
  const listen = vi.fn(options.listen ?? (async () => () => undefined));
  const win = globalThis.window as unknown as Record<string, unknown>;
  win["__PIDASH_NATIVE_HTTP__"] = NATIVE_ORIGIN;
  win["__TAURI__"] = { core: { invoke }, event: { listen } };
  return { invoke, listen };
}

/** Let the subscription's listen promise settle one microtask turn. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

afterEach(() => {
  removeBridge();
  vi.restoreAllMocks();
});

describe("pending-update payload", () => {
  it("maps the shell's camelCase shape", () => {
    expect(toPendingUpdate(waiting)).toEqual(waiting);
  });

  it("reads the installing flag strictly", () => {
    expect(toPendingUpdate({ ...waiting, installing: true })).toMatchObject({ installing: true });
    expect(toPendingUpdate({ ...waiting, installing: "yes" })).toMatchObject({ installing: false });
    expect(toPendingUpdate({ version: "1.4.0", currentVersion: "1.3.2" })).toMatchObject({ installing: false });
  });

  it("treats malformed payloads as no update", () => {
    expect(toPendingUpdate(null)).toBeNull();
    expect(toPendingUpdate(undefined)).toBeNull();
    expect(toPendingUpdate("1.4.0")).toBeNull();
    expect(toPendingUpdate({ currentVersion: "1.3.2" })).toBeNull();
    expect(toPendingUpdate({ version: "", currentVersion: "1.3.2" })).toBeNull();
    expect(toPendingUpdate({ version: 4, currentVersion: "1.3.2" })).toBeNull();
    expect(toPendingUpdate({ version: "1.4.0" })).toBeNull();
    expect(toPendingUpdate({ version: "1.4.0", currentVersion: 3 })).toBeNull();
  });
});

describe("desktop updater", () => {
  it("asks the shell what is waiting", async () => {
    const { invoke } = installBridge({ invoke: async () => waiting });
    await expect(platform.updates?.pending()).resolves.toEqual(waiting);
    expect(invoke).toHaveBeenCalledWith("desktop_pending_update", undefined);
  });

  it("answers null when nothing waits", async () => {
    installBridge({ invoke: async () => null });
    await expect(platform.updates?.pending()).resolves.toBeNull();
  });

  it("answers null for a malformed pending answer", async () => {
    installBridge({ invoke: async () => ({ version: 7 }) });
    await expect(platform.updates?.pending()).resolves.toBeNull();
  });

  it("installs through the shell command", async () => {
    const { invoke } = installBridge({ invoke: async () => undefined });
    await expect(platform.updates?.install()).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith("desktop_install_update", undefined);
  });

  it("passes an install failure to the caller", async () => {
    installBridge({
      invoke: async () => {
        throw new Error("signature mismatch");
      },
    });
    await expect(platform.updates?.install()).rejects.toThrow("signature mismatch");
  });

  it("delivers announced updates on the native event", async () => {
    let handler: Handler | undefined;
    const { listen } = installBridge({
      listen: async (event, callback) => {
        void event;
        handler = callback;
        return () => undefined;
      },
    });
    const seen: PendingUpdate[] = [];
    platform.updates?.onAvailable((update) => seen.push(update));
    await settle();
    expect(listen).toHaveBeenCalledWith(UPDATE_AVAILABLE_EVENT, expect.any(Function));
    handler?.({ payload: waiting });
    expect(seen).toEqual([waiting]);
  });

  it("drops malformed announcements", async () => {
    let handler: Handler | undefined;
    installBridge({
      listen: async (_event, callback) => {
        handler = callback;
        return () => undefined;
      },
    });
    const seen: PendingUpdate[] = [];
    platform.updates?.onAvailable((update) => seen.push(update));
    await settle();
    handler?.({ payload: null });
    handler?.({ payload: { version: "1.4.0" } });
    expect(seen).toEqual([]);
  });

  it("stops listening when the subscription is dropped", async () => {
    const stop = vi.fn();
    let handler: Handler | undefined;
    installBridge({
      listen: async (_event, callback) => {
        handler = callback;
        return stop;
      },
    });
    const seen: PendingUpdate[] = [];
    const unsubscribe = platform.updates?.onAvailable((update) => seen.push(update));
    await settle();
    unsubscribe?.();
    expect(stop).toHaveBeenCalledTimes(1);
    handler?.({ payload: waiting });
    expect(seen).toEqual([]);
  });

  it("unsubscribes a listener that resolves after the control left", async () => {
    let resolveListen: ((stop: Unsubscribe) => void) | undefined;
    installBridge({
      listen: () =>
        new Promise<Unsubscribe>((resolve) => {
          resolveListen = resolve;
        }),
    });
    const unsubscribe = platform.updates?.onAvailable(() => undefined);
    unsubscribe?.();
    const stop = vi.fn();
    resolveListen?.(stop);
    await settle();
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it("swallows a failed subscription", async () => {
    installBridge({
      listen: async () => {
        throw new Error("no event bridge");
      },
    });
    expect(() => platform.updates?.onAvailable(() => undefined)).not.toThrow();
    await settle();
  });

  it("has nothing to subscribe to without the event bridge", () => {
    const invoke = vi.fn(async () => null);
    const win = globalThis.window as unknown as Record<string, unknown>;
    win["__PIDASH_NATIVE_HTTP__"] = NATIVE_ORIGIN;
    win["__TAURI__"] = { core: { invoke } };
    const unsubscribe = platform.updates?.onAvailable(() => undefined);
    expect(typeof unsubscribe).toBe("function");
    expect(() => unsubscribe?.()).not.toThrow();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rejects pending and install calls without the native bridge", async () => {
    await expect(platform.updates?.pending()).rejects.toThrow("desktop_pending_update needs the native bridge");
    await expect(platform.updates?.install()).rejects.toThrow("desktop_install_update needs the native bridge");
  });
});
