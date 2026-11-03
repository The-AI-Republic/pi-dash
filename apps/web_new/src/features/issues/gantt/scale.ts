// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline scale math (parity/specs/issues.md §6, ISS-044/045/048). The
// timeline is a day-indexed strip: every zoom draws the same days at a
// different width and groups the header into two tiers (months over days,
// months over weeks, quarters over months). Day arithmetic runs on UTC
// triples so a DST shift never moves a column. Pure: no React.

import { MONTH_ABBREVS, WEEKDAY_SHORT, parseIsoDay, toIsoDay } from "../dropdowns/calendar.js";

/** The three timeline zoom levels; no day zoom exists (ISS-045). */
export type GanttZoom = "week" | "month" | "quarter";

/** Switcher order and labels. */
export const GANTT_ZOOMS: readonly GanttZoom[] = ["week", "month", "quarter"];
export const GANTT_ZOOM_LABELS: Record<GanttZoom, string> = {
  week: "Week",
  month: "Month",
  quarter: "Quarter",
};

/** Pixels per day: week is widest, quarter narrowest (ISS-045). */
export const DAY_WIDTHS: Record<GanttZoom, number> = { week: 40, month: 16, quarter: 3 };

/** Days rendered around today when a zoom opens or re-centers. */
export const INITIAL_SPAN_DAYS: Record<GanttZoom, number> = { week: 56, month: 182, quarter: 728 };

/** Days appended by one edge extension (ISS-048); whole weeks each time. */
export const EXTENSION_DAYS: Record<GanttZoom, number> = { week: 14, month: 28, quarter: 91 };

/** Distance from the right edge that triggers an append (ISS-048). */
export const EDGE_TRIGGER_PX = 240;

/** A rendered window: its first day and how many days it covers. */
export interface GanttRange {
  startIso: string;
  dayCount: number;
}

/** One header cell: which day it begins on and how many days it spans. */
export interface GanttSegment {
  key: string;
  label: string;
  startIndex: number;
  days: number;
}

/** One day column of the rendered window. */
export interface GanttDay {
  iso: string;
  index: number;
  /** Sunday-first weekday of the day. */
  weekday: number;
  weekend: boolean;
  /** True on the profile's first day of the week. */
  startsWeek: boolean;
}

export function isGanttZoom(value: unknown): value is GanttZoom {
  return value === "week" || value === "month" || value === "quarter";
}

const MS_PER_DAY = 86_400_000;

function dayTriple(iso: string): { year: number; month0: number; day: number } {
  const parsed = parseIsoDay(iso);
  if (parsed === null) throw new RangeError(`Not an ISO day: ${iso}`);
  return parsed;
}

function utcOf(iso: string): number {
  const { year, month0, day } = dayTriple(iso);
  return Date.UTC(year, month0, day);
}

/** Shift an ISO day by a signed number of calendar days. */
export function addDays(iso: string, days: number): string {
  const shifted = new Date(utcOf(iso) + days * MS_PER_DAY);
  return toIsoDay(shifted.getUTCFullYear(), shifted.getUTCMonth(), shifted.getUTCDate());
}

/** Whole days from `fromIso` to `toIso` (negative when `toIso` is earlier). */
export function diffDays(toIso: string, fromIso: string): number {
  return Math.round((utcOf(toIso) - utcOf(fromIso)) / MS_PER_DAY);
}

/** Sunday-first weekday of an ISO day. */
export function weekdayOf(iso: string): number {
  return new Date(utcOf(iso)).getUTCDay();
}

/** The most recent `weekStart` weekday on or before `iso`. */
export function startOfWeek(iso: string, weekStart: number): string {
  const offset = (weekdayOf(iso) - weekStart + 7) % 7;
  return addDays(iso, -offset);
}

/**
 * The window a zoom opens on: a whole number of weeks with today near
 * the middle. Week-aligned starts keep both header tiers flush.
 */
export function initialRange(zoom: GanttZoom, todayIso: string, weekStart: number): GanttRange {
  const weeks = Math.ceil(INITIAL_SPAN_DAYS[zoom] / 7);
  const start = addDays(startOfWeek(todayIso, weekStart), -Math.floor(weeks / 2) * 7);
  return { startIso: start, dayCount: weeks * 7 };
}

