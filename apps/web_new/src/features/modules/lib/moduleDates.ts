// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module date readings (parity/specs/modules.md §4.1, §3.5, §3.6). Module
// start and target dates are *calendar days* (`yyyy-MM-dd`), not instants:
// the day a project picks is the day it shows, in the project's timezone,
// whoever is looking. So display never shifts a day across zones; it only
// states the project zone when the reader's browser zone differs from the
// project's, because a bare day with no zone is otherwise ambiguous. The
// rows, cards, timeline and detail all render through this module so they
// agree on the text, the range shape and the reader-zone note.

/** A calendar day parsed from a `yyyy-MM-dd` value; month is 1-based. */
export interface ModuleZoneDay {
  year: number;
  month: number;
  day: number;
}

/** A module's two-sided date range; either side may be unset. */
export interface ModuleDateRange {
  start: string | null;
  target: string | null;
}

const DAY_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;
const MONTH_SHORT = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/**
 * Parse a calendar day, or null for a missing or malformed value. A day
 * the calendar rejects (month 13, day 32) is malformed too: the row shows
 * no date rather than a shifted one.
 */
export function parseModuleDay(value: string | null | undefined): ModuleZoneDay | null {
  if (value === null || value === undefined) return null;
  const match = DAY_PATTERN.exec(value.trim());
  if (match === null) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (month < 1 || month > 12 || day < 1) return null;
  const probe = new Date(Date.UTC(year, month - 1, day));
  if (probe.getUTCFullYear() !== year || probe.getUTCMonth() !== month - 1 || probe.getUTCDate() !== day) {
    return null;
  }
  return { year, month, day };
}

/** `Oct 15, 2026`: the one-day form used by a single-sided range. */
export function formatModuleDay(day: ModuleZoneDay): string {
  return `${MONTH_SHORT[day.month - 1]} ${String(day.day).padStart(2, "0")}, ${day.year}`;
}

/**
 * The range text. A full range reads `Oct 15 - Oct 22, 2026` when it stays
 * inside one month and year, `Oct 30 - Nov 04, 2026` across months, and
 * repeats both years when it crosses one; a single side reads as one day;
 * no sides read as null so the caller can render its placeholder.
 */
export function formatModuleRange(range: ModuleDateRange): string | null {
  const start = parseModuleDay(range.start);
  const target = parseModuleDay(range.target);
  if (start !== null && target !== null) {
    const startText = `${MONTH_SHORT[start.month - 1]} ${String(start.day).padStart(2, "0")}`;
    const targetText = `${MONTH_SHORT[target.month - 1]} ${String(target.day).padStart(2, "0")}`;
    if (start.year === target.year && start.month === target.month) {
      return `${startText} - ${String(target.day).padStart(2, "0")}, ${start.year}`;
    }
    if (start.year === target.year) return `${startText} - ${targetText}, ${start.year}`;
    return `${startText}, ${start.year} - ${targetText}, ${target.year}`;
  }
  if (start !== null) return formatModuleDay(start);
  if (target !== null) return formatModuleDay(target);
  return null;
}

/**
 * Inclusive day count of a full range (`Oct 1` to `Oct 3` is three days),
 * or null when either side is missing. Used by the card and the timeline,
 * never as a stored value.
 */
export function moduleRangeDurationDays(range: ModuleDateRange): number | null {
  const start = parseModuleDay(range.start);
  const target = parseModuleDay(range.target);
  if (start === null || target === null) return null;
  const startMs = Date.UTC(start.year, start.month - 1, start.day);
  const targetMs = Date.UTC(target.year, target.month - 1, target.day);
  return Math.round((targetMs - startMs) / 86_400_000) + 1;
}

/**
 * The day text for one row or card side: the formatted day, or the
 * product's placeholder when that side is empty. The placeholders are the
 * pinned row labels, so the same function serves both rows and cards.
 */
export function moduleDateText(value: string | null, placeholder: string): string {
  const day = parseModuleDay(value);
  return day === null ? placeholder : formatModuleDay(day);
}

/** The reader's browser zone, or undefined when the engine cannot name it. */
function browserZone(): string | undefined {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    return undefined;
  }
}

/**
 * Whether the reader's zone differs from the project's zone and both are
 * known. Only then does a module date need the project-zone note: two
 * unknown zones are never claimed to differ, and a reader in the project's
 * own zone reads a plain day. The comparison is by zone name; a mismatch
 * between an alias and its target (`America/Los_Angeles` versus
 * `US/Pacific`) is harmless — the note is informational.
 */
export function userZoneDiffers(projectTimeZone: string | null | undefined, viewerTimeZone?: string): boolean {
  if (projectTimeZone === null || projectTimeZone === undefined || projectTimeZone.trim() === "") return false;
  const viewer = viewerTimeZone ?? browserZone();
  if (viewer === undefined || viewer === "") return false;
  return viewer !== projectTimeZone;
}
