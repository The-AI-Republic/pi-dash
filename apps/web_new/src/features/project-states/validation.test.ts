// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { capStateName, STATE_NAME_MAX_LENGTH, STATE_NAME_REQUIRED, validateStateName } from "./validation.js";

describe("capStateName", () => {
  it("accepts names at the cap and trims anything past it", () => {
    expect(STATE_NAME_MAX_LENGTH).toBe(100);
    expect(capStateName("x".repeat(100))).toHaveLength(100);
    expect(capStateName("x".repeat(101))).toHaveLength(100);
    expect(capStateName("short")).toBe("short");
  });
});

describe("validateStateName", () => {
  it("refuses blank and whitespace-only names", () => {
    expect(validateStateName("")).toBe(STATE_NAME_REQUIRED);
    expect(validateStateName("   ")).toBe(STATE_NAME_REQUIRED);
  });

  it("accepts a name with content", () => {
    expect(validateStateName("In review")).toBeNull();
    expect(validateStateName(" x ")).toBeNull();
  });
});