/** The corresponding day window after shifting an edge (ISS-048). */
export function extendRange(range: GanttRange, side: "left" | "right", zoom: GanttZoom): GanttRange {
  const days = EXTENSION_DAYS[zoom];
  if (side === "right") return { startIso: range.startIso, dayCount: range.dayCount + days };
  return { startIso: addDays(range.startIso, -days), dayCount: range.dayCount + days };
}

/**
 * The smallest window grown from `range` that covers both days (ISS-057).
 * A scroll-to-block lands on a bar the initial window never rendered, so
 * the range grows in whole extension steps on the side that falls short —
 * both sides when the span straddles the window — before the scroll runs.
 */
export function growRangeToCover(range: GanttRange, zoom: GanttZoom, fromIso: string, toIso: string): GanttRange {
  const first = fromIso < toIso ? fromIso : toIso;
  const last = fromIso < toIso ? toIso : fromIso;
  let next = range;
  while (addDays(next.startIso, next.dayCount - 1) < last) next = extendRange(next, "right", zoom);
  while (next.startIso > first) next = extendRange(next, "left", zoom);
  return next;
}

/** Pixels from the window's first column to a given day. */
export function dayOffset(iso: string, range: GanttRange, zoom: GanttZoom): number {
  return diffDays(iso, range.startIso) * DAY_WIDTHS[zoom];
}

/** Full pixel width of the rendered window. */
export function timelineWidth(range: GanttRange, zoom: GanttZoom): number {
  return range.dayCount * DAY_WIDTHS[zoom];
}

/** Every day of the window, with its tint and week-boundary flag. */
export function dayColumns(range: GanttRange, weekStart: number): GanttDay[] {
  return Array.from({ length: range.dayCount }, (_, index) => {
    const iso = addDays(range.startIso, index);
    const weekday = weekdayOf(iso);
    return { iso, index, weekday, weekend: weekday === 0 || weekday === 6, startsWeek: weekday === weekStart };
  });
}

/** First day of the month after `iso`'s month. */
function nextMonthStart(iso: string): string {
  const { year, month0 } = dayTriple(iso);
  const next = new Date(Date.UTC(year, month0 + 1, 1));
  return toIsoDay(next.getUTCFullYear(), next.getUTCMonth(), next.getUTCDate());
}

/** First day of the quarter after `iso`'s quarter. */
function nextQuarterStart(iso: string): string {
  const { year, month0 } = dayTriple(iso);
  const quarter = Math.floor(month0 / 3);
  const next = new Date(Date.UTC(year, quarter * 3 + 3, 1));
  return toIsoDay(next.getUTCFullYear(), next.getUTCMonth(), next.getUTCDate());
}

/**
 * Consecutive calendar spans (months or quarters) covering the window.
 * `boundaryOf` returns the first day after the span that starts at `iso`,
 * so a span cut by either edge keeps its true extent.
 */
function calendarSegments(
  range: GanttRange,
  boundaryOf: (iso: string) => string,
  keyOf: (iso: string) => { key: string; label: string }
): GanttSegment[] {
  const segments: GanttSegment[] = [];
  for (let index = 0; index < range.dayCount; ) {
    const iso = addDays(range.startIso, index);
    const days = Math.min(diffDays(boundaryOf(iso), iso), range.dayCount - index);
    const { key, label } = keyOf(iso);
    // Consecutive spans always differ in key (each boundary advances a
    // month/quarter), so every step starts a cell.
    segments.push({ key, label, startIndex: index, days });
    index += days;
  }
  return segments;
}

/** Calendar months covering the window, cut at both edges. */
export function monthSegments(range: GanttRange): GanttSegment[] {
  return calendarSegments(range, nextMonthStart, (iso) => {
    const { year, month0 } = dayTriple(iso);
    return { key: `m-${year}-${month0}`, label: MONTH_ABBREVS[month0] as string };
  });
}

/** Calendar quarters covering the window, cut at both edges. */
export function quarterSegments(range: GanttRange): GanttSegment[] {
  return calendarSegments(range, nextQuarterStart, (iso) => {
    const { year, month0 } = dayTriple(iso);
    const quarter = Math.floor(month0 / 3);
    return { key: `q-${year}-${quarter}`, label: `Q${quarter + 1} ${year}` };
  });
}

