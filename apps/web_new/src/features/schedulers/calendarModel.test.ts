// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar geometry (parity/specs/agents.md §5, AGT-015/016): whole weeks
// Sunday to Saturday, month grids on the same week boundary, day bucketing,
// the minute-to-pixel mapping and the packing rules for crowded days.
import { describe, expect, it } from "vitest";
import type { SchedulerOccurrence } from "@pidash/api-client";

import {
  DAY_HEIGHT,
  HOUR_HEIGHT,
  addDays,
  assignWeekLanes,
  dayKey,
  daysBetween,
  formatBlockTime,
  formatDayLabel,
  formatMonthYear,
  isSameDay,
  monthCells,
  monthWindow,
  nextMonth,
  occurrenceTop,
  occurrencesByDay,
  planMonthDay,
  previousMonth,
  rollupDay,
  startOfMonth,
  startOfNextDay,
  startOfWeek,
  timeLineTop,
  weekDays,
  weekWindow,
} from "./calendarModel.js";

function at(year: number, month: number, day: number, hour = 0, minute = 0): Date {
  return new Date(year, month, day, hour, minute, 0, 0);
}

function occurrence(
  dtstart: string,
  name = "Weekly",
  overrides: Partial<SchedulerOccurrence> = {}
): SchedulerOccurrence {
  return {
    binding_id: "b1",
    scheduler_id: "s1",
    scheduler_name: name,
    scheduler_color: "#5e6ad2",
    dtstart,
    tzid: "UTC",
    kind: "scheduled",
    agent_run_id: null,
    status: null,
    ...overrides,
  };
}

describe("week boundaries", () => {
  it("starts weeks on Sunday midnight, including on a Sunday", () => {
    expect(startOfWeek(at(2026, 9, 21, 13, 45))).toEqual(at(2026, 9, 18));
    expect(startOfWeek(at(2026, 9, 18))).toEqual(at(2026, 9, 18));
  });

  it("walks days across a month and year boundary", () => {
    expect(addDays(at(2026, 11, 31), 1)).toEqual(at(2027, 0, 1));
    expect(addDays(at(2026, 9, 1), -1)).toEqual(at(2026, 8, 30));
  });

  it("covers the visible week inclusively in the query window", () => {
    const window = weekWindow(at(2026, 9, 21, 15));
    expect(new Date(window.from)).toEqual(at(2026, 9, 18));
    expect(new Date(window.to).getTime()).toBe(addDays(at(2026, 9, 18), 7).getTime() - 1);
  });

  it("lists seven days oldest first", () => {
    const days = weekDays(at(2026, 9, 21));
    expect(days).toHaveLength(7);
    expect(dayKey(days[0] as Date)).toBe("2026-10-18");
    expect(dayKey(days[6] as Date)).toBe("2026-10-24");
  });
});

describe("month boundaries", () => {
  it("starts months at their first day and steps both ways", () => {
    expect(startOfMonth(at(2026, 9, 21, 9))).toEqual(at(2026, 9, 1));
    expect(nextMonth(at(2026, 11, 1))).toEqual(at(2027, 0, 1));
    expect(previousMonth(at(2026, 0, 1))).toEqual(at(2025, 11, 1));
  });

  it("spans whole weeks from the first to the last day", () => {
    const window = monthWindow(at(2026, 9, 21));
    expect(new Date(window.from)).toEqual(at(2026, 8, 27));
    expect(new Date(window.to).getTime()).toBe(addDays(at(2026, 9, 31), 1).getTime() - 1);
  });

  it("spans five weeks when the month sits inside one", () => {
    // November 2026 starts on a Sunday and ends on a Monday.
    const window = monthWindow(at(2026, 10, 1));
    expect(new Date(window.from)).toEqual(at(2026, 10, 1));
    // November 30 is a Monday, so the grid ends on Saturday 5 December.
    expect(new Date(window.to)).toEqual(new Date(2026, 11, 5, 23, 59, 59, 999));
  });

  it("marks leading and trailing cells outside the month", () => {
    const cells = monthCells(at(2026, 9, 1));
    expect(cells).toHaveLength(35);
    expect(cells[0]?.inMonth).toBe(false);
    expect(cells.find((cell) => dayKey(cell.date) === "2026-10-01")?.inMonth).toBe(true);
    // October 2026 ends on a Saturday, so the last cell still belongs to it.
    expect(cells.at(-1)?.inMonth).toBe(true);
    expect(dayKey(cells.at(-1)?.date as Date)).toBe("2026-10-31");
  });
});

