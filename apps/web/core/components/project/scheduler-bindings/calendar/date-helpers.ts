/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Pure date helpers for the scheduler calendar. No React, no MobX — just
 * functions over Date.
 */

export type CalendarView = "month" | "week";

export const startOfMonth = (d: Date): Date => new Date(d.getFullYear(), d.getMonth(), 1);
export const endOfMonth = (d: Date): Date => new Date(d.getFullYear(), d.getMonth() + 1, 0, 23, 59, 59, 999);

/** Sunday-rooted start of the week containing ``d``. */
export const startOfWeek = (d: Date): Date => {
  const out = new Date(d);
  out.setHours(0, 0, 0, 0);
  out.setDate(out.getDate() - out.getDay());
  return out;
};

export const endOfWeek = (d: Date): Date => {
  const out = startOfWeek(d);
  out.setDate(out.getDate() + 6);
  out.setHours(23, 59, 59, 999);
  return out;
};

/**
 * For month view: returns the 6-week (42-day) grid covering the full month,
 * padded by days from the previous and following months so each row has 7
 * days. Always 42 entries — same as Google Calendar's month grid.
 */
export function monthGridDays(viewDate: Date): Date[] {
  const first = startOfMonth(viewDate);
  const start = startOfWeek(first);
  const days: Date[] = [];
  for (let i = 0; i < 42; i++) {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    days.push(d);
  }
  return days;
}

export function weekGridDays(viewDate: Date): Date[] {
  const start = startOfWeek(viewDate);
  const days: Date[] = [];
  for (let i = 0; i < 7; i++) {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    days.push(d);
  }
  return days;
}

export const isSameDay = (a: Date, b: Date): boolean =>
  a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

export const isToday = (d: Date): boolean => isSameDay(d, new Date());

export const formatTime = (d: Date): string =>
  new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(d);

export const formatMonthYear = (d: Date): string =>
  new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }).format(d);

export const formatWeekday = (d: Date): string => new Intl.DateTimeFormat(undefined, { weekday: "short" }).format(d);

export const formatDayHeader = (d: Date): string =>
  new Intl.DateTimeFormat(undefined, { weekday: "short", month: "short", day: "numeric" }).format(d);

/** Compute the [from, to] ISO bounds for the API given a view + anchor date. */
export function windowForView(view: CalendarView, viewDate: Date): { fromIso: string; toIso: string } {
  if (view === "week") {
    return {
      fromIso: startOfWeek(viewDate).toISOString(),
      toIso: endOfWeek(viewDate).toISOString(),
    };
  }
  // month view: include the visible grid (which spans into prev/next month).
  const days = monthGridDays(viewDate);
  return {
    fromIso: days[0].toISOString(),
    toIso: new Date(days[days.length - 1].getTime() + 24 * 3600 * 1000 - 1).toISOString(),
  };
}

export function addMonths(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth() + n, 1);
}

export function addWeeks(d: Date, n: number): Date {
  const out = new Date(d);
  out.setDate(out.getDate() + n * 7);
  return out;
}
