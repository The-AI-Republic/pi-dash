// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The filter model: lifecycle values combine as OR, every date entry is
// ANDed, presets resolve against the project-zone today, custom entries
// bound the day from one side, and a missing date never matches a window.

import type { CycleRow as CycleRowData } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { makeCycleRow } from "../testing.js";
import {
  countCycleFilters,
  CYCLE_FILTER_CHIP_LABELS,
  CYCLE_FILTER_SECTION_TITLES,
  CYCLE_FILTER_STATUS_VALUES,
  EMPTY_CYCLE_FILTERS,
  hasCycleFilters,
  makeCycleFilter,
  parseCycleDateEntry,
  type CycleListFilters,
} from "./cycleFilters.js";

/** The oracle's frozen today, noon UTC so zones agree on the day. */
const NOW = new Date("2026-10-10T12:00:00Z");

function filters(overrides: Partial<CycleListFilters> = {}): CycleListFilters {
  return { ...EMPTY_CYCLE_FILTERS, ...overrides };
}

function row(overrides: Partial<CycleRowData> = {}): CycleRowData {
  return makeCycleRow(overrides);
}

describe("cycle filter dimensions", () => {
  it("pins the section titles and lifecycle order", () => {
    expect(CYCLE_FILTER_SECTION_TITLES).toEqual({
      status: "Status of the cycle",
      startDate: "Start date",
      endDate: "Due date",
    });
    expect(CYCLE_FILTER_STATUS_VALUES).toEqual(["current", "upcoming", "completed", "draft"]);
    expect(CYCLE_FILTER_CHIP_LABELS).toEqual({ status: "Status", startDate: "Start date", endDate: "Due date" });
  });

  it("starts empty and counts applied values across dimensions", () => {
    expect(hasCycleFilters(EMPTY_CYCLE_FILTERS)).toBe(false);
    expect(countCycleFilters(EMPTY_CYCLE_FILTERS)).toBe(0);
    const applied = filters({ status: ["upcoming"], startDate: ["1_weeks;after;fromnow"], endDate: [] });
    expect(hasCycleFilters(applied)).toBe(true);
    expect(countCycleFilters(applied)).toBe(2);
  });
});

describe("parseCycleDateEntry", () => {
  it("reads presets as day leads with thirty-day months", () => {
    expect(parseCycleDateEntry("1_weeks;after;fromnow")).toEqual({ kind: "preset", days: 7 });
    expect(parseCycleDateEntry("2_weeks;after;fromnow")).toEqual({ kind: "preset", days: 14 });
    expect(parseCycleDateEntry("1_months;after;fromnow")).toEqual({ kind: "preset", days: 30 });
    expect(parseCycleDateEntry("2_months;after;fromnow")).toEqual({ kind: "preset", days: 60 });
  });

  it("accepts singular units and larger amounts", () => {
    expect(parseCycleDateEntry("3_week;after;fromnow")).toEqual({ kind: "preset", days: 21 });
    expect(parseCycleDateEntry("3_month;after;fromnow")).toEqual({ kind: "preset", days: 90 });
  });

  it("reads custom bounds with their direction", () => {
    expect(parseCycleDateEntry("2026-11-01;after")).toEqual({
      kind: "custom",
      day: "2026-11-01",
      direction: "after",
    });
    expect(parseCycleDateEntry("2026-11-01;before")).toEqual({
      kind: "custom",
      day: "2026-11-01",
      direction: "before",
    });
  });

  it("rejects past windows, bare days and malformed entries", () => {
    expect(parseCycleDateEntry("1_weeks;before;fromnow")).toEqual({ kind: "unknown" });
    expect(parseCycleDateEntry("2026-11-01")).toEqual({ kind: "unknown" });
    expect(parseCycleDateEntry("2026-1-1;after")).toEqual({ kind: "unknown" });
    expect(parseCycleDateEntry("tomorrow;after")).toEqual({ kind: "unknown" });
    expect(parseCycleDateEntry("")).toEqual({ kind: "unknown" });
  });
});

