// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar layout model (parity/specs/issues.md §4, ISS-021–027). Pure
// date math for month and week grids, their fetch window, the header
// title, and the drag rule. Week rows honor the profile's first day and
// drop weekend columns when the option is off.

import type { IssueListItem } from "@pidash/api-client";

import { isIsoDay, parseIsoDay, toIsoDay } from "../dropdowns/calendar.js";

/** One rendered calendar day. */
export interface CalendarCell {
  iso: string;
  day: number;
  inMonth: boolean;
  weekend: boolean;
  isToday: boolean;
}

/** A calendar row: seven cells in month mode, five or seven in week mode. */
export type CalendarWeek = CalendarCell[];

/** Calendar display mode and its persisted options. */
export interface CalendarOptions {
  mode: "month" | "week";
  showWeekends: boolean;
}

/** The visible date range driving the windowed read. */
export interface CalendarRange {
  /** First visible ISO day. */
  after: string;
  /** Last visible ISO day. */
  before: string;
}

const MONTH_TITLES = [
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
] as const;

const SHORT_MONTH_TITLES = [
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
] as const;

/** Weekday labels in Sunday-first order. */
const WEEKDAY_LABELS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;

/** Weekday labels rotated to the profile's first day. */
export function weekdayLabels(weekStart: number): string[] {
  const start = normalizeStart(weekStart);
  return Array.from({ length: 7 }, (_, offset) => WEEKDAY_LABELS[(start + offset) % 7] as string);
}

/** Clamp an unknown week start onto Sunday-first 0-6. */
export function normalizeStart(weekStart: number): number {
  if (!Number.isInteger(weekStart) || weekStart < 0 || weekStart > 6) return 0;
  return weekStart;
}

/** Calendar-safe local date from a year/month/day triple. */
function localDate(year: number, month0: number, day: number): Date {
  return new Date(year, month0, day);
}

/** today's ISO day, local. */
export function todayIso(now: Date = new Date()): string {
  return toIsoDay(now.getFullYear(), now.getMonth(), now.getDate());
}

function startOfWeek(date: Date, weekStart: number): Date {
  const start = normalizeStart(weekStart);
  const offset = (date.getDay() - start + 7) % 7;
  return localDate(date.getFullYear(), date.getMonth(), date.getDate() - offset);
}

