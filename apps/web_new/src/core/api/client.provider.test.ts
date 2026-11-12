// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider-credential client (NEWFRONT-421). A rejected GitHub or GitLab
// token answers 401, which is not an expired session: writes made through
// the provider client leave the session alone, while every other request
// still expires it through the app client's middleware.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Transport, TransportResponse } from "@pidash/api-client";

import type { Platform } from "../platform/index.js";
import { useSessionStore } from "../session/store.js";
import { createAppClient, getAppClient, getProviderWriteClient, resetAppClient } from "./client.js";

const platformHarness = vi.hoisted(() => ({
  current: {} as Partial<Platform>,
}));

vi.mock("../platform/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    get platform() {
      return platformHarness.current;
    },
  };
});

function response(status: number, body: string): TransportResponse {
  return {
    status,
    url: "https://api.test/api/workspaces/acme/integrations/github/connect/",
    headers: { get: () => "application/json" },
    text: async () => body,
  };
}

beforeEach(() => {
  useSessionStore.setState({ status: "authenticated", returnUrl: null });
});

afterEach(() => {
  platformHarness.current = {};
  resetAppClient();
  vi.unstubAllGlobals();
});

describe("provider write client", () => {
  it("shares the origin and transport but skips the session-expiry middleware", async () => {
    const transport: Transport = async (url) =>
      url.includes("/auth/get-csrf-token/")
        ? response(200, JSON.stringify({ csrf_token: "token" }))
        : response(401, JSON.stringify({ detail: "bad token" }));
    const app = createAppClient("https://api.test", transport);
    const provider = getProviderWriteClient();
    expect(provider).not.toBe(app);
    expect(provider.baseUrl).toBe(app.baseUrl);
    await expect(
      provider.sendJson("POST", "/api/workspaces/acme/integrations/github/connect/", { json: {} })
    ).rejects.toThrow();
    expect(useSessionStore.getState().status).toBe("authenticated");
  });

  it("still expires the session for the same 401 through the app client", async () => {
    const transport: Transport = async (url) =>
      url.includes("/auth/get-csrf-token/")
        ? response(200, JSON.stringify({ csrf_token: "token" }))
        : response(401, JSON.stringify({ detail: "expired" }));
    createAppClient("https://api.test", transport);
    await expect(
      getAppClient().sendJson("POST", "/api/workspaces/acme/integrations/github/connect/", { json: {} })
    ).rejects.toThrow();
    expect(useSessionStore.getState().status).toBe("expired");
  });

  it("reuses one sibling client and drops it when the app client resets", () => {
    createAppClient("https://api.test", async () => response(200, "{}"));
    const first = getProviderWriteClient();
    expect(getProviderWriteClient()).toBe(first);
    resetAppClient();
    expect(() => getProviderWriteClient()).toThrow(/not configured/i);
    createAppClient("https://api.test", async () => response(200, "{}"));
    expect(getProviderWriteClient()).not.toBe(first);
  });

  it("throws before bootstrap like the app client", () => {
    expect(() => getProviderWriteClient()).toThrow(/not configured/i);
  });

  it("refuses the app client before bootstrap", () => {
    expect(() => getAppClient()).toThrow(/not configured/i);
  });

  it("addresses the desktop shell's advertised API origin when none is passed", () => {
    platformHarness.current = { kind: "desktop", apiOrigin: "https://api.desktop.test" };
    const app = createAppClient(undefined, async () => response(200, "{}"));
    expect(app.baseUrl).toBe("https://api.desktop.test");
    expect(getProviderWriteClient().baseUrl).toBe("https://api.desktop.test");
  });

  it("sends provider writes through the platform transport when bootstrap passed none", async () => {
    const platformCalls: string[] = [];
    platformHarness.current = {
      kind: "desktop",
      apiOrigin: "https://api.desktop.test",
      fetch: async (url) => {
        platformCalls.push(String(url));
        return new Response(JSON.stringify({ detail: "bad token" }), {
          status: 401,
          headers: { "content-type": "application/json" },
        });
      },
    };
    const webFetch = vi.fn(async () => new Response("{}"));
    vi.stubGlobal("fetch", webFetch);

    createAppClient();
    await expect(
      getProviderWriteClient().sendJson("POST", "/api/workspaces/acme/integrations/github/connect/", { json: {} })
    ).rejects.toThrow();

    // The desktop build must reach the API through the shell bridge, never
    // through the page's own fetch (a different origin, no credentials).
    expect(platformCalls.some((url) => url.includes("/integrations/github/connect/"))).toBe(true);
    expect(webFetch).not.toHaveBeenCalled();
    expect(useSessionStore.getState().status).toBe("authenticated");
  });
});
