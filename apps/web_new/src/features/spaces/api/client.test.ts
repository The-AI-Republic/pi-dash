// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Transport } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createSpaceRuntime } from "./client.js";

const transport: Transport = async (url) => ({
  status: 200,
  url,
  headers: { get: () => null },
  text: () => Promise.resolve("{}"),
});

afterEach(() => {
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

  it("falls back to the page origin", () => {
    vi.stubGlobal("window", { location: { origin: "https://boards.test" } });
    expect(createSpaceRuntime(undefined, transport).client.baseUrl).toBe("https://boards.test");
  });

  it("falls back to the local API origin outside a browser", () => {
    vi.stubGlobal("window", undefined);
    expect(createSpaceRuntime(undefined, transport).client.baseUrl).toBe("http://localhost:8000");
  });
});