describe("day keys and buckets", () => {
  it("compares days by local calendar date", () => {
    expect(dayKey(at(2026, 9, 21, 23, 59))).toBe("2026-10-21");
    expect(isSameDay(at(2026, 9, 21, 1), at(2026, 9, 21, 23))).toBe(true);
    expect(isSameDay(at(2026, 9, 21), at(2026, 9, 22))).toBe(false);
  });

  it("buckets firings by day in time order", () => {
    const buckets = occurrencesByDay([
      occurrence(at(2026, 9, 21, 11).toISOString()),
      occurrence(at(2026, 9, 21, 9).toISOString()),
      occurrence(at(2026, 9, 22, 8).toISOString()),
    ]);
    expect([...buckets.keys()]).toEqual(["2026-10-21", "2026-10-22"]);
    expect((buckets.get("2026-10-21") ?? []).map((row) => new Date(row.dtstart).getHours())).toEqual([9, 11]);
  });

  it("counts whole days and starts the next day at midnight", () => {
    expect(daysBetween(at(2026, 9, 21, 23), at(2026, 9, 23, 1))).toBe(2);
    expect(daysBetween(at(2026, 9, 23), at(2026, 9, 21))).toBe(-2);
    expect(startOfNextDay(at(2026, 9, 21, 22))).toEqual(at(2026, 9, 22));
  });
});

describe("time-to-pixel mapping", () => {
  it("maps midday to half the day lane", () => {
    expect(HOUR_HEIGHT).toBe(48);
    expect(DAY_HEIGHT).toBe(1152);
    expect(timeLineTop(at(2026, 9, 21, 12, 0))).toBe(576);
    expect(timeLineTop(at(2026, 9, 21, 0, 30))).toBe(24);
  });

  it("places a firing by its local minute", () => {
    expect(occurrenceTop(at(2026, 9, 21, 9, 15).toISOString())).toBe(444);
  });
});

describe("labels", () => {
  it("formats month-year, day and block time", () => {
    expect(formatMonthYear(at(2026, 9, 1))).toContain("2026");
    expect(formatDayLabel(at(2026, 9, 21))).toBe("Wed 21");
    expect(formatBlockTime(at(2026, 9, 21, 9, 5))).toMatch(/9:05/);
  });
});

describe("crowded days", () => {
  const many = Array.from({ length: 24 }, (_, index) => occurrence(at(2026, 9, 21, 0, index).toISOString(), "Hourly"));

  it("keeps every block when the day fits", () => {
    const plan = planMonthDay(many.slice(0, 4));
    expect(plan.blocks).toHaveLength(4);
    expect(plan.more).toBeNull();
  });

  it("folds the rest behind a count", () => {
    const plan = planMonthDay(many);
    expect(plan.blocks).toHaveLength(4);
    expect(plan.more).toBe("+ 20 more");
  });

  it("keeps the folded firings so the overflow control can open them", () => {
    const plan = planMonthDay(many);
    expect(plan.folded).toEqual(many.slice(4));
    expect(planMonthDay(many.slice(0, 4)).folded).toEqual([]);
  });

  it("rolls a truncated day up per scheduler", () => {
    expect(rollupDay(many).map(({ name, count, label }) => ({ name, count, label }))).toEqual([
      { name: "Hourly", count: 24, label: "24× Hourly" },
    ]);
    expect(
      rollupDay([...many, occurrence(at(2026, 9, 21, 5).toISOString(), "Daily")]).map((rollup) => rollup.label)
    ).toEqual(["24× Hourly", "1× Daily"]);
  });

  it("points each rollup at its scheduler's first firing of the day", () => {
    const daily = occurrence(at(2026, 9, 21, 5).toISOString(), "Daily");
    const [hourly, dailyRollup] = rollupDay([...many, daily]);
    expect(hourly?.first).toBe(many[0]);
    expect(dailyRollup?.first).toBe(daily);
  });
});

describe("week lanes", () => {
  it("keeps one column when starts differ", () => {
    const lanes = assignWeekLanes([
      occurrence(at(2026, 9, 21, 8, 0).toISOString(), "A"),
      occurrence(at(2026, 9, 21, 8, 30).toISOString(), "B"),
    ]);
    expect(lanes.map(({ lane, lanes: width }) => ({ lane, width }))).toEqual([
      { lane: 0, width: 1 },
      { lane: 0, width: 1 },
    ]);
  });

  it("numbers same-instant firings across one group", () => {
    const start = at(2026, 9, 21, 8, 0).toISOString();
    const lanes = assignWeekLanes([
      occurrence(start, "A"),
      occurrence(at(2026, 9, 21, 8, 30).toISOString(), "B"),
      occurrence(start, "C"),
      occurrence(start, "D"),
    ]);
    expect(
      lanes.map(({ occurrence: firing, lane, lanes: width }) => ({ name: firing.scheduler_name, lane, width }))
    ).toEqual([
      { name: "A", lane: 0, width: 3 },
      { name: "B", lane: 0, width: 1 },
      { name: "C", lane: 1, width: 3 },
      { name: "D", lane: 2, width: 3 },
    ]);
  });
});
