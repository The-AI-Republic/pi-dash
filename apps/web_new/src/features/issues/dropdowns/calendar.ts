// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar math behind the date pickers (parity/specs/issues.md §17,
// ISS-214/215). ISO-day parsing and formatting, the week-start-aware
// month grid with min/max disabling, the merged smart label, and the
// range click state machine (repick adjusts one end; a same-day double
// pick resets). Pure: no React, no network.
// The week-start primitives live in weekDays.ts (NEWFRONT-852, size
// gate) and are re-exported here, so existing importers keep working.

import { normalizeWeekStart } from "./weekDays.js";

export { DEFAULT_WEEK_START, normalizeWeekStart } from "./weekDays.js";

/** Full month names for the caption select, January-first. */
export const MONTH_NAMES: readonly string[] = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/** Abbreviated month names for value labels, January-first. */
export const MONTH_ABBREVS: readonly string[] = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** Full weekday names, Sunday-first (index matches Date#getDay). */
export const WEEKDAY_NAMES: readonly string[] = [
  "Sunday",
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
];

/** Two-letter weekday labels for the grid header, Sunday-first. */
export const WEEKDAY_SHORT: readonly string[] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/** Caption year bounds: wide enough for planning horizons, finite for the select. */
export const CAPTION_YEAR_MIN = 1900;
export const CAPTION_YEAR_MAX = 2100;

/** A picked range: either end may be empty while picking or clearing. */
export interface RangeValue {
  start: string | null;
  end: string | null;
}

const ISO_DAY_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;

/** True for real calendar days in YYYY-MM-DD form (rejects 2026-02-30). */
export function isIsoDay(value: unknown): value is string {
  if (typeof value !== "string") return false;
  const match = ISO_DAY_PATTERN.exec(value);
  if (match === null) return false;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (month < 1 || month > 12 || day < 1 || day > 31) return false;
  const built = new Date(year, month - 1, day);
  return built.getFullYear() === year && built.getMonth() === month - 1 && built.getDate() === day;
}

/** Format a Gregorian triple as an ISO day. */
export function toIsoDay(year: number, month0: number, day: number): string {
  return `${year}-${String(month0 + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

/** Split an ISO day into its parts; null when the value is not a day. */
export function parseIsoDay(value: string | null | undefined): { year: number; month0: number; day: number } | null {
  if (!isIsoDay(value)) return null;
  const [year, month, day] = value.split("-").map(Number) as [number, number, number];
  return { year, month0: month - 1, day };
}

/** Today's ISO day in local time. */
export function todayIsoDay(now: Date = new Date()): string {
  return toIsoDay(now.getFullYear(), now.getMonth(), now.getDate());
}

/** Weekday header labels rotated so the week starts on the given day. */
export function orderedWeekdays(weekStart: number): Array<{ full: string; short: string }> {
  const start = normalizeWeekStart(weekStart);
  return Array.from({ length: 7 }, (_, offset) => {
    const index = (start + offset) % 7;
    return { full: WEEKDAY_NAMES[index] as string, short: WEEKDAY_SHORT[index] as string };
  });
}

/** One grid cell: the ISO day plus its display state. */
export interface CalendarDay {
  iso: string;
  day: number;
  /** False for the leading/trailing filler days of adjacent months. */
  inMonth: boolean;
  disabled: boolean;
  isToday: boolean;
}

/**
 * Six week rows covering the visible month. Filler days from the
 * adjacent months keep every row full; min/max disable out-of-range
 * days (ISO strings compare chronologically).
 */
export function monthGrid(
  year: number,
  month0: number,
  weekStart: number,
  min?: string | null,
  max?: string | null,
  today: string = todayIsoDay()
): CalendarDay[][] {
  const start = normalizeWeekStart(weekStart);
  const first = new Date(year, month0, 1);
  const lead = (first.getDay() - start + 7) % 7;
  const cells: CalendarDay[] = [];
  for (let index = 0; index < 42; index++) {
    const date = new Date(year, month0, 1 - lead + index);
    const iso = toIsoDay(date.getFullYear(), date.getMonth(), date.getDate());
    cells.push({
      iso,
      day: date.getDate(),
      inMonth: date.getMonth() === month0,
      disabled: (min != null && min !== "" && iso < min) || (max != null && max !== "" && iso > max),
      isToday: iso === today,
    });
  }
  return Array.from({ length: 6 }, (_, week) => cells.slice(week * 7, week * 7 + 7));
}

/** Shift a visible month by whole months, carrying the year. */
export function shiftMonth(year: number, month0: number, delta: number): { year: number; month0: number } {
  const total = year * 12 + month0 + delta;
  return { year: Math.floor(total / 12), month0: ((total % 12) + 12) % 12 };
}

/** Render one ISO day as a short label ("Jan 5, 2026"). */
export function formatDay(iso: string): string {
  const parsed = parseIsoDay(iso);
  if (parsed === null) return iso;
  return `${MONTH_ABBREVS[parsed.month0]} ${parsed.day}, ${parsed.year}`;
}

/** Render one ISO day as a long label for assistive names. */
export function formatDayLong(iso: string): string {
  const parsed = parseIsoDay(iso);
  if (parsed === null) return iso;
  return `${MONTH_NAMES[parsed.month0]} ${parsed.day}, ${parsed.year}`;
}

/**
 * The merged range label: one month name when both ends share it,
 * both months otherwise, the year once when shared.
 */
export function formatMergedRange(start: string, end: string): string {
  const from = parseIsoDay(start);
  const to = parseIsoDay(end);
  if (from === null || to === null) return `${formatDay(start)} - ${formatDay(end)}`;
  const sameMonth = from.year === to.year && from.month0 === to.month0;
  if (sameMonth) return `${MONTH_ABBREVS[from.month0]} ${from.day} - ${to.day}, ${from.year}`;
  if (from.year === to.year) {
    return `${MONTH_ABBREVS[from.month0]} ${from.day} - ${MONTH_ABBREVS[to.month0]} ${to.day}, ${from.year}`;
  }
  return `${formatDay(start)} - ${formatDay(end)}`;
}

/**
 * Range click state machine. The first pick opens a pending range; a
 * second pick before the start restarts it, on the start resets it,
 * after the start completes it. On a complete range, clicking on or
 * before the start moves the start, clicking after it moves the end —
 * so repicking adjusts exactly one end.
 */
export function applyRangeClick(range: RangeValue, day: string): RangeValue {
  const { start, end } = range;
  if (start === null || start === "") return { start: day, end: null };
  if (end === null || end === "") {
    if (day === start) return { start: null, end: null };
    if (day < start) return { start: day, end: null };
    return { start, end: day };
  }
  if (day <= start) return { start: day, end };
  return { start, end: day };
}

/** True when the day falls strictly inside a complete range. */
export function isDayInRange(day: string, range: RangeValue): boolean {
  const { start, end } = range;
  if (start === null || start === "" || end === null || end === "") return false;
  return day > start && day < end;
}
