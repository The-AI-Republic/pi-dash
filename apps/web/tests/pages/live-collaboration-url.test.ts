/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

const constants = vi.hoisted(() => ({ LIVE_BASE_URL: "", LIVE_BASE_PATH: "/live" }));

vi.mock("@pi-dash/constants", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pi-dash/constants")>()),
  get LIVE_BASE_URL() {
    return constants.LIVE_BASE_URL;
  },
  get LIVE_BASE_PATH() {
    return constants.LIVE_BASE_PATH;
  },
}));

import { liveCollaborationUrl } from "@/utils/live-collaboration-url";

const onPage = (origin: string) =>
  vi.stubGlobal("location", { ...window.location, protocol: new URL(origin).protocol, origin });

afterEach(() => {
  vi.unstubAllGlobals();
  constants.LIVE_BASE_URL = "";
  constants.LIVE_BASE_PATH = "/live";
});

describe("live collaboration URL on the web", () => {
  it.each([
    ["https://pidash.example.com", "wss://pidash.example.com/live/collaboration"],
    ["http://localhost:3000", "ws://localhost:3000/live/collaboration"],
  ])("uses the page origin when no live host is baked (%s)", (origin, expected) => {
    onPage(origin);

    expect(liveCollaborationUrl().toString()).toBe(expected);
  });

  it("uses the baked live host and its own scheme", () => {
    onPage("http://localhost:3000");
    constants.LIVE_BASE_URL = "http://localhost:3100";

    expect(liveCollaborationUrl({ workspaceSlug: "acme", projectId: null }).toString()).toBe(
      "ws://localhost:3100/live/collaboration?workspaceSlug=acme"
    );
  });
});
