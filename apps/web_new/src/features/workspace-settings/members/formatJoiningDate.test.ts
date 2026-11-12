// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Joining-date formatting helper (WSET-011).
import { describe, expect, it } from "vitest";

import { formatJoiningDate } from "./MemberRow.js";

describe("formatJoiningDate", () => {
  it("formats a parseable timestamp for display", () => {
    expect(formatJoiningDate("2026-01-15T12:00:00Z")).toMatch(/^Jan 1[45], 2026$/);
  });

  it("falls back to an empty string for an unparseable timestamp", () => {
    expect(formatJoiningDate("not-a-date")).toBe("");
  });
});
