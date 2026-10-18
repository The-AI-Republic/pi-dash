// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DELAY_PRESETS,
  delayLabel,
  isDelayEnabled,
  MAX_DELAY_MONTHS,
  MIN_DELAY_MONTHS,
  parseDelayInput,
  rejectsFractionOnly,
} from "./months.js";

describe("delayLabel", () => {
  it("uses the singular for one month", () => {
    expect(delayLabel(1)).toBe("1 month");
  });

  it("uses the plural for every other count", () => {
    expect(delayLabel(3)).toBe("3 months");
    expect(delayLabel(12)).toBe("12 months");
  });
});

describe("isDelayEnabled", () => {
  it("treats zero as off and positive counts as on", () => {
    expect(isDelayEnabled(0)).toBe(false);
    expect(isDelayEnabled(1)).toBe(true);
  });
});

describe("DELAY_PRESETS", () => {
  it("offers the five fixed delays in ascending order", () => {
    expect([...DELAY_PRESETS]).toEqual([1, 3, 6, 9, 12]);
  });

  it("bounds the custom range inside the presets", () => {
    expect(MIN_DELAY_MONTHS).toBe(1);
    expect(MAX_DELAY_MONTHS).toBe(12);
  });
});

describe("parseDelayInput", () => {
  it("accepts whole months in range", () => {
    expect(parseDelayInput("1")).toBe(1);
    expect(parseDelayInput("5")).toBe(5);
    expect(parseDelayInput("12")).toBe(12);
    expect(parseDelayInput(" 7 ")).toBe(7);
  });

  it("refuses empty input", () => {
    expect(parseDelayInput("")).toBeNull();
    expect(parseDelayInput("   ")).toBeNull();
  });

  it("refuses zero and values above the range", () => {
    expect(parseDelayInput("0")).toBeNull();
    expect(parseDelayInput("13")).toBeNull();
  });

  it("refuses fractional and signed values", () => {
    expect(parseDelayInput("2.5")).toBeNull();
    expect(parseDelayInput("-3")).toBeNull();
    expect(parseDelayInput("abc")).toBeNull();
  });
});

describe("rejectsFractionOnly", () => {
  it("flags only numeric values with a fractional part", () => {
    expect(rejectsFractionOnly("2.5")).toBe(true);
    expect(rejectsFractionOnly(" 0.25 ")).toBe(true);
    expect(rejectsFractionOnly("1.0")).toBe(false);
    expect(rejectsFractionOnly("0")).toBe(false);
    expect(rejectsFractionOnly("13")).toBe(false);
    expect(rejectsFractionOnly("")).toBe(false);
    expect(rejectsFractionOnly("abc")).toBe(false);
    expect(rejectsFractionOnly("-3")).toBe(false);
  });
});
