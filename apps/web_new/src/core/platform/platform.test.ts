// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

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

  it("falls back to the local API origin without a page origin", () => {
    vi.stubGlobal("window", undefined);
    try {
      expect(webPlatform.apiOrigin).toBe("http://localhost:8000");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("exposes no desktop capabilities on the web platform", () => {
    expect(webPlatform.window).toBeUndefined();
    expect(webPlatform.menu).toBeUndefined();
    expect(webPlatform.deepLinks).toBeUndefined();
    expect(webPlatform.agentRuntime).toBeUndefined();
    // Without the local-chat seam the bundled-engine profile read never
    // fires on web, so the desktop endpoints stay desktop-only (AGT-059).
    expect(webPlatform.localChat).toBeUndefined();
    expect(webPlatform.updates).toBeUndefined();
    expect(webPlatform.sessionTeardown).toBeUndefined();
    expect(webPlatform.shellSignIn).toBeUndefined();
  });

  it("implements the agent-runtime and session-teardown seams (RUN-044, RUN-047)", () => {
    expect(desktopPlatform.agentRuntime).toBeDefined();
    expect(desktopPlatform.sessionTeardown).toBeDefined();
  });

  it("exposes the deep-link subscription on the desktop platform", () => {
    expect(typeof desktopPlatform.deepLinks?.onOpenUrl).toBe("function");
  });

  it("resolves the session wipe without the native bridge", async () => {
    await expect(desktopPlatform.sessionTeardown?.clearLocalSession()).resolves.toBeUndefined();
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

  it("closes the browser stream more than once safely", () => {
    vi.stubGlobal("fetch", () => new Promise<Response>(() => undefined));
    try {
      const stream = webPlatform.stream("https://example.test/api/events/");
      expect(() => {
        stream.close();
        stream.close();
      }).not.toThrow();
      expect(stream.error).toBeUndefined();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("falls back to the local API origin when the location is missing", () => {
    vi.stubGlobal("window", {});
    try {
      expect(webPlatform.apiOrigin).toBe("http://localhost:8000");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("falls back to the local API origin when the page origin is blank", () => {
    vi.stubGlobal("window", { location: { origin: "" } });
    try {
      expect(webPlatform.apiOrigin).toBe("http://localhost:8000");
    } finally {
      vi.unstubAllGlobals();
    }
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

describe("desktop openExternal without the shell", () => {
  it("rejects so the caller falls back to plain navigation", async () => {
    await expect(desktopPlatform.openExternal("https://example.test/")).rejects.toThrow(
      "openExternal needs the native bridge"
    );
  });
});

describe("external open address guard", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("refuses a non-web address on the web platform before opening a window", async () => {
    const open = vi.fn();
    vi.stubGlobal("window", { open });
    await expect(webPlatform.openExternal("javascript:alert(1)")).rejects.toThrow("non-web address");
    expect(open).not.toHaveBeenCalled();
  });

  it("opens a web address in a new window on the web platform", async () => {
    const open = vi.fn();
    vi.stubGlobal("window", { open });
    await webPlatform.openExternal("https://app.example.test/board");
    expect(open).toHaveBeenCalledWith("https://app.example.test/board", "_blank", "noopener");
  });

  it("refuses a non-web address on the desktop platform before invoking native code", async () => {
    const invoke = vi.fn();
    vi.stubGlobal("window", {
      __PIDASH_NATIVE_HTTP__: "https://api.test",
      __TAURI__: { core: { invoke, Channel: class {} } },
    });
    await expect(desktopPlatform.openExternal("file:///etc/passwd")).rejects.toThrow("non-web address");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("opens a web address through the native command", async () => {
    const invoke = vi.fn(async () => undefined);
    vi.stubGlobal("window", {
      __PIDASH_NATIVE_HTTP__: "https://api.test",
      __TAURI__: { core: { invoke, Channel: class {} } },
    });
    await desktopPlatform.openExternal("https://app.example.test/");
    expect(invoke).toHaveBeenCalledWith("plugin:opener|open_url", { url: "https://app.example.test/" });
  });

  it("uses the browser fallback when the native open rejects", async () => {
    const open = vi.fn();
    const invoke = vi.fn(async () => {
      throw new Error("opener unavailable");
    });
    vi.stubGlobal("window", {
      open,
      __PIDASH_NATIVE_HTTP__: "https://api.test",
      __TAURI__: { core: { invoke, Channel: class {} } },
    });
    await desktopPlatform.openExternal("https://app.example.test/");
    expect(open).toHaveBeenCalledWith("https://app.example.test/", "_blank", "noopener");
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

  it("maps the multipart channel onto the request body", async () => {
    const seen: { body?: unknown }[] = [];
    const fake: Platform = {
      ...webPlatform,
      fetch: (async (_input: unknown, init?: { body?: unknown }) => {
        seen.push({ body: init?.body });
        return new Response("{}", { status: 200 });
      }) as typeof fetch,
    };
    const transport = toApiTransport(fake);
    const form = new FormData();
    form.append("file", new Blob(["audio"]), "dictation.webm");
    await transport("https://example.test/api/upload/", {
      method: "POST",
      headers: {},
      formData: form,
      credentials: "include",
    });
    expect(seen[0]?.body).toBe(form);
    await transport("https://example.test/api/x/", {
      method: "POST",
      headers: {},
      body: '{"a":1}',
      credentials: "include",
    });
    expect(seen[1]?.body).toBe('{"a":1}');
  });
});
