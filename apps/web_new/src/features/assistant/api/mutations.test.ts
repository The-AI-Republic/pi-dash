// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { START_FAILED_FALLBACK, startFailureDetail } from "./mutations.js";

describe("startFailureDetail", () => {
  it("prefers the normalized server message", () => {
    expect(startFailureDetail(new ApiError({ status: 403, code: "role_not_allowed", message: "members only" }))).toBe(
      "members only"
    );
  });

  it("falls back to a plain error message, then to our copy", () => {
    expect(startFailureDetail(new Error("offline"))).toBe("offline");
    expect(startFailureDetail(new Error("  "))).toBe(START_FAILED_FALLBACK);
    expect(startFailureDetail("boom")).toBe(START_FAILED_FALLBACK);
    expect(startFailureDetail(null)).toBe(START_FAILED_FALLBACK);
  });
});
