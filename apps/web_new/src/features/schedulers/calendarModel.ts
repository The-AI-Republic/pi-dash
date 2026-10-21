// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar geometry for parity/specs/agents.md §5 (AGT-015/016). One
// forward week runs Sunday to Saturday and a 24-hour column is 48px per
// hour; a month grid starts on the Sunday of the week holding its first
// day and ends on the Saturday of the week holding its last. Everything
// here is pure so the grids, the window maths and the drawer share one
// notion of "which day is this firing on".

import type { SchedulerOccurrence } from "@pidash/api-client";

const DAY_MS = 24 * 60 * 60 * 1000;

/** Height of one hour lane in the week grid. */
export const HOUR_HEIGHT = 48;
/** Full day lane: 24 hours. */
export const DAY_HEIGHT = 24 * HOUR_HEIGHT;
/** Blocks a month cell shows before it folds the rest behind a control. */
export const MONTH_BLOCKS_PER_DAY = 4;

function atMidnight(date: Date): Date {
  const copy = new Date(date.getTime());
  copy.setHours(0, 0, 0, 0);
  return copy;
}

export function addDays(date: Date, days: number): Date {
  const copy = new Date(date.getTime());
  copy.setDate(copy.getDate() + days);
  return copy;
}

/** Sunday 00:00 local of the week holding `date`. */
export function startOfWeek(date: Date): Date {
  const midnight = atMidnight(date);
  return addDays(midnight, -midnight.getDay());
}

/** First day of the month, 00:00 local. */
export function startOfMonth(date: Date): Date {
  const midnight = atMidnight(date);
  return new Date(midnight.getFullYear(), midnight.getMonth(), 1);
}

/** Next month's first day, 00:00 local. */
export function nextMonth(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth() + 1, 1);
}

/** Previous month's first day, 00:00 local. */
export function previousMonth(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth() - 1, 1);
}

/** The visible Sunday-through-Saturday week, inclusive of both ends. */
export function weekWindow(anchor: Date): { from: string; to: string } {
  const start = startOfWeek(anchor);
  const end = new Date(addDays(start, 7).getTime() - 1);
  return { from: start.toISOString(), to: end.toISOString() };
}

/** The visible grid for a month: whole weeks only, both ends inclusive. */
export function monthWindow(anchor: Date): { from: string; to: string } {
  const first = startOfMonth(anchor);
  const last = new Date(first.getFullYear(), first.getMonth() + 1, 0);
  const start = startOfWeek(first);
  const end = new Date(addDays(startOfWeek(last), 7).getTime() - 1);
  return { from: start.toISOString(), to: end.toISOString() };
}

/** The seven days of the week holding `anchor`, oldest first. */
export function weekDays(anchor: Date): Date[] {
  const start = startOfWeek(anchor);
  return Array.from({ length: 7 }, (_, index) => addDays(start, index));
}

export interface MonthCell {
  date: Date;
  /** False for the leading/trailing days that belong to a neighbouring month. */
  inMonth: boolean;
}

/** Every cell in the month's grid, whole weeks, oldest first. */
export function monthCells(anchor: Date): MonthCell[] {
  const first = startOfMonth(anchor);
  const end = monthWindow(anchor).to;
  const start = startOfWeek(first);
  const cells: MonthCell[] = [];
  for (let cursor = new Date(start); cursor.getTime() <= new Date(end).getTime(); cursor = addDays(cursor, 1)) {
    cells.push({ date: new Date(cursor), inMonth: cursor.getMonth() === first.getMonth() });
  }
  return cells;
}

