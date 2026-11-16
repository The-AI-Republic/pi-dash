// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  elapsedHalfHour,
  isoDayOffset,
  isWeekendISO,
  pickFutureWeekdayISO,
  pickPastWeekdayISO,
  zonedTodayISO,
} from "./calendar-dates.js";

describe("isoDayOffset", () => {
  it("shifts days within the month", () => {
    expect(isoDayOffset("2026-10-14", 0)).toBe("2026-10-14");
    expect(isoDayOffset("2026-10-14", 7)).toBe("2026-10-21");
    expect(isoDayOffset("2026-10-14", -1)).toBe("2026-10-13");
  });

  it("crosses month boundaries", () => {
    expect(isoDayOffset("2026-10-31", 1)).toBe("2026-11-01");
    expect(isoDayOffset("2026-11-01", -1)).toBe("2026-10-31");
  });
});

describe("isWeekendISO", () => {
  it("flags Saturday and Sunday only", () => {
    expect(isWeekendISO("2026-10-17")).toBe(true);
    expect(isWeekendISO("2026-10-18")).toBe(true);
    expect(isWeekendISO("2026-10-16")).toBe(false);
    expect(isWeekendISO("2026-10-19")).toBe(false);
  });
});

describe("pickFutureWeekdayISO", () => {
  // Week of Mon 2026-10-12: every run day must land on a visible tile.
  it.each([
    ["2026-10-12", "2026-10-19"],
    ["2026-10-13", "2026-10-20"],
    ["2026-10-14", "2026-10-21"],
    ["2026-10-15", "2026-10-22"],
    ["2026-10-16", "2026-10-23"],
    ["2026-10-17", "2026-10-20"],
    ["2026-10-18", "2026-10-21"],
  ])("picks a same-month weekday running on %s", (today, expected) => {
    expect(pickFutureWeekdayISO(today, [7, 3, 1], 1)).toBe(expected);
  });

  it("falls back past month's end onto the nearest upcoming weekday", () => {
    // Fri Oct 30: +7/+3 leave the month, +1 is a Saturday.
    expect(pickFutureWeekdayISO("2026-10-30", [7, 3, 1], 1)).toBe("2026-11-02");
  });

  it("keeps a fallback that already lands on a weekday", () => {
    // Thu Oct 29 with no in-month candidate: tomorrow is Friday.
    expect(pickFutureWeekdayISO("2026-10-29", [7, 3], 1)).toBe("2026-10-30");
  });

  it("never lands on a weekend across two months of run days", () => {
    let day = "2026-10-01";
    for (let i = 0; i < 61; i++) {
      expect(isWeekendISO(pickFutureWeekdayISO(day, [7, 3, 1], 1))).toBe(false);
      day = isoDayOffset(day, 1);
    }
  });
});

describe("pickPastWeekdayISO", () => {
  it.each([
    ["2026-10-14", "2026-10-13"],
    ["2026-10-17", "2026-10-16"],
    ["2026-10-18", "2026-10-16"],
    ["2026-10-12", "2026-10-09"],
  ])("pushes yesterday back to Friday running on %s", (today, expected) => {
    expect(pickPastWeekdayISO(today, 1)).toBe(expected);
  });
});

describe("elapsedHalfHour", () => {
  it.each([
    [new Date(2026, 9, 20, 13, 14), "PM", "01:00"],
    [new Date(2026, 9, 20, 13, 15), "PM", "01:00"],
    [new Date(2026, 9, 20, 13, 30), "PM", "01:30"],
    [new Date(2026, 9, 20, 11, 30), "PM", "11:30"],
    [new Date(2026, 9, 20, 11, 29), "AM", "11:00"],
    [new Date(2026, 9, 20, 2, 0), "AM", "02:00"],
    [new Date(2026, 9, 20, 0, 5), "AM", "12:00"],
  ])("derives the elapsed half-hour and its live period", (now, meridiem, slot) => {
    expect(elapsedHalfHour(now)).toEqual({ meridiem, slot });
  });

  it("returns null once both grids are exhausted", () => {
    expect(elapsedHalfHour(new Date(2026, 9, 20, 23, 30))).toBeNull();
    expect(elapsedHalfHour(new Date(2026, 9, 20, 23, 59))).toBeNull();
  });
});

describe("zonedTodayISO", () => {
  it("reads the day in the given zone across the date boundary", () => {
    // Noon UTC October 15: UTC reads the 15th, Auckland (NZDT, UTC+13)
    // already reads the 16th, Honolulu (UTC-10) still reads the 15th.
    const noon = new Date("2026-10-15T12:00:00Z");
    expect(zonedTodayISO("UTC", noon)).toBe("2026-10-15");
    expect(zonedTodayISO("Pacific/Auckland", noon)).toBe("2026-10-16");
    expect(zonedTodayISO("Pacific/Honolulu", noon)).toBe("2026-10-15");
    expect(zonedTodayISO("Pacific/Honolulu", new Date("2026-10-15T09:00:00Z"))).toBe("2026-10-14");
  });

  it("degrades to the runner-local day for a missing or unknown zone", () => {
    // Local noon: the runner's own calendar day, in any machine zone.
    const localNoon = new Date(2026, 9, 15, 12, 0, 0);
    expect(zonedTodayISO(null, localNoon)).toBe("2026-10-15");
    expect(zonedTodayISO("Not/AZone", localNoon)).toBe("2026-10-15");
    expect(zonedTodayISO("", localNoon)).toBe("2026-10-15");
  });
});
