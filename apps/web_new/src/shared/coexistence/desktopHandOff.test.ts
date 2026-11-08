// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { afterEach, vi } from "vitest";

import { classifyDesktopRoute, currentAddress, hostedAddressFor } from "./desktopHandOff.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("classifyDesktopRoute", () => {
  it("keeps the root entry in the window, with or without a query", () => {
    for (const path of ["", "/", "//", "/?next_path=%2Facme", "/#top", "?next_path=%2Facme", "#top"]) {
      expect(classifyDesktopRoute(path)).toEqual({ kind: "in-app" });
    }
  });

  it("keeps addresses the migrated-route list claims in the window", () => {
    for (const path of [
      "/login",
      "/sign-in?error=access_denied",
      "/auth/device/",
      "/acme",
      "/acme/projects/project-1/issues",
      "/acme/settings/account/notifications/",
      "/acme/drafts",
      "/acme/analytics/overview",
      "/acme/ai-dev-machines",
      "/spaces/parity-anchor/",
      "/acme/runners/runs/run-1",
    ]) {
      expect(classifyDesktopRoute(path)).toEqual({ kind: "in-app" });
    }
  });

  it("hands addresses another frontend owns to the browser at the same path", () => {
    for (const path of [
      "/acme/projects/archives",
      "/acme/notifications",
      "/acme/profile/1234",
      "/__parity_no_such_ws__/__parity_no_such_page__",
    ]) {
      expect(classifyDesktopRoute(path)).toEqual({ kind: "hand-off", path });
    }
  });

  it("keeps the query and fragment of a handed-off address", () => {
    const path = "/acme/projects/archives?tab=modules#archived-1";
    expect(classifyDesktopRoute(path)).toEqual({ kind: "hand-off", path });
  });

  it("accepts an explicit route list for previews and tests", () => {
    const entries = ["/acme/notifications"];
    expect(classifyDesktopRoute("/acme/notifications", entries)).toEqual({ kind: "in-app" });
    expect(classifyDesktopRoute("/acme/drafts", entries)).toEqual({ kind: "hand-off", path: "/acme/drafts" });
    // The root stays in the window even with a list that claims nothing.
    expect(classifyDesktopRoute("/", entries)).toEqual({ kind: "in-app" });
  });
});

describe("hostedAddressFor", () => {
  it("joins a relative path to the configured origin", () => {
    expect(hostedAddressFor("/spaces/board/", "https://app.example.test")).toBe(
      "https://app.example.test/spaces/board/"
    );
  });

  it("keeps the query and fragment", () => {
    expect(hostedAddressFor("/acme/notifications?tab=all#today", "https://app.example.test/")).toBe(
      "https://app.example.test/acme/notifications?tab=all#today"
    );
  });

  it("honours a configured origin's path prefix", () => {
    expect(hostedAddressFor("/spaces/board/", "https://example.test/pi-dash")).toBe(
      "https://example.test/spaces/board/"
    );
  });

  it("passes an absolute web address through unchanged", () => {
    expect(hostedAddressFor("https://admin.example.test/settings", undefined)).toBe(
      "https://admin.example.test/settings"
    );
  });

  it("refuses without a configured origin instead of inventing one", () => {
    expect(() => hostedAddressFor("/spaces/board/", undefined)).toThrow(/no hosted web app address/i);
    expect(() => hostedAddressFor("/spaces/board/", "   ")).toThrow(/no hosted web app address/i);
  });

  it("refuses an origin that is not a web address", () => {
    expect(() => hostedAddressFor("/spaces/board/", "tauri://localhost")).toThrow(/no hosted web app address/i);
    expect(() => hostedAddressFor("/spaces/board/", "not a url")).toThrow(/no hosted web app address/i);
  });
});

describe("currentAddress", () => {
  it("reads the path, query and fragment the window shows", () => {
    window.history.replaceState(null, "", "/acme/projects/archives?tab=modules#archived-1");
    expect(currentAddress()).toBe("/acme/projects/archives?tab=modules#archived-1");
  });

  it("falls back to the root without a window (a prerendered document)", () => {
    vi.stubGlobal("window", undefined);
    expect(currentAddress()).toBe("/");
  });
});
