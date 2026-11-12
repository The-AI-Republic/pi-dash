// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import {
  bakedWebBaseUrl,
  describeDesktopSignInFailure,
  fallbackToPlainNavigation,
  parseBakedWebBaseUrl,
  parseDesktopEntrySearch,
  resolveBrowserAppUrl,
} from "./desktop.js";

describe("desktop entry search", () => {
  it("reads the failure landing plus a safe pending destination", () => {
    expect(
      parseDesktopEntrySearch({
        error: "access_denied",
        error_description: "The provider refused.",
        next: "/acme",
      })
    ).toEqual({ error: "access_denied", errorDescription: "The provider refused.", nextPath: "/acme" });
  });

  it("drops invalid params instead of throwing", () => {
    expect(parseDesktopEntrySearch({ error: "", error_description: 7, next_path: "https://evil.test/x" })).toEqual({});
    expect(parseDesktopEntrySearch({})).toEqual({});
  });

  it("prefers next_path over the next alias", () => {
    expect(parseDesktopEntrySearch({ next_path: "/canonical", next: "/alias" })).toEqual({
      nextPath: "/canonical",
    });
  });
});

describe("desktop sign-in failure", () => {
  it("stays silent without ?error=", () => {
    expect(describeDesktopSignInFailure({})).toBeNull();
  });

  it("echoes the provider description", () => {
    const failure = describeDesktopSignInFailure({ error: "access_denied", errorDescription: "The provider refused." });
    expect(failure?.body).toBe("The provider refused.");
    expect(failure?.heading.length).toBeGreaterThan(0);
  });

  it("names the error when no description arrived", () => {
    const failure = describeDesktopSignInFailure({ error: "access_denied" });
    expect(failure?.body).toContain("access_denied");
  });
});

describe("browser-app URL resolution", () => {
  it("prefers the baked web base URL", () => {
    expect(resolveBrowserAppUrl("https://app.example.test/", "https://api.example.test/")).toBe(
      "https://app.example.test/"
    );
  });

  it("falls back to the API origin", () => {
    expect(resolveBrowserAppUrl(undefined, "https://api.example.test/api/v1")).toBe("https://api.example.test");
  });

  it("passes an unparseable API base through", () => {
    expect(resolveBrowserAppUrl(undefined, "not-a-url")).toBe("not-a-url");
  });

  it("treats an empty bake as absent", () => {
    expect(parseBakedWebBaseUrl("https://app.example.test")).toBe("https://app.example.test");
    expect(parseBakedWebBaseUrl("")).toBeUndefined();
    expect(parseBakedWebBaseUrl(undefined)).toBeUndefined();
  });

  it("navigates plainly when no native bridge answers", () => {
    const assign = vi.fn();
    vi.stubGlobal("window", { location: { assign } });
    try {
      fallbackToPlainNavigation("https://app.example.test/");
    } finally {
      vi.unstubAllGlobals();
    }
    expect(assign).toHaveBeenCalledWith("https://app.example.test/");
  });

  it("reads the bake out of the build define", () => {
    expect(bakedWebBaseUrl()).toBeUndefined();
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "https://app.example.test/");
    try {
      expect(bakedWebBaseUrl()).toBe("https://app.example.test/");
    } finally {
      vi.unstubAllGlobals();
    }
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "");
    try {
      expect(bakedWebBaseUrl()).toBeUndefined();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
