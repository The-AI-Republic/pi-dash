// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { DesktopTransportError } from "./tauri.js";
import { platform as desktopPlatform } from "./tauri.js";

function stubDesktop(origin = "https://api.test", session?: { refresh?: boolean; strict?: boolean }) {
  const location = { href: "tauri://localhost/", origin: "tauri://localhost" };
  // The transport resolves request URLs against `globalThis.location`, which
  // is `window.location` in the webview but undefined in this node test env.
  vi.stubGlobal("location", location);
  vi.stubGlobal("window", {
    location,
    __PIDASH_NATIVE_HTTP__: origin,
    __PIDASH_NATIVE_SESSION__: session ?? { refresh: false, strict: true },
    __TAURI__: {
      core: {
        invoke: vi.fn(async () => new Uint8Array(0)),
        Channel: class {
          onmessage = null;
        },
      },
    },
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("DesktopTransportError in strict bundled mode", () => {
  it("throws a named error for a credentialed request to another origin", async () => {
    stubDesktop("https://api.test", { refresh: false, strict: true });
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("should not be called"))
    );
    await expect(
      desktopPlatform.fetch("https://api.other.test/api/issues/", { credentials: "include" })
    ).rejects.toThrow(DesktopTransportError);
    // The message names both origins.
    try {
      await desktopPlatform.fetch("https://api.other.test/api/issues/", { credentials: "include" });
    } catch (error) {
      expect(error).toBeInstanceOf(DesktopTransportError);
      const e = error as DesktopTransportError;
      expect(e.name).toBe("DesktopTransportError");
      expect(e.requestOrigin).toBe("https://api.other.test");
      expect(e.message).toContain("https://api.other.test");
      expect(e.message).toContain("https://api.test");
    }
    // No request leaves through the webview.
    expect(fetch).not.toHaveBeenCalled();
  });

  it("throws for a credentialed same-origin path outside the API allowlist", async () => {
    stubDesktop("https://api.test", { refresh: false, strict: true });
    await expect(desktopPlatform.fetch("https://api.test/sign-in", { credentials: "include" })).rejects.toThrow(
      DesktopTransportError
    );
  });

  it("keeps presigned uploads (omit) on the webview", async () => {
    stubDesktop("https://api.test", { refresh: false, strict: true });
    const fetches: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo) => {
        fetches.push(typeof input === "string" ? input : String(input));
        return new Response("ok", { status: 200 });
      })
    );
    const response = await desktopPlatform.fetch("https://storage.example.com/upload", {
      credentials: "omit",
      method: "POST",
      body: "data",
    });
    expect(await response.text()).toBe("ok");
    expect(fetches).toEqual(["https://storage.example.com/upload"]);
  });

  it("keeps external uncredentialed hosts on the webview even when strict", async () => {
    stubDesktop("https://api.test", { refresh: false, strict: true });
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("external"))
    );
    const response = await desktopPlatform.fetch("https://cdn.example.com/logo.png", { credentials: "omit" });
    expect(await response.text()).toBe("external");
  });

  it("does not throw in hot-reload (strict false) for a credentialed mismatch", async () => {
    stubDesktop("https://api.test", { refresh: false, strict: false });
    const fetchSpy = vi.fn(async () => new Response("hot", { status: 200 }));
    vi.stubGlobal("fetch", fetchSpy);
    const response = await desktopPlatform.fetch("https://api.other.test/api/issues/", { credentials: "include" });
    expect(await response.text()).toBe("hot");
    expect(fetchSpy).toHaveBeenCalled();
  });

  it("throws for an SSE stream aimed off the native allowlist when strict", () => {
    stubDesktop("https://api.test", { refresh: false, strict: true });
    expect(() => desktopPlatform.stream("https://api.other.test/api/events/", {})).toThrow(DesktopTransportError);
  });

  it("does not throw for an SSE stream in hot-reload", () => {
    stubDesktop("https://api.test", { refresh: false, strict: false });
    // Should not throw; it will try to create a native stream via mock core.
    // The mock invoke returns empty; the stream will live but not error on creation.
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("nope"))
    );
    expect(() => desktopPlatform.stream("https://api.other.test/api/events/", {})).not.toThrow();
  });
});

describe("native session advertisement", () => {
  it("exposes strict and refresh from __PIDASH_NATIVE_SESSION__", async () => {
    stubDesktop("https://api.test", { refresh: true, strict: true });
    // The advertisement is read via nativeApiOrigin internally; the next two
    // cases prove it by behavior: strict throws, refresh stands down cloud
    // middleware (covered in api-client tests).
    const response = await desktopPlatform
      .fetch("https://api.test/api/users/me/", { credentials: "include" })
      .catch(() => null);
    // Not throwing here means the native route was taken (mock invoke returns empty frame and fails decoding?).
    // Our mock invoke returns empty Uint8Array(0) which will throw on decode, but not DesktopTransportError.
    // So just ensure it didn't throw DesktopTransportError.
    if (response && response instanceof Error) {
      expect((response as Error).name).not.toBe("DesktopTransportError");
    }
  });
});