describe("makeCycleFilter", () => {
  it("keeps every row when no dimension applies", () => {
    const keep = makeCycleFilter(EMPTY_CYCLE_FILTERS, "UTC", NOW);
    expect(keep(row({ status: "current" }))).toBe(true);
    expect(keep(row({ status: undefined, start_date: null, end_date: null }))).toBe(true);
  });

  it("matches lifecycle states as OR, case-insensitively", () => {
    const keep = makeCycleFilter(filters({ status: ["upcoming", "draft"] }), "UTC", NOW);
    expect(keep(row({ status: "upcoming" }))).toBe(true);
    expect(keep(row({ status: "DRAFT" }))).toBe(true);
    expect(keep(row({ status: "current" }))).toBe(false);
    expect(keep(row({ status: "completed" }))).toBe(false);
    expect(keep(row({ status: undefined }))).toBe(false);
  });

  it("keeps cycles starting at least the preset lead ahead", () => {
    const keep = makeCycleFilter(filters({ startDate: ["1_weeks;after;fromnow"] }), "UTC", NOW);
    // 2026-10-10 plus seven days is 2026-10-17; the boundary keeps.
    expect(keep(row({ start_date: "2026-10-17T12:00:00Z" }))).toBe(true);
    expect(keep(row({ start_date: "2026-11-01T12:00:00Z" }))).toBe(true);
    expect(keep(row({ start_date: "2026-10-16T12:00:00Z" }))).toBe(false);
    expect(keep(row({ start_date: "2026-09-01T12:00:00Z" }))).toBe(false);
  });

  it("keeps cycles ending at least the preset lead ahead", () => {
    const keep = makeCycleFilter(filters({ endDate: ["1_months;after;fromnow"] }), "UTC", NOW);
    expect(keep(row({ end_date: "2026-12-31T12:00:00Z" }))).toBe(true);
    expect(keep(row({ end_date: "2026-10-20T12:00:00Z" }))).toBe(false);
    expect(keep(row({ end_date: "2026-02-01T12:00:00Z" }))).toBe(false);
  });

  it("ANDs several entries in one window", () => {
    const keep = makeCycleFilter(
      filters({ startDate: ["1_weeks;after;fromnow", "2_weeks;after;fromnow"] }),
      "UTC",
      NOW
    );
    expect(keep(row({ start_date: "2026-11-01T12:00:00Z" }))).toBe(true);
    expect(keep(row({ start_date: "2026-10-17T12:00:00Z" }))).toBe(false);
  });

  it("ANDs the two windows with each other", () => {
    const keep = makeCycleFilter(
      filters({ startDate: ["1_weeks;after;fromnow"], endDate: ["1_months;after;fromnow"] }),
      "UTC",
      NOW
    );
    expect(keep(row({ start_date: "2026-11-01T12:00:00Z", end_date: "2026-12-31T12:00:00Z" }))).toBe(true);
    expect(keep(row({ start_date: "2026-11-01T12:00:00Z", end_date: "2026-10-20T12:00:00Z" }))).toBe(false);
    expect(keep(row({ start_date: "2026-09-01T12:00:00Z", end_date: "2026-12-31T12:00:00Z" }))).toBe(false);
  });

  it("ANDs lifecycle states with the date windows", () => {
    const keep = makeCycleFilter(filters({ status: ["upcoming"], startDate: ["1_weeks;after;fromnow"] }), "UTC", NOW);
    expect(keep(row({ status: "upcoming", start_date: "2026-11-01T12:00:00Z" }))).toBe(true);
    expect(keep(row({ status: "current", start_date: "2026-11-01T12:00:00Z" }))).toBe(false);
    expect(keep(row({ status: "upcoming", start_date: "2026-09-01T12:00:00Z" }))).toBe(false);
  });

  it("bounds custom days from the stored side, edges included", () => {
    const after = makeCycleFilter(filters({ startDate: ["2026-11-01;after"] }), "UTC", NOW);
    expect(after(row({ start_date: "2026-11-01T12:00:00Z" }))).toBe(true);
    expect(after(row({ start_date: "2026-11-02T12:00:00Z" }))).toBe(true);
    expect(after(row({ start_date: "2026-10-31T12:00:00Z" }))).toBe(false);
    const before = makeCycleFilter(filters({ endDate: ["2026-11-01;before"] }), "UTC", NOW);
    expect(before(row({ end_date: "2026-11-01T12:00:00Z" }))).toBe(true);
    expect(before(row({ end_date: "2026-10-31T12:00:00Z" }))).toBe(true);
    expect(before(row({ end_date: "2026-11-02T12:00:00Z" }))).toBe(false);
  });

  it("reads a custom range pair as both bounds together", () => {
    const keep = makeCycleFilter(filters({ startDate: ["2026-11-01;after", "2026-11-30;before"] }), "UTC", NOW);
    expect(keep(row({ start_date: "2026-11-15T12:00:00Z" }))).toBe(true);
    expect(keep(row({ start_date: "2026-10-15T12:00:00Z" }))).toBe(false);
    expect(keep(row({ start_date: "2026-12-15T12:00:00Z" }))).toBe(false);
  });

  it("drops undated and broken dates from every dated filter", () => {
    const keep = makeCycleFilter(
      filters({ startDate: ["1_weeks;after;fromnow"], endDate: ["2026-11-01;after"] }),
      "UTC",
      NOW
    );
    expect(keep(row({ start_date: null, end_date: null }))).toBe(false);
    expect(keep(row({ start_date: "2026-11-01T12:00:00Z", end_date: null }))).toBe(false);
    expect(keep(row({ start_date: "not-a-date", end_date: "2026-12-31T12:00:00Z" }))).toBe(false);
    expect(keep(row({ start_date: "2026-11-01T12:00:00Z", end_date: "not-a-date" }))).toBe(false);
  });

  it("matches nothing for an unknown entry", () => {
    const keep = makeCycleFilter(filters({ startDate: ["tomorrow;after"] }), "UTC", NOW);
    expect(keep(row({ start_date: "2026-11-01T12:00:00Z" }))).toBe(false);
  });

  it("resolves days and today in the project timezone", () => {
    // 2026-10-10T01:00:00Z is still Oct 9 behind UTC; a cycle starting
    // that instant reads Oct 9 in New York and Oct 10 in UTC.
    const at = new Date("2026-10-10T01:00:00Z");
    const newYork = makeCycleFilter(filters({ startDate: ["2026-10-10;after"] }), "America/New_York", at);
    expect(newYork(row({ start_date: "2026-10-10T01:00:00Z" }))).toBe(false);
    const utc = makeCycleFilter(filters({ startDate: ["2026-10-10;after"] }), "UTC", at);
    expect(utc(row({ start_date: "2026-10-10T01:00:00Z" }))).toBe(true);
  });
});
