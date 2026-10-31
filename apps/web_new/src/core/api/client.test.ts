// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The app client's default address (DESK-020): the native shell's API
// origin wins because the desktop bundle's own origin is private; the page
// origin and then the local Django address cover web and no-window runs.

import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "./client.js";
import type { Platform } from "../platform/index.js";

const platformHarness = vi.hoisted(() => ({ current: {} as Partial<Platform> }));
const editionHarness = vi.hoisted(() => ({ api: undefined as unknown }));
vi.mock("../platform/index.js", () => ({
  get platform() {
    return platformHarness.current;
  },
  toApiTransport: () => async () => new Response("{}"),
}));
vi.mock("../edition/index.js", () => ({
  get edition() {
    return { id: "oss", flags: {}, api: editionHarness.api };
  },
}));
vi.mock("../session/middleware.js", () => ({
  createSessionMiddleware: () => async (request: unknown, next: (value: unknown) => unknown) => next(request),
}));
vi.mock("../session/queries.js", () => ({ setSessionClient: () => undefined }));

afterEach(() => {
  platformHarness.current = {};
  editionHarness.api = undefined;
  resetAppClient();
  vi.unstubAllGlobals();
});

describe("app client default base address", () => {
  it("takes the native API origin when the desktop shell advertises one", () => {
    platformHarness.current = { kind: "desktop", apiOrigin: "https://api.desktop.test" };
    expect(createAppClient().baseUrl).toBe("https://api.desktop.test");
  });

  it("uses the page origin when the platform advertises none", () => {
    platformHarness.current = { kind: "web" };
    vi.stubGlobal("window", { location: { origin: "https://app.example.test" } });
    expect(createAppClient().baseUrl).toBe("https://app.example.test");
    expect(getAppClient().baseUrl).toBe("https://app.example.test");
  });

  it("falls back to the local Django address without a window", () => {
    platformHarness.current = { kind: "web", apiOrigin: undefined };
    vi.stubGlobal("window", undefined);
    expect(createAppClient().baseUrl).toBe("http://localhost:8000");
  });

  it("runs the edition interceptors inside the session layer", async () => {
    platformHarness.current = { kind: "web", apiOrigin: "https://app.example.test" };
    const seen: string[] = [];
    editionHarness.api = [
      async (request: { url: string }, next: (value: unknown) => Promise<unknown>) => {
        seen.push(request.url);
        return next(request);
      },
    ];
    const client = createAppClient();
    await client.getJson("/api/ping/");
    expect(seen).toEqual(["https://app.example.test/api/ping/"]);
  });
});
