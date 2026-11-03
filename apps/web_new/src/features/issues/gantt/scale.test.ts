// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  DAY_WIDTHS,
  EXTENSION_DAYS,
  GANTT_ZOOMS,
  GANTT_ZOOM_LABELS,
  centeredScroll,
  addDays,
  dayColumns,
  dayOffset,
  daySegments,
  diffDays,
  edgeAt,
  extendRange,
  growRangeToCover,
  headerTiers,
  initialRange,
  isGanttZoom,
  monthSegments,
  quarterSegments,
  segmentAt,
  startOfWeek,
  timelineWidth,
  todaySegment,
  weekSegments,
  windowTodaySegment,
  weekdayOf,
  EDGE_TRIGGER_PX,
} from "./scale.js";

describe("day arithmetic", () => {
  it("shifts across months, years and leap days", () => {
    expect(addDays("2026-01-31", 1)).toBe("2026-02-01");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2024-02-28", 1)).toBe("2024-02-29");
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
  });

  it("counts signed whole days between dates", () => {
    expect(diffDays("2026-03-10", "2026-03-01")).toBe(9);
    expect(diffDays("2026-03-01", "2026-03-10")).toBe(-9);
    expect(diffDays("2026-03-01", "2026-03-01")).toBe(0);
  });

  it("reads weekdays and snaps to a profile week start", () => {
    expect(weekdayOf("2026-10-18")).toBe(0);
    expect(startOfWeek("2026-10-18", 0)).toBe("2026-10-18");
    expect(startOfWeek("2026-10-18", 1)).toBe("2026-10-12");
    expect(startOfWeek("2026-10-18", 6)).toBe("2026-10-17");
  });

  it("rejects malformed days", () => {
    expect(() => addDays("2026-02-30", 1)).toThrow(RangeError);
  });
});

describe("zoom windows", () => {
  it("offers exactly three zooms in switcher order with labels", () => {
    expect(GANTT_ZOOMS).toEqual(["week", "month", "quarter"]);
    expect(GANTT_ZOOM_LABELS).toEqual({ week: "Week", month: "Month", quarter: "Quarter" });
    expect(isGanttZoom("month")).toBe(true);
    expect(isGanttZoom("day")).toBe(false);
    expect(isGanttZoom(null)).toBe(false);
  });

  it("widens days strictly from week to quarter", () => {
    expect(DAY_WIDTHS.week).toBeGreaterThan(DAY_WIDTHS.month);
    expect(DAY_WIDTHS.month).toBeGreaterThan(DAY_WIDTHS.quarter);
  });

  it("opens on whole weeks with today inside the window", () => {
    for (const zoom of GANTT_ZOOMS) {
      const range = initialRange(zoom, "2026-10-18", 0);
      expect(range.dayCount % 7).toBe(0);
      expect(weekdayOf(range.startIso)).toBe(0);
      const offset = diffDays("2026-10-18", range.startIso);
      expect(offset).toBeGreaterThan(0);
      expect(offset).toBeLessThan(range.dayCount);
    }
  });

  it("extends a window on either side by the zoom's step", () => {
    const range = { startIso: "2026-10-01", dayCount: 28 };
    expect(extendRange(range, "right", "week")).toEqual({ startIso: "2026-10-01", dayCount: 42 });
    expect(extendRange(range, "left", "week")).toEqual({ startIso: "2026-09-17", dayCount: 42 });
    expect(extendRange(range, "right", "quarter").dayCount).toBe(28 + EXTENSION_DAYS.quarter);
  });

  it("leaves a window that already covers the span alone", () => {
    const range = { startIso: "2026-10-01", dayCount: 28 };
    expect(growRangeToCover(range, "week", "2026-10-05", "2026-10-10")).toBe(range);
  });

  it("grows the short side in whole steps until the span fits", () => {
    const range = { startIso: "2026-10-01", dayCount: 28 };
    // Window ends 2026-10-28; the span needs two 14-day steps right.
    expect(growRangeToCover(range, "week", "2026-10-05", "2026-11-20")).toEqual({
      startIso: "2026-10-01",
      dayCount: 56,
    });
    // Reversed ends still grow the left side, overshooting in whole steps.
    expect(growRangeToCover(range, "week", "2026-10-10", "2026-09-01")).toEqual({
      startIso: "2026-08-20",
      dayCount: 70,
    });
  });

  it("grows both sides when the span straddles the window", () => {
    const range = { startIso: "2026-10-01", dayCount: 28 };
    expect(growRangeToCover(range, "week", "2026-09-20", "2026-11-20")).toEqual({
      startIso: "2026-09-17",
      dayCount: 70,
    });
  });

  it("converts days to pixel offsets and full width", () => {
    const range = { startIso: "2026-10-01", dayCount: 10 };
    expect(dayOffset("2026-10-04", range, "week")).toBe(3 * DAY_WIDTHS.week);
    expect(dayOffset("2026-09-30", range, "month")).toBe(-DAY_WIDTHS.month);
    expect(timelineWidth(range, "month")).toBe(10 * DAY_WIDTHS.month);
  });
});