/** Local calendar day key ("2026-10-27") used to bucket firings. */
export function dayKey(date: Date): string {
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

export function isSameDay(left: Date, right: Date): boolean {
  return dayKey(left) === dayKey(right);
}

/** Firings bucketed by their local calendar day, each bucket time-ordered. */
export function occurrencesByDay(occurrences: readonly SchedulerOccurrence[]): Map<string, SchedulerOccurrence[]> {
  const buckets = new Map<string, SchedulerOccurrence[]>();
  for (const occurrence of occurrences) {
    const key = dayKey(new Date(occurrence.dtstart));
    const bucket = buckets.get(key);
    if (bucket === undefined) buckets.set(key, [occurrence]);
    else bucket.push(occurrence);
  }
  for (const bucket of buckets.values()) {
    bucket.sort((left, right) => new Date(left.dtstart).getTime() - new Date(right.dtstart).getTime());
  }
  return buckets;
}

/** Pixels from the top of the day lane to the current minute. */
export function timeLineTop(now: Date): number {
  return ((now.getHours() * 60 + now.getMinutes()) / 60) * HOUR_HEIGHT;
}

/** Pixels from the top of the day lane to a firing's start. */
export function occurrenceTop(dtstart: string): number {
  return timeLineTop(new Date(dtstart));
}

const monthYearFormat = new Intl.DateTimeFormat("en-US", { month: "long", year: "numeric" });
// Weekday label built from parts so every runtime reads "Sun 18" the same
// way regardless of its default day/month order.
const weekdayNameFormat = new Intl.DateTimeFormat("en-US", { weekday: "short" });
const blockTimeFormat = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit" });

export function formatMonthYear(date: Date): string {
  return monthYearFormat.format(date);
}

/** Header label for a week column, e.g. "Sun 25". */
export function formatDayLabel(date: Date): string {
  return `${weekdayNameFormat.format(date)} ${date.getDate()}`;
}

/** Block label, e.g. "9:00 AM". */
export function formatBlockTime(date: Date): string {
  return blockTimeFormat.format(date);
}

export interface MonthDayPlan {
  /** Firings drawn as their own blocks, in time order. */
  blocks: SchedulerOccurrence[];
  /** Firings folded behind the overflow control. */
  folded: SchedulerOccurrence[];
  /** Overflow control text ("+ 20 more"), null when nothing folds away. */
  more: string | null;
}

/** How one month cell packs its firings. */
export function planMonthDay(occurrences: readonly SchedulerOccurrence[]): MonthDayPlan {
  if (occurrences.length <= MONTH_BLOCKS_PER_DAY) return { blocks: [...occurrences], folded: [], more: null };
  return {
    blocks: occurrences.slice(0, MONTH_BLOCKS_PER_DAY),
    folded: occurrences.slice(MONTH_BLOCKS_PER_DAY),
    more: `+ ${occurrences.length - MONTH_BLOCKS_PER_DAY} more`,
  };
}

export interface MonthRollup {
  name: string;
  count: number;
  label: string;
  /** The scheduler's first firing of the day, opened by the rollup line. */
  first: SchedulerOccurrence;
}

export interface WeekLane {
  occurrence: SchedulerOccurrence;
  /** Zero-based column within a group of firings sharing one start instant. */
  lane: number;
  /** How many columns that group has. */
  lanes: number;
}

/**
 * Side-by-side columns for firings that start at the same instant. Blocks
 * with distinct starts keep one full-width column each, so a lane only
 * appears when two schedules would otherwise draw exactly on top of one
 * another. Order follows the input order (the bucket is time-sorted).
 */
export function assignWeekLanes(occurrences: readonly SchedulerOccurrence[]): WeekLane[] {
  const groups = new Map<string, SchedulerOccurrence[]>();
  for (const occurrence of occurrences) {
    const group = groups.get(occurrence.dtstart);
    if (group === undefined) groups.set(occurrence.dtstart, [occurrence]);
    else group.push(occurrence);
  }
  const indexOf = new Map<SchedulerOccurrence, number>(occurrences.map((occurrence, index) => [occurrence, index]));
  return [...groups.values()]
    .flatMap((group) => group.map((occurrence, lane) => ({ occurrence, lane, lanes: group.length })))
    .sort((left, right) => (indexOf.get(left.occurrence) as number) - (indexOf.get(right.occurrence) as number));
}

/**
 * Density form for a truncated window: firings fold into one "N× Name"
 * line per scheduler so a month of minutely beats stays readable.
 */
export function rollupDay(occurrences: readonly SchedulerOccurrence[]): MonthRollup[] {
  const groups = new Map<string, { count: number; first: SchedulerOccurrence }>();
  for (const occurrence of occurrences) {
    const group = groups.get(occurrence.scheduler_name);
    if (group === undefined) groups.set(occurrence.scheduler_name, { count: 1, first: occurrence });
    else group.count += 1;
  }
  return [...groups.entries()].map(([name, { count, first }]) => ({
    name,
    count,
    label: `${count}× ${name}`,
    first,
  }));
}

/** Start of the day after `date`, at local midnight. */
export function startOfNextDay(date: Date): Date {
  return atMidnight(addDays(date, 1));
}

/** Whole days between two instants, rounded down; negative when reversed. */
export function daysBetween(from: Date, to: Date): number {
  return Math.floor((atMidnight(to).getTime() - atMidnight(from).getTime()) / DAY_MS);
}
