// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DATE_PRESETS,
  clearCustomDate,
  customRange,
  customEndEntry,
  customStartEntry,
  cycleLifecycle,
  dateValueLabel,
  dateValueParts,
  hasCustomDate,
  isCustomDateEntry,
  isDatePreset,
  toggleDatePreset,
} from "./legacyDimensions.js";

describe("cycleLifecycle", () => {
  it.each([
    ["DRAFT", "draft"],
    ["UPCOMING", "upcoming"],
    ["CURRENT", "current"],
    ["ACTIVE", "current"],
    ["active", "current"],
    ["COMPLETED", "completed"],
    [" completed ", "completed"],
    ["SOMETHING_ELSE", "draft"],
    [undefined, "draft"],
    ["", "draft"],
  ] as const)("reads %s as %s", (status, lifecycle) => {
    expect(cycleLifecycle(status)).toBe(lifecycle);
  });
});

describe("date presets", () => {
  it("offers the four forward windows in server token form", () => {
    expect(DATE_PRESETS.map((preset) => preset.value)).toEqual([
      "1_weeks;after;fromnow",
      "2_weeks;after;fromnow",
      "1_months;after;fromnow",
      "2_months;after;fromnow",
    ]);
    expect(DATE_PRESETS.find((preset) => preset.value === "2_weeks;after;fromnow")?.label).toBe("2 weeks from now");
  });

  it("tells presets from custom entries", () => {
    expect(isDatePreset("1_weeks;after;fromnow")).toBe(true);
    expect(isCustomDateEntry("1_weeks;after;fromnow")).toBe(false);
    expect(isDatePreset("2026-01-01;after")).toBe(false);
    expect(isCustomDateEntry("2026-01-01;after")).toBe(true);
    expect(isCustomDateEntry("nonsense")).toBe(false);
    expect(customStartEntry("2026-01-01")).toBe("2026-01-01;after");
    expect(customEndEntry("2026-01-31")).toBe("2026-01-31;before");
  });

  it("reads entries from every stored shape", () => {
    expect(dateValueParts(null)).toEqual({ presets: [], custom: [] });
    expect(dateValueParts("2026-01-01;after")).toEqual({ presets: [], custom: ["2026-01-01;after"] });
    expect(dateValueParts(["1_weeks;after;fromnow", "2026-01-01;after"])).toEqual({
      presets: ["1_weeks;after;fromnow"],
      custom: ["2026-01-01;after"],
    });
    expect(dateValueParts("2026-01-01;after,2026-01-31;before")).toEqual({
      presets: [],
      custom: ["2026-01-01;after", "2026-01-31;before"],
    });
    expect(dateValueParts(["", 42])).toEqual({ presets: [], custom: [] });
    expect(dateValueParts(42)).toEqual({ presets: [], custom: [] });
  });

  it("toggles presets off and on while keeping custom entries", () => {
    const custom = "2026-01-01;after";
    const once = toggleDatePreset(custom, "2_weeks;after;fromnow");
    expect(once).toEqual([custom, "2_weeks;after;fromnow"]);
    expect(toggleDatePreset(once, "1_months;after;fromnow")).toEqual([
      custom,
      "2_weeks;after;fromnow",
      "1_months;after;fromnow",
    ]);
    expect(toggleDatePreset(once, "2_weeks;after;fromnow")).toBe(custom);
  });

  it("reads a lone pick as one value and clears the last one", () => {
    expect(toggleDatePreset(null, "1_weeks;after;fromnow")).toBe("1_weeks;after;fromnow");
    expect(toggleDatePreset("1_weeks;after;fromnow", "1_weeks;after;fromnow")).toBeNull();
  });

  it("clears custom entries without touching presets", () => {
    const preset = "1_weeks;after;fromnow";
    expect(clearCustomDate([preset, "2026-01-01;after"])).toBe(preset);
    expect(clearCustomDate("2026-01-01;after")).toBeNull();
  });

  it("reads the custom ends back for the calendar", () => {
    expect(customRange(null)).toEqual({ start: null, end: null });
    expect(customRange(["2026-01-01;after", "2026-01-31;before"])).toEqual({ start: "2026-01-01", end: "2026-01-31" });
    expect(customRange(["2026-01-31;before"])).toEqual({ start: null, end: "2026-01-31" });
    expect(customRange("garbage;after")).toEqual({ start: null, end: null });
  });

  it("detects a custom half", () => {
    expect(hasCustomDate("1_weeks;after;fromnow")).toBe(false);
    expect(hasCustomDate("2026-01-01;after")).toBe(true);
    expect(hasCustomDate(["1_weeks;after;fromnow", "2026-01-01;after"])).toBe(true);
  });

  it("labels presets by name and the custom half as a range", () => {
    expect(dateValueLabel(null)).toBeNull();
    expect(dateValueLabel("1_weeks;after;fromnow")).toBe("1 week from now");
    expect(dateValueLabel("2026-01-01;after")).toBe("2026-01-01");
    expect(dateValueLabel("2026-01-31;before")).toBe("2026-01-31");
    expect(dateValueLabel(["1_weeks;after;fromnow", "2026-01-01;after", "2026-01-31;before"])).toBe(
      "1 week from now, 2026-01-01 → 2026-01-31"
    );
    expect(dateValueLabel(["unknown_token"])).toBe("unknown_token");
  });
});
