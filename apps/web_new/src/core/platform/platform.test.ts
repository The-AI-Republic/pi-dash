// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import { platform } from "./index.js";
import { createMemoryStore } from "./storage.js";
import { toApiTransport } from "./transport.js";
import { platform as desktopPlatform } from "./tauri.js";
import { decodeFrame, encodeFrame } from "./tauri.js";
import type { Platform } from "./types.js";
import { platform as webPlatform } from "./web.js";

describe("platform selection", () => {
  it("resolves the web implementation through the index", () => {
    expect(platform.kind).toBe("web");
    expect(platform).toBe(webPlatform);
  });

  it("builds a distinct desktop implementation", () => {
    expect(desktopPlatform.kind).toBe("desktop");
    expect(desktopPlatform).not.toBe(webPlatform);
  });

  it("exposes no desktop capabilities on the web platform", () => {
    expect(webPlatform.window).toBeUndefined();
    expect(webPlatform.menu).toBeUndefined();
    expect(webPlatform.deepLinks).toBeUndefined();
    expect(webPlatform.agentRuntime).toBeUndefined();
    expect(webPlatform.updates).toBeUndefined();
  });

  it("exposes the title-bar capability on the desktop platform", () => {
    expect(typeof desktopPlatform.window?.setTitle).toBe("function");
    expect(typeof desktopPlatform.window?.trafficLightInset).toBe("boolean");
  });
});

describe("memory store", () => {
  it("rounds-trips values and clears by namespace", async () => {
    const store = createMemoryStore();
    await store.set("cache", "payload");
    expect(await store.get("cache")).toBe("payload");
    await store.remove("cache");
    expect(await store.get("cache")).toBeNull();
    await store.set("a", "1");
    await store.clear();
    expect(await store.get("a")).toBeNull();
  });
});

describe("web platform fallbacks without a browser", () => {
  it("stores through the in-memory fallback", async () => {
    await webPlatform.storage.set("key", "value");
    expect(await webPlatform.storage.get("key")).toBe("value");
    await webPlatform.storage.remove("key");
    expect(await webPlatform.storage.get("key")).toBeNull();
  });

  it("returns a no-op unsubscribe for focus tracking", () => {
    expect(() => webPlatform.onFocusChange(() => undefined)()).not.toThrow();
  });
});

describe("desktop transport without the shell", () => {
  it("delegates to the browser transport when the shell is absent", async () => {
    const seen: string[] = [];
    const stubFetch = vi.fn(async (input: RequestInfo | URL) => {
      seen.push(String(input));
      return new Response("ok");
    });
    vi.stubGlobal("fetch", stubFetch);
    try {
      const response = await desktopPlatform.fetch("https://example.test/api/users/me/");
      expect(await response.text()).toBe("ok");
      expect(seen).toEqual(["https://example.test/api/users/me/"]);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("native frame codec", () => {
  it("round-trips a head and body", () => {
    const head = { id: "1", url: "https://example.test/api/x/", method: "POST" };
    const body = new TextEncoder().encode('{"a":1}');
    const decoded = decodeFrame(encodeFrame(head, body));
    expect(decoded.head).toEqual(head);
    expect(decoded.body).toEqual(body);
  });

  it("rejects truncated frames", () => {
    expect(() => decodeFrame(new Uint8Array([0, 0]))).toThrow();
    const full = encodeFrame({ a: 1 }, null);
    expect(() => decodeFrame(full.subarray(0, 5))).toThrow();
  });
});

describe("toApiTransport", () => {
  it("adapts platform.fetch to the api-client transport shape", async () => {
    const fake: Platform = {
      ...webPlatform,
      fetch: (async () =>
        new Response(JSON.stringify({ hello: 1 }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })) as typeof fetch,
    };
    const transport = toApiTransport(fake);
    const response = await transport("https://example.test/api/x/", {
      method: "GET",
      headers: {},
      credentials: "include",
    });
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toContain("application/json");
    expect(await response.text()).toBe(JSON.stringify({ hello: 1 }));
  });
});
