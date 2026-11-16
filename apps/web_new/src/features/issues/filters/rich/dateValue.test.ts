// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Date value grammar tests (parity/specs/issues.md §9, ISS-089–092). The
// editor and the list query both read stored values through this module
// (NEWFRONT-871), so it is exercised directly as well as by the editor.
import { describe, expect, it } from "vitest";

import {
  DATE_PRESET_VALUES,
  customDateEntries,
  customDateMode,
  customRange,
  dateEntries,
  dateValueParts,
  isCustomDateEntry,
  isDatePreset,
} from "./dateValue.js";

describe("date value grammar", () => {
  it("offers the four forward windows in server token form", () => {
    expect(DATE_PRESET_VALUES).toEqual([
      "1_weeks;after;fromnow",
      "2_weeks;after;fromnow",
      "1_months;after;fromnow",
      "2_months;after;fromnow",
    ]);
    expect(isDatePreset("1_weeks;after;fromnow")).toBe(true);
    expect(isDatePreset("3_weeks;after;fromnow")).toBe(false);
  });

  it("tells a directioned custom day from a preset and from junk", () => {
    expect(isCustomDateEntry("2026-01-01;after")).toBe(true);
    expect(isCustomDateEntry("2026-01-01;before")).toBe(true);
    expect(isCustomDateEntry("1_weeks;after;fromnow")).toBe(false);
    expect(isCustomDateEntry("nonsense")).toBe(false);
    expect(isCustomDateEntry("")).toBe(false);
  });

  it("reads entries from every stored shape and drops blanks", () => {
    expect(dateEntries(null)).toEqual([]);
    expect(dateEntries({})).toEqual([]);
    expect(dateEntries("2026-01-01;after")).toEqual(["2026-01-01;after"]);
    expect(dateEntries([" 2026-01-01;after ", "", "1_weeks;after;fromnow"])).toEqual([
      "2026-01-01;after",
      "1_weeks;after;fromnow",
    ]);
    expect(dateValueParts("2026-01-01;after,1_weeks;after;fromnow")).toEqual({
      presets: ["1_weeks;after;fromnow"],
      custom: ["2026-01-01;after"],
    });
  });

  it("names the shape of the custom half", () => {
    expect(customDateMode(null)).toBeNull();
    expect(customDateMode("1_weeks;after;fromnow")).toBeNull();
    expect(customDateMode("2026-01-01")).toBe("exact");
    expect(customDateMode("2026-01-01;after")).toBe("after");
    expect(customDateMode("2026-01-31;before")).toBe("before");
    expect(customDateMode(["2026-01-01", "2026-01-31"])).toBe("range");
    expect(customDateEntries(["2026-01-01", "1_weeks;after;fromnow"])).toEqual(["2026-01-01"]);
  });

  it("reads both custom ends, directions and bare days alike", () => {
    expect(customRange(null)).toEqual({ start: null, end: null });
    expect(customRange("1_weeks;after;fromnow")).toEqual({ start: null, end: null });
    expect(customRange("2026-01-01;after,2026-01-31;before")).toEqual({
      start: "2026-01-01",
      end: "2026-01-31",
    });
    // Bare days take their place by position: first lower, then upper.
    expect(customRange(["2026-01-01", "2026-01-31"])).toEqual({ start: "2026-01-01", end: "2026-01-31" });
    expect(customRange(["not-a-day", "2026-01-31"])).toEqual({ start: "2026-01-31", end: null });
  });
});
