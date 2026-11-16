// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The hosted base the desktop bundle bakes (DESK-019) must come from the
// variable the desktop pipeline actually sets, with the newer name taking
// precedence and blank values ignored.

import { describe, expect, it } from "vitest";

import { hostedBaseUrlFromEnv } from "./hostedBaseUrl.js";

describe("hosted base from the build environment", () => {
  it("uses VITE_WEB_BASE_URL when that is the only name set", () => {
    expect(hostedBaseUrlFromEnv({ VITE_WEB_BASE_URL: "https://app.example.test" })).toBe("https://app.example.test");
  });

  it("prefers PIDASH_WEB_BASE_URL when both are set", () => {
    expect(
      hostedBaseUrlFromEnv({
        PIDASH_WEB_BASE_URL: "https://new.example.test",
        VITE_WEB_BASE_URL: "https://old.example.test",
      })
    ).toBe("https://new.example.test");
  });

  it("skips blank values instead of masking the other name", () => {
    expect(hostedBaseUrlFromEnv({ PIDASH_WEB_BASE_URL: "", VITE_WEB_BASE_URL: "https://app.example.test" })).toBe(
      "https://app.example.test"
    );
    expect(hostedBaseUrlFromEnv({ PIDASH_WEB_BASE_URL: "   ", VITE_WEB_BASE_URL: "" })).toBeNull();
  });

  it("reports no base when neither variable is set", () => {
    expect(hostedBaseUrlFromEnv({})).toBeNull();
    expect(hostedBaseUrlFromEnv({ PIDASH_WEB_BASE_URL: undefined, VITE_WEB_BASE_URL: undefined })).toBeNull();
  });
});
