// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { isWebAddress } from "./urls.js";

describe("isWebAddress", () => {
  it("accepts absolute http and https addresses", () => {
    expect(isWebAddress("https://app.example.test/path?q=1")).toBe(true);
    expect(isWebAddress("http://localhost:8000/")).toBe(true);
  });

  it("refuses every other scheme", () => {
    for (const value of [
      "javascript:alert(1)",
      "file:///etc/passwd",
      "mailto:ada@example.test",
      "pidash://auth/callback",
    ]) {
      expect(isWebAddress(value)).toBe(false);
    }
  });

  it("refuses relative and malformed values", () => {
    expect(isWebAddress("/acme")).toBe(false);
    expect(isWebAddress("not a url")).toBe(false);
    expect(isWebAddress("")).toBe(false);
  });
});
