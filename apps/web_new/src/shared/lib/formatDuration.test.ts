// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { formatMinutes } from "./formatDuration.js";

describe("formatMinutes", () => {
  it("reads sub-hour counts as minutes", () => {
    expect(formatMinutes(0)).toBe("0m");
    expect(formatMinutes(1)).toBe("1m");
    expect(formatMinutes(45)).toBe("45m");
    expect(formatMinutes(59)).toBe("59m");
  });

  it("reads exact hours without a minutes tail", () => {
    expect(formatMinutes(60)).toBe("1h");
    expect(formatMinutes(120)).toBe("2h");
  });

  it("reads hours plus the leftover minutes", () => {
    expect(formatMinutes(90)).toBe("1h 30m");
    expect(formatMinutes(61)).toBe("1h 1m");
    expect(formatMinutes(150)).toBe("2h 30m");
  });

  it("floors fractions and clamps negatives to zero", () => {
    expect(formatMinutes(90.9)).toBe("1h 30m");
    expect(formatMinutes(-5)).toBe("0m");
  });
});
