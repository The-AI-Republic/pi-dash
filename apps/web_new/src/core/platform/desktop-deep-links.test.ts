// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { platform } from "./tauri.js";

type Listener = (event: { payload: unknown }) => void;

function installEventApi(listen: (event: string, handler: Listener) => Promise<() => void>): void {
  Object.assign(window, { __TAURI__: { event: { listen } } });
}

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI__");
});

describe("desktop deep links", () => {
  it("subscribes to the plugin event and forwards a single URL", async () => {
    let listener: Listener | undefined;
    const listen = vi.fn(async (event: string, handler: Listener) => {
      expect(event).toBe("deep-link://new-url");
      listener = handler;
      return () => undefined;
    });
    installEventApi(listen);
    const opened: string[] = [];

    platform.deepLinks?.onOpenUrl((url) => opened.push(url));
    await Promise.resolve();
    listener?.({ payload: "pidash://auth/callback?code=abc&state=xyz" });

    expect(listen).toHaveBeenCalledTimes(1);
    expect(opened).toEqual(["pidash://auth/callback?code=abc&state=xyz"]);
  });

  it("forwards a batched payload and skips non-string entries", async () => {
    let listener: Listener | undefined;
    installEventApi(async (_event, handler) => {
      listener = handler;
      return () => undefined;
    });
    const opened: string[] = [];

    platform.deepLinks?.onOpenUrl((url) => opened.push(url));
    await Promise.resolve();
    listener?.({ payload: ["pidash://auth/callback?code=a", 7, "", "pidash://auth/callback?code=b"] });

    expect(opened).toEqual(["pidash://auth/callback?code=a", "pidash://auth/callback?code=b"]);
  });

  it("ignores empty and missing payloads", async () => {
    let listener: Listener | undefined;
    installEventApi(async (_event, handler) => {
      listener = handler;
      return () => undefined;
    });
    const opened = vi.fn();

    platform.deepLinks?.onOpenUrl(opened);
    await Promise.resolve();
    listener?.({ payload: "" });
    listener?.({ payload: [] });
    listener?.({ payload: 7 });

    expect(opened).not.toHaveBeenCalled();
  });

  it("ignores unrelated payload shapes", async () => {
    let listener: Listener | undefined;
    installEventApi(async (_event, handler) => {
      listener = handler;
      return () => undefined;
    });
    const opened = vi.fn();

    platform.deepLinks?.onOpenUrl(opened);
    await Promise.resolve();
    listener?.({ payload: undefined });
    listener?.({ payload: { url: "pidash://auth/callback" } });

    expect(opened).not.toHaveBeenCalled();
  });

  it("stops forwarding after unsubscribe and releases a late listener", async () => {
    let listener: Listener | undefined;
    let resolveListen: ((stop: () => void) => void) | undefined;
    installEventApi(
      (_event, handler) =>
        new Promise<() => void>((resolve) => {
          listener = handler;
          resolveListen = resolve;
        })
    );
    const opened = vi.fn();
    const stop = vi.fn();

    const unsubscribe = platform.deepLinks?.onOpenUrl(opened);
    unsubscribe?.();
    resolveListen?.(stop);
    await Promise.resolve();
    listener?.({ payload: "pidash://auth/callback?code=a" });

    expect(opened).not.toHaveBeenCalled();
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the registered listener", async () => {
    const stop = vi.fn();
    installEventApi(async () => stop);
    const opened = vi.fn();

    const unsubscribe = platform.deepLinks?.onOpenUrl(opened);
    await Promise.resolve();
    unsubscribe?.();

    expect(stop).toHaveBeenCalledTimes(1);
  });

  it("returns a no-op without the event bridge", () => {
    expect(() => platform.deepLinks?.onOpenUrl(() => undefined)()).not.toThrow();
  });

  it("swallows a failed listener registration", async () => {
    installEventApi(async () => {
      throw new Error("event bridge unavailable");
    });
    const opened = vi.fn();
    const unsubscribe = platform.deepLinks?.onOpenUrl(opened);
    await Promise.resolve();
    await Promise.resolve();
    expect(() => unsubscribe?.()).not.toThrow();
    expect(opened).not.toHaveBeenCalled();
  });
});
