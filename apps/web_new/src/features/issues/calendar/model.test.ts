// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { IssueListItem } from "@pidash/api-client";
import {
  canAttachToDay,
  decideDrop,
  groupRowsByDate,
  isTouchLayout,
  monthTitle,
  monthWeeks,
  normalizeStart,
  rangeForCells,
  rowsFromGroupedBuckets,
  shortMonthTitles,
  stepMonth,
  stepWeek,
  todayIso,
  weekRow,
  weekTitle,
  weekdayLabels,
  weekdayLabelsForWeek,
} from "./model.js";

const NOW = new Date(2026, 9, 15, 12, 0, 0);

function row(overrides: Partial<IssueListItem>): IssueListItem {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Issue",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 1,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

describe("calendar model", () => {
  it("builds a Sunday-first month grid with the rows the month needs", () => {
    const weeks = monthWeeks(2026, 9, 0, true, NOW);
    expect(weeks).toHaveLength(5);
    expect(weeks[0]).toHaveLength(7);
    expect(weeks[0]?.[0]?.iso).toBe("2026-09-27");
    expect(weeks[0]?.[0]?.inMonth).toBe(false);
    expect(weeks[0]?.[4]?.iso).toBe("2026-10-01");
    expect(weeks[0]?.[4]?.inMonth).toBe(true);
    expect(weeks[4]?.[6]?.iso).toBe("2026-10-31");
    expect(weeks[4]?.[6]?.inMonth).toBe(true);
  });

  it("uses four rows for a month that fits them", () => {
    const weeks = monthWeeks(2026, 1, 0, true, NOW);
    expect(weeks).toHaveLength(4);
    expect(weeks[0]?.[0]?.iso).toBe("2026-02-01");
    expect(weeks[3]?.[6]?.iso).toBe("2026-02-28");
  });

  it("rotates the grid to a Monday week start", () => {
    const weeks = monthWeeks(2026, 9, 1, true, NOW);
    expect(weeks[0]?.[0]?.iso).toBe("2026-09-28");
    expect(weekdayLabels(1)).toEqual(["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
  });

  it("drops weekend columns when asked", () => {
    const weeks = monthWeeks(2026, 9, 0, false, NOW);
    expect(weeks[0]).toHaveLength(5);
    expect(weeks[0]?.some((cell) => cell.weekend)).toBe(false);
    expect(weekdayLabelsForWeek(weeks[0] ?? [])).toEqual(["Mon", "Tue", "Wed", "Thu", "Fri"]);
  });

  it("derives weekday labels from visible cells on a Monday-start week", () => {
    const week = weekRow(new Date(2026, 9, 15), 1, false, NOW);
    expect(weekdayLabelsForWeek(week)).toEqual(["Mon", "Tue", "Wed", "Thu", "Fri"]);
  });

  it("badges today and marks weekend cells", () => {
    const weeks = monthWeeks(2026, 9, 0, true, NOW);
    const cells = weeks.flat();
    expect(cells.find((cell) => cell.iso === "2026-10-15")?.isToday).toBe(true);
    expect(cells.find((cell) => cell.iso === "2026-10-17")?.weekend).toBe(true);
    expect(cells.find((cell) => cell.iso === "2026-10-16")?.weekend).toBe(false);
  });

  it("builds a single week row and clamps bad week starts", () => {
    const week = weekRow(new Date(2026, 9, 15), 0, true, NOW);
    expect(week).toHaveLength(7);
    expect(week[0]?.iso).toBe("2026-10-11");
    expect(week[6]?.iso).toBe("2026-10-17");
    expect(normalizeStart(9)).toBe(0);
    expect(normalizeStart(-1)).toBe(0);
    expect(normalizeStart(1.5)).toBe(0);
    expect(normalizeStart(6)).toBe(6);
  });

  it("derives the inclusive fetch window from the visible cells", () => {
    expect(rangeForCells(monthWeeks(2026, 9, 0, true, NOW))).toEqual({
      after: "2026-09-27",
      before: "2026-10-31",
    });
    expect(rangeForCells([weekRow(new Date(2026, 9, 15), 0, true, NOW)])).toEqual({
      after: "2026-10-11",
      before: "2026-10-17",
    });
    expect(rangeForCells([])).toEqual({ after: todayIso(), before: todayIso() });
  });

  it("titles months and same-month or split weeks", () => {
    expect(monthTitle(2026, 9)).toBe("October 2026");
    expect(weekTitle(weekRow(new Date(2026, 9, 15), 0, true, NOW))).toBe("Oct 11 – Oct 17, 2026");
    expect(weekTitle(weekRow(new Date(2026, 9, 1), 0, true, NOW))).toBe("Sep 27 – Oct 3, 2026");
    expect(weekTitle(weekRow(new Date(2026, 11, 31), 0, true, NOW))).toBe("Dec 27, 2026 – Jan 2, 2027");
    expect(weekTitle([])).toBe("");
  });

  it("ranges every week title so prev/next always move it (ISS-022)", () => {
    // Same-month weeks used to collapse to the bare month name, freezing
    // the header while the grid stepped; every week step must move it.
    const anchor = new Date(2026, 9, 15);
    const current = weekTitle(weekRow(anchor, 0, true, NOW));
    expect(current).toBe("Oct 11 – Oct 17, 2026");
    expect(weekTitle(weekRow(stepWeek(anchor, -1), 0, true, NOW))).not.toBe(current);
    expect(weekTitle(weekRow(stepWeek(anchor, 1), 0, true, NOW))).not.toBe(current);
  });

  it("steps months and weeks across year boundaries", () => {
    expect(stepMonth(2026, 11, 1)).toEqual({ year: 2027, month0: 0 });
    expect(stepMonth(2026, 0, -1)).toEqual({ year: 2025, month0: 11 });
    expect(stepWeek(new Date(2026, 9, 15), 1).getDate()).toBe(22);
    expect(stepWeek(new Date(2026, 9, 15), -1).getDate()).toBe(8);
    expect(shortMonthTitles()).toHaveLength(12);
    expect(shortMonthTitles()[0]).toBe("Jan");
  });

  it("groups rows by a valid target date and skips undated rows", () => {
    const grouped = groupRowsByDate([
      row({ id: "a", target_date: "2026-10-05" }),
      row({ id: "b", target_date: "2026-10-05" }),
      row({ id: "c", target_date: null }),
      row({ id: "d" }),
      row({ id: "e", target_date: "not-a-day" }),
    ]);
    expect([...grouped.keys()]).toEqual(["2026-10-05"]);
    expect(grouped.get("2026-10-05")?.map((entry) => entry.id)).toEqual(["a", "b"]);
  });

  it("flattens grouped buckets", () => {
    const rows = rowsFromGroupedBuckets({
      "2026-10-05": { results: [row({ id: "a" })] },
      "2026-10-06": { results: [row({ id: "b" }), row({ id: "c" })] },
    });
    expect(rows.map((entry) => entry.id)).toEqual(["a", "b", "c"]);
  });

  it("decides drag drops by same-day, start-date, and validity rules", () => {
    expect(decideDrop({ target_date: "2026-10-05", start_date: null }, "2026-10-05")).toEqual({ kind: "noop" });
    expect(decideDrop({ target_date: "2026-10-05", start_date: "2026-10-07" }, "2026-10-06")).toEqual({
      kind: "refuse",
      reason: "The target date cannot fall before the start date.",
    });
    expect(decideDrop({ target_date: "2026-10-05", start_date: "2026-10-07" }, "2026-10-08")).toEqual({
      kind: "apply",
      targetDate: "2026-10-08",
    });
    expect(decideDrop({ target_date: "2026-10-05", start_date: null }, "bad")).toEqual({
      kind: "refuse",
      reason: "That date is not valid.",
    });
  });

  it("filters add-existing choices by start date", () => {
    expect(canAttachToDay({ start_date: null }, "2026-10-05")).toBe(true);
    expect(canAttachToDay({ start_date: "2026-10-05" }, "2026-10-05")).toBe(true);
    expect(canAttachToDay({ start_date: "2026-10-06" }, "2026-10-05")).toBe(false);
  });

  it("detects touch layouts under the md breakpoint", () => {
    expect(isTouchLayout(true, 390)).toBe(true);
    expect(isTouchLayout(false, 390)).toBe(false);
    expect(isTouchLayout(true, 1024)).toBe(false);
  });

  it("falls back for out-of-range months and empty week ranges", () => {
    expect(monthTitle(2026, 99)).toBe(" 2026");
    expect(weekTitle([])).toBe("");
    expect(weekTitle([{ iso: "bogus", day: 1, inMonth: true, weekend: false, isToday: false }])).toBe("");
  });
});