describe("day columns", () => {
  it("marks weekends and week starts for the profile's first day", () => {
    const days = dayColumns({ startIso: "2026-10-12", dayCount: 7 }, 1);
    expect(days.map((day) => day.iso)).toHaveLength(7);
    expect(days[0]?.startsWeek).toBe(true);
    expect(days.filter((day) => day.weekend).map((day) => day.weekday)).toEqual([6, 0]);
    expect(days[0]?.weekend).toBe(false);
  });
});

describe("header tiers", () => {
  it("labels weeks, months and quarters", () => {
    const range = { startIso: "2026-09-28", dayCount: 34 };
    expect(monthSegments(range).map((segment) => segment.label)).toEqual(["Sep", "Oct"]);
    const quarters = quarterSegments({ startIso: "2026-10-01", dayCount: 120 });
    expect(quarters.map((segment) => segment.label)).toEqual(["Q4 2026", "Q1 2027"]);
    expect(quarters.map((segment) => segment.days)).toEqual([92, 28]);
  });

  it("cuts calendar spans at the window's edges", () => {
    const months = monthSegments({ startIso: "2026-10-20", dayCount: 20 });
    expect(months.map((segment) => segment.days)).toEqual([12, 8]);
    const quarter = quarterSegments({ startIso: "2026-12-20", dayCount: 20 });
    expect(quarter.map((segment) => segment.days)).toEqual([12, 8]);
  });

  it("draws weeks as consecutive 7-day spans from the window start", () => {
    const weeks = weekSegments({ startIso: "2026-10-14", dayCount: 21 });
    expect(weeks.map((segment) => segment.days)).toEqual([4, 7, 7, 3]);
    expect(weeks[0]?.label).toBe("14 - 17");
  });

  it("returns two tiers per zoom", () => {
    const range = { startIso: "2026-10-01", dayCount: 28 };
    expect(Object.keys(headerTiers("week", range))).toEqual(["coarse", "fine"]);
    expect(headerTiers("week", range).fine).toHaveLength(28);
    expect(headerTiers("month", range).coarse[0]?.label).toBe("Oct");
    expect(headerTiers("quarter", range).fine[0]?.label).toBe("Oct");
  });

  it("finds the segment containing a day", () => {
    const segments = daySegments({ startIso: "2026-10-01", dayCount: 3 });
    expect(segmentAt(segments, 1)?.label).toBe("Fr 2");
    expect(segmentAt(segments, 3)).toBeNull();
  });
});

