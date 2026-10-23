// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { CYCLE_TITLE_MAX_LENGTH, validateCycleTitle } from "./cycleTitle.js";

describe("validateCycleTitle", () => {
  it("requires a title", () => {
    expect(validateCycleTitle("")).toBe("Title is required");
    expect(validateCycleTitle("   ")).toBe("Title is required");
  });

  it("caps the title at 255 characters", () => {
    expect(CYCLE_TITLE_MAX_LENGTH).toBe(255);
    expect(validateCycleTitle("x".repeat(255))).toBeNull();
    expect(validateCycleTitle("x".repeat(256))).toBe("Title must be at most 255 characters");
  });

  it("accepts a plain title", () => {
    expect(validateCycleTitle("Cycle 7")).toBeNull();
  });
});
