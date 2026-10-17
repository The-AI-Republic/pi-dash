// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  applyRangeClick,
  CAPTION_YEAR_MAX,
  CAPTION_YEAR_MIN,
  DEFAULT_WEEK_START,
  formatDay,
  formatDayLong,
  formatMergedRange,
  isDayInRange,
  isIsoDay,
  MONTH_ABBREVS,
  MONTH_NAMES,
  monthGrid,
  normalizeWeekStart,
  orderedWeekdays,
  parseIsoDay,
  shiftMonth,
  todayIsoDay,
  toIsoDay,
  WEEKDAY_NAMES,
  WEEKDAY_SHORT,
} from "./calendar.js";

describe("calendar days", () => {
  it("accepts real days and rejects malformed or impossible ones", () => {
    expect(isIsoDay("2026-10-15")).toBe(true);
    expect(isIsoDay("2024-02-29")).toBe(true);
    expect(isIsoDay(null)).toBe(false);
    expect(isIsoDay(20261015)).toBe(false);
    expect(isIsoDay("15.10.2026")).toBe(false);
    expect(isIsoDay("2026-13-01")).toBe(false);
    expect(isIsoDay("2026-00-10")).toBe(false);
    expect(isIsoDay("2026-10-32")).toBe(false);
    expect(isIsoDay("2026-10-00")).toBe(false);
    expect(isIsoDay("2026-02-30")).toBe(false);
    expect(isIsoDay("2025-02-29")).toBe(false);
  });

  it("formats triples with padded month and day", () => {
    expect(toIsoDay(2026, 0, 5)).toBe("2026-01-05");
    expect(toIsoDay(2026, 11, 25)).toBe("2026-12-25");
  });

  it("parses days into parts and rejects the rest", () => {
    expect(parseIsoDay("2026-10-15")).toEqual({ year: 2026, month0: 9, day: 15 });
    expect(parseIsoDay(null)).toBe(null);
    expect(parseIsoDay(undefined)).toBe(null);
    expect(parseIsoDay("not-a-day")).toBe(null);
  });

  it("reads today in local time", () => {
    expect(todayIsoDay(new Date(2026, 9, 15))).toBe("2026-10-15");
    expect(isIsoDay(todayIsoDay())).toBe(true);
  });
});

