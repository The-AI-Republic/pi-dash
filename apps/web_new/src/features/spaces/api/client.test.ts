// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Transport } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

const host = vi.hoisted(() => ({
  apiOrigin: undefined as string | undefined,
  transport: (async (url: string) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: () => Promise.resolve("{}"),
  })) as Transport,
}));

vi.mock("../../../core/platform/index.js", () => ({
  platform: {
    get apiOrigin() {
      return host.apiOrigin;
    },
  },
  toApiTransport: () => host.transport,
}));

import { createSpaceRuntime } from "./client.js";

const transport: Transport = host.transport;

afterEach(() => {
  host.apiOrigin = undefined;
  host.transport = transport;
  vi.unstubAllGlobals();
});

describe("createSpaceRuntime", () => {
  it("uses an explicit base URL and transport without a workspace session", () => {
    const runtime = createSpaceRuntime("https://api.test/", transport);
    expect(runtime.client.baseUrl).toBe("https://api.test");
    expect(runtime.queryClient.getDefaultOptions().queries).toMatchObject({
      retry: false,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
      staleTime: 30_000,
    });
  });

  it("prefers the platform API origin", () => {
    host.apiOrigin = "https://api.platform.test";
    expect(createSpaceRuntime(undefined, transport).client.baseUrl).toBe("https://api.platform.test");
  });

  it("falls back to the page origin when the platform has no API origin", () => {
    vi.stubGlobal("window", { location: { origin: "https://boards.test" } });
    expect(createSpaceRuntime(undefined, transport).client.baseUrl).toBe("https://boards.test");
  });

  it("falls back to the local API origin outside a browser", () => {
    vi.stubGlobal("window", undefined);
    expect(createSpaceRuntime(undefined, transport).client.baseUrl).toBe("http://localhost:8000");
  });

  it("builds the platform transport and the module runtime at import time", async () => {
    vi.resetModules();
    const module = await import("./client.js");
    expect(module.spaceRuntime.client.baseUrl).toBe("http://localhost:8000");
    expect(module.createSpaceRuntime("https://api.test").client.baseUrl).toBe("https://api.test");
  });
});
