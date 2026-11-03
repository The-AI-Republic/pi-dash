// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  customModeForValue,
  INTAKE_CUSTOM_MODES,
  INTAKE_CUSTOM_RANGE_LABEL,
  INTAKE_DATE_PRESETS,
  intakeDateChipLabel,
  intakeDateToken,
  isIntakeRangeComplete,
  presetDayRange,
  sameDayNextMonth,
  shiftIsoDay,
  todayUtcIsoDay,
  DEFAULT_INTAKE_CUSTOM_MODE,
} from "./dates.js";

const TODAY = "2026-10-23";

describe("intake date values (INT-010)", () => {
  it("reads the UTC day, not the local day", () => {
    // 01:30 UTC is still the previous day behind UTC.
    expect(todayUtcIsoDay(new Date("2026-10-23T01:30:00Z"))).toBe("2026-10-23");
  });

  it("shifts days across month boundaries", () => {
    expect(shiftIsoDay("2026-10-23", -1)).toBe("2026-10-22");
    expect(shiftIsoDay("2026-10-01", -1)).toBe("2026-09-30");
    expect(shiftIsoDay("2026-10-23", 10)).toBe("2026-11-02");
  });

  it("seeds the range dialog on the same day of next month", () => {
    expect(sameDayNextMonth("2026-10-23")).toBe("2026-11-23");
    expect(sameDayNextMonth("2026-12-15")).toBe("2027-01-15");
  });

  it("offers the four presets in section order", () => {
    expect(INTAKE_DATE_PRESETS.map((preset) => preset.label)).toEqual([
      "Today",
      "Yesterday",
      "Last 7 days",
      "Last 30 days",
    ]);
    expect(INTAKE_CUSTOM_RANGE_LABEL).toBe("Custom");
    expect(DEFAULT_INTAKE_CUSTOM_MODE).toBe("range");
    expect(INTAKE_CUSTOM_MODES.map((entry) => entry.mode)).toEqual(["before", "after", "range"]);
  });

  it("resolves presets to inclusive UTC day bounds", () => {
    expect(presetDayRange("today", TODAY)).toEqual({ from: TODAY, to: TODAY });
    expect(presetDayRange("yesterday", TODAY)).toEqual({ from: "2026-10-22", to: "2026-10-22" });
    expect(presetDayRange("last7", TODAY)).toEqual({ from: "2026-10-16", to: TODAY });
    expect(presetDayRange("last30", TODAY)).toEqual({ from: "2026-09-23", to: TODAY });
  });

  it("falls back to today for an unknown preset", () => {
    expect(presetDayRange("someday" as never, TODAY)).toEqual({ from: TODAY, to: TODAY });
  });

  it("encodes presets as one FROM;after,TO;before pair", () => {
    expect(intakeDateToken({ kind: "preset", preset: "today" }, TODAY)).toBe(`${TODAY};after,${TODAY};before`);
    expect(intakeDateToken({ kind: "preset", preset: "yesterday" }, TODAY)).toBe("2026-10-22;after,2026-10-22;before");
    expect(intakeDateToken({ kind: "preset", preset: "last7" }, TODAY)).toBe(`2026-10-16;after,${TODAY};before`);
  });

  it("encodes explicit ranges one-sided or paired", () => {
    expect(intakeDateToken({ kind: "custom-before", day: "2026-10-20" }, TODAY)).toBe("2026-10-20;before");
    expect(intakeDateToken({ kind: "custom-after", day: "2026-10-20" }, TODAY)).toBe("2026-10-20;after");
    expect(intakeDateToken({ kind: "custom-range", from: "2026-10-15", to: "2026-10-20" }, TODAY)).toBe(
      "2026-10-15;after,2026-10-20;before"
    );
  });

  it("chips presets by label and explicit bounds by day", () => {
    expect(intakeDateChipLabel({ kind: "preset", preset: "last30" })).toBe("Last 30 days");
    expect(intakeDateChipLabel({ kind: "preset", preset: "someday" as never })).toBe("Today");
    expect(intakeDateChipLabel({ kind: "custom-before", day: "2026-10-20" })).toBe("Before Oct 20, 2026");
    expect(intakeDateChipLabel({ kind: "custom-after", day: "2026-10-20" })).toBe("After Oct 20, 2026");
    expect(intakeDateChipLabel({ kind: "custom-range", from: "2026-10-15", to: "2026-10-20" })).toBe(
      "Oct 15 - 20, 2026"
    );
  });

  it("applies a range only when both ends are picked in order", () => {
    expect(isIntakeRangeComplete("2026-10-15", "2026-10-20")).toBe(true);
    expect(isIntakeRangeComplete("2026-10-20", "2026-10-20")).toBe(true);
    expect(isIntakeRangeComplete("2026-10-20", "2026-10-15")).toBe(false);
    expect(isIntakeRangeComplete("2026-10-15", null)).toBe(false);
    expect(isIntakeRangeComplete(null, "2026-10-20")).toBe(false);
    expect(isIntakeRangeComplete(null, null)).toBe(false);
  });

  it("reopens a stored custom value in its own shape", () => {
    expect(customModeForValue({ kind: "preset", preset: "today" })).toBe(null);
    expect(customModeForValue({ kind: "custom-before", day: TODAY })).toBe("before");
    expect(customModeForValue({ kind: "custom-after", day: TODAY })).toBe("after");
    expect(customModeForValue({ kind: "custom-range", from: TODAY, to: TODAY })).toBe("range");
  });
});
