// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// ISO-day helpers for parity calendar scenarios (NEWFRONT-335). The old
// app hides weekend columns unless the user enabled them, so scenarios
// that must land on a visible day tile pick weekday dates through these
// pure functions instead of raw day offsets.
const DAY_MS = 86400000;

/** Parse a `yyyy-MM-dd` day as UTC noon (offset-free either side). */
function parseDayISO(iso: string): number {
  return new Date(`${iso}T12:00:00Z`).getTime();
}

/** Format epoch millis back to `yyyy-MM-dd` (UTC, offset-free). */
function formatDayISO(time: number): string {
  return new Date(time).toISOString().slice(0, 10);
}

/** `baseISO` shifted by `days` days, still `yyyy-MM-dd`. */
export function isoDayOffset(baseISO: string, days: number): string {
  return formatDayISO(parseDayISO(baseISO) + days * DAY_MS);
}

/** True when the ISO day is a Saturday or Sunday. */
export function isWeekendISO(iso: string): boolean {
  const weekday = new Date(`${iso}T12:00:00Z`).getUTCDay();
  return weekday === 0 || weekday === 6;
}

/**
 * A future weekday for calendar scenarios: the first `candidatesOut`
 * offset from `todayISO` that stays in-month and off weekends. When
 * month's end defeats every candidate, the nearest upcoming weekday past
 * `fallbackOut` (the month grid's spillover week still shows it).
 */
export function pickFutureWeekdayISO(todayISO: string, candidatesOut: number[], fallbackOut: number): string {
  const thisMonth = todayISO.slice(0, 7);
  for (const days of candidatesOut) {
    const iso = isoDayOffset(todayISO, days);
    if (iso.slice(0, 7) === thisMonth && !isWeekendISO(iso)) return iso;
  }
  let iso = isoDayOffset(todayISO, fallbackOut);
  while (isWeekendISO(iso)) iso = isoDayOffset(iso, 1);
  return iso;
}

/**
 * A recent past weekday for overdue scenarios: `daysBack` days before
 * `todayISO`, pushed back past any weekend onto Friday.
 */
export function pickPastWeekdayISO(todayISO: string, daysBack: number): string {
  let iso = isoDayOffset(todayISO, -daysBack);
  while (isWeekendISO(iso)) iso = isoDayOffset(iso, -1);
  return iso;
}

/** A local `yyyy-MM-dd` for the browser's own today. */
export function localTodayISO(now: Date = new Date()): string {
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${month}-${day}`;
}

/**
 * The calendar tile's accessible name for an ISO day, in a fixed
 * month-name vocabulary (the grid labels each tile "Month D, YYYY"
 * regardless of locale).
 */
export function calendarDayLabel(iso: string): string {
  const names = [
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
  const [year, month, day] = iso.split("-").map((part) => Number(part));
  return `${names[(month ?? 1) - 1]} ${day}, ${year}`;
}
