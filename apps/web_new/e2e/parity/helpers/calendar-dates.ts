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
 * Today's `yyyy-MM-dd` in a project timezone (`now` is injectable for
 * tests). A missing or unknown zone degrades to the runner's local day,
 * mirroring the app's system-time fallback.
 */
export function zonedTodayISO(timeZone: string | null, now: Date = new Date()): string {
  if (timeZone !== null) {
    try {
      const rendered = new Intl.DateTimeFormat("en-US", {
        timeZone,
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
      })
        .formatToParts(now)
        .reduce<Record<string, string>>((acc, part) => ({ ...acc, [part.type]: part.value }), {});
      return `${rendered["year"]}-${rendered["month"]}-${rendered["day"]}`;
    } catch {
      // Fall through to the runner-local day below.
    }
  }
  return localTodayISO(now);
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

/**
 * A period that still has future slots today, plus its current half-hour,
 * which counts as elapsed. The snooze dialog switches to PM once AM is
 * empty, so asserting an AM slot after that would read an empty grid; null
 * means both grids are exhausted (after 23:30) and the scenario cannot
 * prove an elapsed slot on today's grid.
 */
export function elapsedHalfHour(now: Date): { meridiem: "AM" | "PM"; slot: string } | null {
  const nowMinutes = now.getHours() * 60 + now.getMinutes();
  const meridiem = nowMinutes < 11 * 60 + 30 ? "AM" : nowMinutes < 23 * 60 + 30 ? "PM" : null;
  if (meridiem === null) return null;
  const minutes = Math.floor(nowMinutes / 30) * 30;
  const hour = Math.floor(minutes / 60);
  const twelve = hour % 12 === 0 ? 12 : hour % 12;
  return {
    meridiem,
    slot: `${String(twelve).padStart(2, "0")}:${minutes % 60 === 0 ? "00" : "30"}`,
  };
}
