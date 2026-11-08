// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { afterEach, vi } from "vitest";

import { classifyDesktopRoute, currentAddress, hostedAddressFor } from "./desktopHandOff.js";
import { assertExclusionsAreMigrated, WEB_ONLY_DESKTOP_EXCLUSIONS, isWebOnlyDesktopPath } from "./webOnlyRoutes.js";

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
      "/acme/runners/runs/run-1",
    ]) {
      expect(classifyDesktopRoute(path)).toEqual({ kind: "in-app" });
    }
  });

  it("hands the web-only subtrees to the browser even though the list claims them", () => {
    // The instance console and public boards are mounted in the web build
    // only (desktop spec §11, D13), so a desktop window cannot render them
    // even though the proxy serves them from the same list.
    for (const path of [
      "/god-mode",
      "/god-mode/",
      "/god-mode/parity-workspace",
      "/god-mode/users?page=2",
      "/spaces",
      "/spaces/parity-anchor/",
      "/spaces/acme/project-1?view=board#top",
    ]) {
      expect(classifyDesktopRoute(path)).toEqual({ kind: "hand-off", path });
    }
  });

  it("treats look-alike names as ordinary addresses, not the excluded subtree", () => {
    for (const path of ["/god-mode-ish", "/spaceship"]) {
      expect(isWebOnlyDesktopPath(path)).toBe(false);
      // No dot, not reserved: these read as workspace slugs and stay in the
      // window as the in-app workspace-not-found screen.
      expect(classifyDesktopRoute(path)).toEqual({ kind: "in-app" });
    }
    // A reserved top-level name that is not a workspace: old-app screen.
    expect(isWebOnlyDesktopPath("/acme/spaces/board")).toBe(false);
    expect(classifyDesktopRoute("/acme/spaces/board")).toEqual({ kind: "hand-off", path: "/acme/spaces/board" });
  });

  it("keeps the exclusion list inside the migrated-route list", () => {
    expect(() => assertExclusionsAreMigrated()).not.toThrow();
    expect(() => assertExclusionsAreMigrated(["/god-mode"], ["/god-mode"])).not.toThrow();
    expect(() => assertExclusionsAreMigrated(["/not-a-listed-screen"], ["/god-mode"])).toThrow(
      /not in migrated-routes\.json/
    );
    expect(() => assertExclusionsAreMigrated([], [])).not.toThrow();
  });

  it("names the console and the public boards as web-only", () => {
    expect([...WEB_ONLY_DESKTOP_EXCLUSIONS].sort()).toEqual(["/god-mode", "/spaces"]);
  });

  it("recognizes the excluded subtrees with the migrated-route rules", () => {
    expect(isWebOnlyDesktopPath("/god-mode")).toBe(true);
    expect(isWebOnlyDesktopPath("/god-mode/")).toBe(true);
    expect(isWebOnlyDesktopPath("/god-mode/users")).toBe(true);
    expect(isWebOnlyDesktopPath("/spaces/board/?a=1")).toBe(true);
    expect(isWebOnlyDesktopPath("/spacesx")).toBe(false);
    expect(isWebOnlyDesktopPath("/acme")).toBe(false);
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
    // An explicit list describes the deployed app, not this bundle, so it
    // keeps the web-only subtrees it claims (the proxy's own view).
    expect(classifyDesktopRoute("/god-mode", ["/god-mode"])).toEqual({ kind: "in-app" });
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