/** Consecutive weeks covering the window, cut at its edges. */
export function weekSegments(range: GanttRange): GanttSegment[] {
  const segments: GanttSegment[] = [];
  for (let index = 0; index < range.dayCount; ) {
    const iso = addDays(range.startIso, index);
    const boundary = addDays(iso, 7 - weekdayOf(iso));
    const days = Math.min(diffDays(boundary, iso), range.dayCount - index);
    segments.push({
      key: `w-${iso}`,
      label: `${dayTriple(iso).day} - ${dayTriple(addDays(iso, days - 1)).day}`,
      startIndex: index,
      days,
    });
    index += days;
  }
  return segments;
}

/** One day per cell for the week zoom's fine tier. */
export function daySegments(range: GanttRange): GanttSegment[] {
  return Array.from({ length: range.dayCount }, (_, index) => {
    const iso = addDays(range.startIso, index);
    return {
      key: `d-${iso}`,
      label: `${WEEKDAY_SHORT[weekdayOf(iso)] as string} ${dayTriple(iso).day}`,
      startIndex: index,
      days: 1,
    };
  });
}

/** Two header tiers for one zoom: the coarse row above the fine row. */
export function headerTiers(zoom: GanttZoom, range: GanttRange): { coarse: GanttSegment[]; fine: GanttSegment[] } {
  switch (zoom) {
    case "week":
      return { coarse: monthSegments(range), fine: daySegments(range) };
    case "month":
      return { coarse: monthSegments(range), fine: weekSegments(range) };
    case "quarter":
      return { coarse: quarterSegments(range), fine: monthSegments(range) };
  }
}

/**
 * The header segment that carries today (ISS-046). Week and quarter mark
 * the day and month respectively; month marks the week containing today,
 * which stays highlighted on the week's last day too (the intended
 * behavior; the old app's NEWFRONT-163 gap is not reproduced).
 */
export function todaySegment(zoom: GanttZoom, range: GanttRange, todayIso: string): GanttSegment | null {
  const dayIndex = diffDays(todayIso, range.startIso);
  if (dayIndex < 0 || dayIndex >= range.dayCount) return null;
  switch (zoom) {
    case "week":
      return segmentAt(daySegments(range), dayIndex);
    case "month":
      return segmentAt(weekSegments(range), dayIndex);
    case "quarter":
      return segmentAt(monthSegments(range), dayIndex);
  }
}

/**
 * Today's fine-tier segment for the chart. A rendered window always opens
 * around today and only grows, so today is inside it; if a caller ever
 * hands in a window that excludes today, the first rendered segment
 * stands in so the chart stays total instead of null-checking everywhere.
 */
export function windowTodaySegment(zoom: GanttZoom, range: GanttRange, todayIso: string): GanttSegment {
  const fine = headerTiers(zoom, range).fine;
  return todaySegment(zoom, range, todayIso) ?? (fine[0] as GanttSegment);
}

/** Which segment contains a day index (null when outside). */
export function segmentAt(segments: readonly GanttSegment[], dayIndex: number): GanttSegment | null {
  return (
    segments.find((segment) => dayIndex >= segment.startIndex && dayIndex < segment.startIndex + segment.days) ?? null
  );
}

/**
 * Scroll offset that puts today a little right of the viewport's middle
 * (ISS-046) — the old screen lands it off-center, never centered.
 */
export function centeredScroll(todayIso: string, range: GanttRange, zoom: GanttZoom, viewportWidth: number): number {
  const width = DAY_WIDTHS[zoom];
  const todayCenter = dayOffset(todayIso, range, zoom) + width / 2;
  const target = todayCenter - viewportWidth * 0.6;
  const max = Math.max(0, timelineWidth(range, zoom) - viewportWidth);
  return Math.min(max, Math.max(0, target));
}

/** The edge a scroll offset has reached, if any (ISS-048). */
export function edgeAt(scrollLeft: number, scrollWidth: number, viewportWidth: number): "left" | "right" | null {
  if (scrollLeft <= EDGE_TRIGGER_PX) return "left";
  if (scrollLeft + viewportWidth >= scrollWidth - EDGE_TRIGGER_PX) return "right";
  return null;
}