function addDays(date: Date, days: number): Date {
  return localDate(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

function cellFor(date: Date, inMonth: boolean, today: string): CalendarCell {
  const iso = toIsoDay(date.getFullYear(), date.getMonth(), date.getDate());
  const weekday = date.getDay();
  return {
    iso,
    day: date.getDate(),
    inMonth,
    weekend: weekday === 0 || weekday === 6,
    isToday: iso === today,
  };
}

function visibleCells(cells: CalendarCell[], showWeekends: boolean): CalendarCell[] {
  return showWeekends ? cells : cells.filter((cell) => !cell.weekend);
}

/** Every week row of the active month; filler days keep rows full. */
export function monthWeeks(
  year: number,
  month0: number,
  weekStart: number,
  showWeekends: boolean,
  now: Date = new Date()
): CalendarWeek[] {
  const today = todayIso(now);
  const first = localDate(year, month0, 1);
  const gridStart = startOfWeek(first, weekStart);
  const weeks: CalendarWeek[] = [];
  for (let week = 0; week < 6; week += 1) {
    const cells: CalendarCell[] = [];
    for (let offset = 0; offset < 7; offset += 1) {
      const date = addDays(gridStart, week * 7 + offset);
      cells.push(cellFor(date, date.getMonth() === month0, today));
    }
    weeks.push(visibleCells(cells, showWeekends));
  }
  return weeks;
}

/** The single active week, trimmed to the visible columns. */
export function weekRow(date: Date, weekStart: number, showWeekends: boolean, now: Date = new Date()): CalendarWeek {
  const today = todayIso(now);
  const start = startOfWeek(date, weekStart);
  const cells = Array.from({ length: 7 }, (_, offset) => cellFor(addDays(start, offset), true, today));
  return visibleCells(cells, showWeekends);
}

/** The fetch window covering the visible cells (inclusive). */
export function rangeForCells(weeks: readonly CalendarWeek[]): CalendarRange {
  const cells = weeks.flat();
  const first = cells[0];
  const last = cells[cells.length - 1];
  if (first === undefined || last === undefined) {
    const today = todayIso();
    return { after: today, before: today };
  }
  return { after: first.iso, before: last.iso };
}

/** Month title, e.g. "October 2026". */
export function monthTitle(year: number, month0: number): string {
  return `${MONTH_TITLES[month0] ?? ""} ${year}`;
}

/**
 * Week title: a single month name when the week fits one month, otherwise
 * the short range form ("Sep 28 – Oct 4, 2026").
 */
export function weekTitle(week: readonly CalendarCell[]): string {
  const first = week[0];
  const last = week[week.length - 1];
  if (first === undefined || last === undefined) return "";
  const from = parseIsoDay(first.iso);
  const to = parseIsoDay(last.iso);
  if (from === null || to === null) return "";
  if (from.year === to.year && from.month0 === to.month0) return monthTitle(from.year, from.month0);
  if (from.year === to.year) {
    return `${SHORT_MONTH_TITLES[from.month0]} ${from.day} – ${SHORT_MONTH_TITLES[to.month0]} ${to.day}, ${from.year}`;
  }
  return `${SHORT_MONTH_TITLES[from.month0]} ${from.day}, ${from.year} – ${SHORT_MONTH_TITLES[to.month0]} ${to.day}, ${to.year}`;
}

/** Short month labels for the picker grid, January-first. */
export function shortMonthTitles(): string[] {
  return [...SHORT_MONTH_TITLES];
}

/** A moved month; carries the year. */
export function stepMonth(year: number, month0: number, delta: number): { year: number; month0: number } {
  const total = year * 12 + month0 + delta;
  return { year: Math.floor(total / 12), month0: ((total % 12) + 12) % 12 };
}

/** A moved week; returns the new anchor date. */
export function stepWeek(date: Date, delta: number): Date {
  return addDays(date, delta * 7);
}

/** Group rows by their target date; undated rows are omitted. */
export function groupRowsByDate(rows: readonly IssueListItem[]): Map<string, IssueListItem[]> {
  const grouped = new Map<string, IssueListItem[]>();
  for (const row of rows) {
    const target = row.target_date;
    if (target === null || target === undefined || !isIsoDay(target)) continue;
    const bucket = grouped.get(target);
    if (bucket === undefined) grouped.set(target, [row]);
    else bucket.push(row);
  }
  return grouped;
}

/** Flatten a grouped response's day buckets into rows. */
export function rowsFromGroupedBuckets(
  buckets: Record<string, { results: readonly IssueListItem[] }>
): IssueListItem[] {
  const rows: IssueListItem[] = [];
  for (const bucket of Object.values(buckets)) rows.push(...bucket.results);
  return rows;
}

/** The result of a drag: apply, no-op, or refuse. */
export type DropDecision =
  | { kind: "apply"; targetDate: string }
  | { kind: "noop" }
  | { kind: "refuse"; reason: string };

/**
 * Drag rule (ISS-024): same-day drops do nothing; a target earlier than
 * the issue's start date is refused; everything else re-dates the item.
 */
export function decideDrop(
  issue: Pick<IssueListItem, "target_date" | "start_date">,
  destination: string
): DropDecision {
  if (!isIsoDay(destination)) return { kind: "refuse", reason: "That date is not valid." };
  if (issue.target_date === destination) return { kind: "noop" };
  if (issue.start_date !== null && issue.start_date !== undefined && issue.start_date > destination) {
    return { kind: "refuse", reason: "The target date cannot fall before the start date." };
  }
  return { kind: "apply", targetDate: destination };
}

/** Filter the add-existing choices: hide items starting after the day. */
export function canAttachToDay(issue: Pick<IssueListItem, "start_date">, day: string): boolean {
  return issue.start_date === null || issue.start_date === undefined || issue.start_date <= day;
}

/** True when the viewport is a narrow, touch layout. */
export function isTouchLayout(hasTouch: boolean, width: number): boolean {
  return hasTouch && width < 768;
}
