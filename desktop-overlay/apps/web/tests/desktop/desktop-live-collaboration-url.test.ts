/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

// A bundled desktop page runs on a Tauri-owned origin and a desktop build
// bakes no VITE_LIVE_BASE_URL, so a live URL resolved against
// window.location.origin comes out as tauri://localhost/collaboration — a
// scheme WebSocket rejects without ever emitting the close event the editor
// waits on to fall back to REST, so an uncached page never leaves its loader.
const constants = vi.hoisted(() => ({
  API_BASE_URL: "https://apipidash.airepublic.com",
  LIVE_BASE_URL: "",
  LIVE_BASE_PATH: "",
}));

vi.mock("@pi-dash/constants", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pi-dash/constants")>()),
  get API_BASE_URL() {
    return constants.API_BASE_URL;
  },
  get LIVE_BASE_URL() {
    return constants.LIVE_BASE_URL;
  },
  get LIVE_BASE_PATH() {
    return constants.LIVE_BASE_PATH;
  },
}));

import { liveCollaborationUrl } from "../../core/utils/live-collaboration-url";

const PLATFORMS = [
  ["macOS and Linux", "tauri:", "tauri://localhost"],
  ["Windows", "http:", "http://tauri.localhost"],
] as const;

const onPage = (protocol: string, origin: string) =>
  vi.stubGlobal("location", { ...window.location, protocol, origin, href: `${origin}/acme/pages/page-id` });

afterEach(() => {
  vi.unstubAllGlobals();
  constants.API_BASE_URL = "https://apipidash.airepublic.com";
  constants.LIVE_BASE_URL = "";
  constants.LIVE_BASE_PATH = "";
});

describe("desktop live collaboration URL", () => {
  it.each(PLATFORMS)("targets the server's /live mount on %s (page origin %s)", (_platform, protocol, origin) => {
    onPage(protocol, origin);

    const url = liveCollaborationUrl({ workspaceSlug: "acme", projectId: "project-id", documentType: undefined });

    expect(url.toString()).toBe(
      "wss://apipidash.airepublic.com/live/collaboration?workspaceSlug=acme&projectId=project-id"
    );
  });

  it.each(PLATFORMS)("uses ws:// for an http server on %s", (_platform, protocol, origin) => {
    onPage(protocol, origin);
    constants.API_BASE_URL = "http://localhost:8000";

    expect(liveCollaborationUrl().toString()).toBe("ws://localhost:8000/live/collaboration");
  });

  it("prefers a baked VITE_LIVE_BASE_URL and VITE_LIVE_BASE_PATH", () => {
    onPage("tauri:", "tauri://localhost");
    constants.LIVE_BASE_URL = "https://live.example.com";
    constants.LIVE_BASE_PATH = "/rt";

    expect(liveCollaborationUrl().toString()).toBe("wss://live.example.com/rt/collaboration");
  });

  it("refuses to build a URL without a server origin rather than falling back to the page", () => {
    onPage("tauri:", "tauri://localhost");
    constants.API_BASE_URL = "";

    expect(() => liveCollaborationUrl()).toThrow(/VITE_API_BASE_URL/);
  });
});
