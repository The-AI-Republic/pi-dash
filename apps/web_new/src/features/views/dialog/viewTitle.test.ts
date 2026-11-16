// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { VIEW_TITLE_MAX_LENGTH, validateViewTitle } from "./viewTitle.js";

describe("validateViewTitle", () => {
  it("requires a title", () => {
    expect(validateViewTitle("")).toBe("Title is required");
  });

  it("treats whitespace as missing", () => {
    expect(validateViewTitle("   ")).toBe("Title is required");
  });

  it("accepts a title at the cap", () => {
    expect(validateViewTitle("x".repeat(VIEW_TITLE_MAX_LENGTH))).toBeNull();
  });

  it("refuses a title past the cap and names it", () => {
    const error = validateViewTitle("x".repeat(VIEW_TITLE_MAX_LENGTH + 1));
    expect(error).toContain("255");
  });

  it("accepts an ordinary title", () => {
    expect(validateViewTitle("Triage")).toBeNull();
  });
});
