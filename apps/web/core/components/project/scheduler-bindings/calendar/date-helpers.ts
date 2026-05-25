/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Calendar-grid + label helpers for the scheduler calendar. Primitive date
 * math is delegated to ``date-fns`` (already a workspace dependency); only
 * the calendar-shape helpers and the locale-aware label formatters live here.
 */

import {
  addDays,
  addMonths as dfAddMonths,
  addWeeks as dfAddWeeks,
  endOfWeek as dfEndOfWeek,
  startOfMonth,
  startOfWeek,
} from "date-fns";

export type CalendarView = "month" | "week";

/** 6-week (42-day) Sunday-rooted grid covering the month of ``viewDate``. */
export function monthGridDays(viewDate: Date): Date[] {
  const start = startOfWeek(startOfMonth(viewDate), { weekStartsOn: 0 });
  return Array.from({ length: 42 }, (_, i) => addDays(start, i));
}

/** 7-day Sunday-rooted week containing ``viewDate``. */
export function weekGridDays(viewDate: Date): Date[] {
  const start = startOfWeek(viewDate, { weekStartsOn: 0 });
  return Array.from({ length: 7 }, (_, i) => addDays(start, i));
}

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
      fromIso: startOfWeek(viewDate, { weekStartsOn: 0 }).toISOString(),
      toIso: dfEndOfWeek(viewDate, { weekStartsOn: 0 }).toISOString(),
    };
  }
  const days = monthGridDays(viewDate);
  return {
    fromIso: days[0].toISOString(),
    toIso: new Date(days[days.length - 1].getTime() + 24 * 3600 * 1000 - 1).toISOString(),
  };
}

export const addMonths = dfAddMonths;
export const addWeeks = dfAddWeeks;

export { isSameDay, isToday } from "date-fns";