describe("today", () => {
  it("marks the day at Week, the current week at Month, the month at Quarter", () => {
    const range = { startIso: "2026-10-01", dayCount: 31 };
    expect(todaySegment("week", range, "2026-10-18")?.key).toBe("d-2026-10-18");
    const month = todaySegment("month", range, "2026-10-18");
    expect(month?.startIndex).toBeGreaterThanOrEqual(0);
    expect(month?.days).toBeGreaterThan(0);
    expect(todaySegment("quarter", range, "2026-10-18")?.label).toBe("Oct");
  });

  it("keeps the today week marked on the week's last day (intended, NEWFRONT-163)", () => {
    const range = { startIso: "2026-10-12", dayCount: 14 };
    // 2026-10-17 is a Saturday, the Sunday-start week's last day.
    const marked = todaySegment("month", range, "2026-10-17");
    expect(marked?.startIndex).toBe(0);
    expect(marked?.days).toBe(6);
  });

  it("marks the first day of the range and nothing outside it", () => {
    const range = { startIso: "2026-10-01", dayCount: 10 };
    expect(todaySegment("week", range, "2026-10-01")?.key).toBe("d-2026-10-01");
    expect(todaySegment("week", range, "2026-09-30")).toBeNull();
    expect(todaySegment("week", range, "2026-10-11")).toBeNull();
  });
});

describe("scroll helpers", () => {
  const range = { startIso: "2026-10-01", dayCount: 56 };

  it("centers today right of the middle, never past the strip", () => {
    const offset = centeredScroll("2026-10-28", range, "week", 1200);
    expect(offset).toBeGreaterThan(0);
    const total = timelineWidth(range, "week");
    expect(offset).toBeLessThanOrEqual(total - 1200);
  });

  it("clamps at zero for early days and never exceeds the strip", () => {
    expect(centeredScroll("2026-10-01", range, "week", 2000)).toBe(0);
    expect(centeredScroll("2026-10-30", range, "quarter", 100)).toBe(28.5);
  });

  it("reports the edge a scroll offset reached", () => {
    expect(edgeAt(0, 2000, 800)).toBe("left");
    expect(edgeAt(1200, 2000, 800)).toBe("right");
    expect(edgeAt(EDGE_TRIGGER_PX + 100, 5000, 800)).toBeNull();
  });
});

describe("edge coverage of the calendar walk", () => {
  it("merges the tail of a wrapped span into the previous month cell", () => {
    // A two-day window on New Year's Eve: the December span ends at the
    // window edge, and the single January day must be its own cell.
    const segments = monthSegments({ startIso: "2026-12-30", dayCount: 3 });
    expect(segments.map((segment) => [segment.key, segment.days])).toEqual([
      ["m-2026-11", 2],
      ["m-2027-0", 1],
    ]);
  });

  it("marks the same quarter across a month boundary as one cell", () => {
    const segments = quarterSegments({ startIso: "2026-10-25", dayCount: 10 });
    expect(segments).toHaveLength(1);
    expect(segments[0]).toMatchObject({ key: "q-2026-3", days: 10 });
  });

  it("falls back to the window's first segment when today is outside it", () => {
    const range = { startIso: addDays("2026-10-18", 400), dayCount: 7 };
    expect(windowTodaySegment("week", range, "2026-10-18")).toEqual(daySegments(range)[0]);
    expect(windowTodaySegment("month", range, "2026-10-18").key).toMatch(/^w-/);
  });

  it("returns the segment holding today inside the window", () => {
    const range = { startIso: "2026-10-12", dayCount: 28 };
    expect(windowTodaySegment("week", range, "2026-10-18").key).toBe("d-2026-10-18");
    expect(windowTodaySegment("month", range, "2026-10-18").key).toBe("w-2026-10-18");
    expect(windowTodaySegment("quarter", range, "2026-10-18").key).toBe("m-2026-9");
  });

  it("returns no today segment once today sits outside a moved window", () => {
    const range = { startIso: addDays("2026-10-18", 400), dayCount: 7 };
    expect(todaySegment("week", range, "2026-10-18")).toBeNull();
    expect(todaySegment("month", range, "2026-10-18")).toBeNull();
    expect(todaySegment("quarter", range, "2026-10-18")).toBeNull();
  });
});