describe("week start", () => {
  it("defaults to Sunday", () => {
    expect(DEFAULT_WEEK_START).toBe(0);
    expect(WEEKDAY_NAMES).toHaveLength(7);
    expect(WEEKDAY_SHORT).toHaveLength(7);
    expect(MONTH_NAMES).toHaveLength(12);
    expect(MONTH_ABBREVS).toHaveLength(12);
  });

  it("clamps unknown values onto the Sunday-first range", () => {
    expect(normalizeWeekStart(1)).toBe(1);
    expect(normalizeWeekStart(6)).toBe(6);
    expect(normalizeWeekStart(2.9)).toBe(2);
    expect(normalizeWeekStart(undefined)).toBe(0);
    expect(normalizeWeekStart("1")).toBe(0);
    expect(normalizeWeekStart(Number.NaN)).toBe(0);
    expect(normalizeWeekStart(-1)).toBe(0);
    expect(normalizeWeekStart(7)).toBe(0);
  });

  it("rotates the header row onto the week start", () => {
    expect(orderedWeekdays(0).map((day) => day.short)).toEqual(["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]);
    expect(orderedWeekdays(1).map((day) => day.short)).toEqual(["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]);
    expect(orderedWeekdays(3)[0]).toEqual({ full: "Wednesday", short: "We" });
    expect(orderedWeekdays(9).map((day) => day.short)[0]).toBe("Su");
  });
});

describe("month grid", () => {
  it("covers six full weeks with adjacent-month filler", () => {
    // October 2026 opens on a Thursday, so a Sunday start leads with Sep 27.
    const grid = monthGrid(2026, 9, 0, null, null, "2026-10-15");
    expect(grid).toHaveLength(6);
    for (const week of grid) expect(week).toHaveLength(7);
    expect(grid[0]?.[0]).toMatchObject({ iso: "2026-09-27", day: 27, inMonth: false });
    expect(grid[0]?.[4]).toMatchObject({ iso: "2026-10-01", day: 1, inMonth: true });
    expect(grid[5]?.[6]).toMatchObject({ iso: "2026-11-07", inMonth: false });
    const inMonth = grid.flat().filter((cell) => cell.inMonth);
    expect(inMonth).toHaveLength(31);
  });

  it("starts the grid on the week start without filler when aligned", () => {
    // October 2026 opens on a Thursday: a Thursday start needs no lead.
    const grid = monthGrid(2026, 9, 4, null, null, "2026-10-15");
    expect(grid[0]?.[0]).toMatchObject({ iso: "2026-10-01", inMonth: true });
    expect(grid[0]?.map((cell) => cell.iso)).toEqual([
      "2026-10-01",
      "2026-10-02",
      "2026-10-03",
      "2026-10-04",
      "2026-10-05",
      "2026-10-06",
      "2026-10-07",
    ]);
  });

  it("disables days outside min/max and marks today", () => {
    const grid = monthGrid(2026, 9, 0, "2026-10-10", "2026-10-20", "2026-10-15");
    const byDay = new Map(grid.flat().map((cell) => [cell.iso, cell]));
    expect(byDay.get("2026-10-09")?.disabled).toBe(true);
    expect(byDay.get("2026-10-10")?.disabled).toBe(false);
    expect(byDay.get("2026-10-20")?.disabled).toBe(false);
    expect(byDay.get("2026-10-21")?.disabled).toBe(true);
    expect(byDay.get("2026-10-15")?.isToday).toBe(true);
    expect(byDay.get("2026-10-16")?.isToday).toBe(false);
  });

  it("treats empty bounds as unset", () => {
    const grid = monthGrid(2026, 9, 0, "", "", "2026-10-15");
    expect(grid.flat().every((cell) => !cell.disabled)).toBe(true);
  });

  it("disables one side when only one bound is set", () => {
    const fromMin = monthGrid(2026, 9, 0, "2026-10-10", null, "2026-10-15");
    const minDays = new Map(fromMin.flat().map((cell) => [cell.iso, cell]));
    expect(minDays.get("2026-10-09")?.disabled).toBe(true);
    expect(minDays.get("2026-10-31")?.disabled).toBe(false);
    const toMax = monthGrid(2026, 9, 0, null, "2026-10-20", "2026-10-15");
    const maxDays = new Map(toMax.flat().map((cell) => [cell.iso, cell]));
    expect(maxDays.get("2026-10-21")?.disabled).toBe(true);
    expect(maxDays.get("2026-10-01")?.disabled).toBe(false);
  });
});

describe("month shifting", () => {
  it("moves within the year", () => {
    expect(shiftMonth(2026, 9, 1)).toEqual({ year: 2026, month0: 10 });
    expect(shiftMonth(2026, 9, -1)).toEqual({ year: 2026, month0: 8 });
  });

  it("carries across year boundaries", () => {
    expect(shiftMonth(2026, 11, 1)).toEqual({ year: 2027, month0: 0 });
    expect(shiftMonth(2026, 0, -1)).toEqual({ year: 2025, month0: 11 });
    expect(shiftMonth(2026, 0, -13)).toEqual({ year: 2024, month0: 11 });
  });
});

describe("caption years", () => {
  it("bounds the year select", () => {
    expect(CAPTION_YEAR_MIN).toBe(1900);
    expect(CAPTION_YEAR_MAX).toBe(2100);
    expect(CAPTION_YEAR_MAX).toBeGreaterThan(CAPTION_YEAR_MIN);
  });
});

describe("day formatting", () => {
  it("renders short and long labels", () => {
    expect(formatDay("2026-10-05")).toBe("Oct 5, 2026");
    expect(formatDayLong("2026-01-09")).toBe("January 9, 2026");
  });

  it("passes invalid input through", () => {
    expect(formatDay("nope")).toBe("nope");
    expect(formatDayLong("nope")).toBe("nope");
  });
});

describe("merged range label", () => {
  it("names the month once when both ends share it", () => {
    expect(formatMergedRange("2026-10-10", "2026-10-20")).toBe("Oct 10 - 20, 2026");
  });

  it("names both months across a month boundary", () => {
    expect(formatMergedRange("2026-10-28", "2026-11-03")).toBe("Oct 28 - Nov 3, 2026");
  });

  it("renders both dates fully across a year boundary", () => {
    expect(formatMergedRange("2026-12-30", "2027-01-02")).toBe("Dec 30, 2026 - Jan 2, 2027");
  });

  it("falls back to joined labels for invalid ends", () => {
    expect(formatMergedRange("nope", "2026-10-20")).toBe("nope - Oct 20, 2026");
    expect(formatMergedRange("2026-10-10", "nope")).toBe("Oct 10, 2026 - nope");
  });
});

describe("range clicks", () => {
  it("opens a pending range on the first pick", () => {
    expect(applyRangeClick({ start: null, end: null }, "2026-10-10")).toEqual({ start: "2026-10-10", end: null });
    expect(applyRangeClick({ start: "", end: null }, "2026-10-10")).toEqual({ start: "2026-10-10", end: null });
  });

  it("completes a pending range with a later pick", () => {
    expect(applyRangeClick({ start: "2026-10-10", end: null }, "2026-10-12")).toEqual({
      start: "2026-10-10",
      end: "2026-10-12",
    });
    expect(applyRangeClick({ start: "2026-10-10", end: "" }, "2026-10-12")).toEqual({
      start: "2026-10-10",
      end: "2026-10-12",
    });
  });

  it("restarts a pending range with an earlier pick", () => {
    expect(applyRangeClick({ start: "2026-10-10", end: null }, "2026-10-05")).toEqual({
      start: "2026-10-05",
      end: null,
    });
  });

  it("resets on a same-day double pick", () => {
    expect(applyRangeClick({ start: "2026-10-10", end: null }, "2026-10-10")).toEqual({
      start: null,
      end: null,
    });
  });

  it("moves the start when clicking on or before it", () => {
    expect(applyRangeClick({ start: "2026-10-10", end: "2026-10-20" }, "2026-10-05")).toEqual({
      start: "2026-10-05",
      end: "2026-10-20",
    });
    expect(applyRangeClick({ start: "2026-10-10", end: "2026-10-20" }, "2026-10-10")).toEqual({
      start: "2026-10-10",
      end: "2026-10-20",
    });
  });

  it("moves the end when clicking inside or after the range", () => {
    expect(applyRangeClick({ start: "2026-10-10", end: "2026-10-20" }, "2026-10-15")).toEqual({
      start: "2026-10-10",
      end: "2026-10-15",
    });
    expect(applyRangeClick({ start: "2026-10-10", end: "2026-10-20" }, "2026-10-25")).toEqual({
      start: "2026-10-10",
      end: "2026-10-25",
    });
  });
});

describe("range membership", () => {
  it("matches strictly interior days of a complete range", () => {
    const range = { start: "2026-10-10", end: "2026-10-20" };
    expect(isDayInRange("2026-10-15", range)).toBe(true);
    expect(isDayInRange("2026-10-10", range)).toBe(false);
    expect(isDayInRange("2026-10-20", range)).toBe(false);
    expect(isDayInRange("2026-10-25", range)).toBe(false);
  });

  it("matches nothing while the range is incomplete", () => {
    expect(isDayInRange("2026-10-15", { start: "2026-10-10", end: null })).toBe(false);
    expect(isDayInRange("2026-10-15", { start: null, end: "2026-10-20" })).toBe(false);
    expect(isDayInRange("2026-10-15", { start: "", end: "" })).toBe(false);
  });
});
