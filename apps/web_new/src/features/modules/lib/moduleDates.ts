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

/** The row/card calendar selection: one picked day, or an open range end. */
export interface ModuleRangePick {
  start: string | null;
  end: string | null;
}

/**
 * Next selection after a row/card calendar click (§4.1). An empty calendar
 * anchors the start; a lone start extends to the clicked day (or
 * re-anchors when the click lands before it); clicking the lone selected
 * day again clears both sides, which the control saves as empty dates.
 * A complete range re-anchors: the click becomes the new lone start and
 * the next click sets the end. Every click changes the selection, so
 * every pick saves — there is deliberately no silent click.
 */
export function moduleRangeClick(range: ModuleRangePick, day: string): ModuleRangePick {
  const { start, end } = range;
  if (start === null || start === "") return { start: day, end: null };
  if (end === null || end === "") {
    if (day === start) return { start: null, end: null };
    if (day < start) return { start: day, end: null };
    return { start, end: day };
  }
  return { start: day, end: null };
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
  if (start !== null && target !== null) return formatModuleDayRange(start, target);
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

// ---------------------------------------------------------------------
// Project-zone readings (parity/specs/modules.md §2.1, §3.4–§3.6).
//
// A module's start and target are calendar days and never shift zones;
// the rows and cards read them through the functions above. A *timestamp*
// (`created_at`, `updated_at`, a timeline anchor) is an instant, though,
// and the list states which zone it is read in: the project's. When the
// reader's browser sits in another zone the chips and notes below supply
// that equivalent, because an offset alone does not tell a reader what
// day the instant falls on for them. An unknown zone degrades past the
// note, never a throw.

/** A zone's UTC offset in minutes at one instant (DST-aware), or null. */
export function moduleZoneOffsetMinutes(timeZone: string | null | undefined, at: Date): number | null {
  if (timeZone === null || timeZone === undefined || timeZone.trim() === "") return null;
  if (Number.isNaN(at.getTime())) return null;
  try {
    const parts = new Intl.DateTimeFormat("en-US", {
      timeZone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hourCycle: "h23",
    })
      .formatToParts(at)
      .reduce<Record<string, string>>((acc, part) => ({ ...acc, [part.type]: part.value }), {});
    const asUtc = Date.UTC(
      Number(parts["year"]),
      Number(parts["month"]) - 1,
      Number(parts["day"]),
      Number(parts["hour"]),
      Number(parts["minute"]),
      Number(parts["second"])
    );
    return Math.round((asUtc - at.getTime()) / 60_000);
  } catch {
    return null;
  }
}

/**
 * `UTC -04:00` / `UTC +05:30`, or null at a zero offset: a reader in UTC
 * (or any zero-offset zone) needs no chip. Matches the pinned offset
 * shape the date tooltips use.
 */
export function formatModuleOffsetChip(offsetMinutes: number): string | null {
  if (!Number.isFinite(offsetMinutes) || offsetMinutes === 0) return null;
  const sign = offsetMinutes < 0 ? "-" : "+";
  const abs = Math.abs(Math.round(offsetMinutes));
  const hours = String(Math.floor(abs / 60)).padStart(2, "0");
  const minutes = String(abs % 60).padStart(2, "0");
  return `UTC ${sign}${hours}:${minutes}`;
}

/** The chip for one zone at one instant; null for an unknown zone or offset zero. */
export function moduleOffsetChip(timeZone: string | null | undefined, at: Date): string | null {
  const offset = moduleZoneOffsetMinutes(timeZone, at);
  return offset === null ? null : formatModuleOffsetChip(offset);
}

/** The zone the formatter accepts; an unknown name degrades to the engine zone. */
function usableZone(timeZone: string | null | undefined): string | undefined {
  if (timeZone === null || timeZone === undefined || timeZone.trim() === "") return undefined;
  try {
    new Intl.DateTimeFormat("en-US", { timeZone });
    return timeZone;
  } catch {
    return undefined;
  }
}

/**
 * The calendar day an instant falls on in a zone, or null for a broken
 * instant. An absent or unknown zone reads in the engine zone rather than
 * failing: the day still has to render, only the zone claim is dropped.
 */
export function moduleInstantDay(instant: string | Date, timeZone: string | null | undefined): ModuleZoneDay | null {
  const at = instant instanceof Date ? instant : new Date(instant);
  if (Number.isNaN(at.getTime())) return null;
  return dayInZone(at, timeZone);
}

/** A valid instant's calendar day in a zone; the zone falls back to the engine's. */
function dayInZone(at: Date, timeZone: string | null | undefined): ModuleZoneDay {
  const zone = usableZone(timeZone);
  const parts = new Intl.DateTimeFormat("en-US", {
    year: "numeric",
    month: "numeric",
    day: "numeric",
    ...(zone === undefined ? {} : { timeZone: zone }),
  })
    .formatToParts(at)
    .reduce<Record<string, string>>((acc, part) => ({ ...acc, [part.type]: part.value }), {});
  return { year: Number(parts["year"]), month: Number(parts["month"]), day: Number(parts["day"]) };
}

/**
 * The instant a calendar day is anchored to for offset readings: midday
 * in the project zone. Noon is never inside a DST gap, so the reading
 * stays stable; the offset is solved in two passes because the first
 * (UTC) offset can be an hour off across a transition.
 */
export function moduleDayAnchor(day: ModuleZoneDay, timeZone: string | null | undefined): Date | null {
  if (timeZone === null || timeZone === undefined || timeZone.trim() === "") return null;
  const guess = Date.UTC(day.year, day.month - 1, day.day, 12, 0, 0);
  const first = moduleZoneOffsetMinutes(timeZone, new Date(guess));
  if (first === null) return null;
  // The second reading settles a day whose own offset differs from the
  // guess's (a DST transition day). The zone resolved a moment ago; if the
  // engine refuses it now, the first reading stands.
  const settled = moduleZoneOffsetMinutes(timeZone, new Date(guess - first * 60_000));
  return new Date(guess - (settled ?? first) * 60_000);
}

/** The pinned heading a reader-zone note carries. */
export const MODULE_USER_ZONE_HEADING = "In your timezone";

/** One date reading: the project-zone text, its chip and the reader's equivalent. */
export interface ModuleZoneReading {
  /** The project-zone range text; null when both sides are empty. */
  text: string | null;
  /** `UTC -04:00`, or null for a zero or unknown offset. */
  chip: string | null;
  /** The same range in the viewer's zone; null when the zones agree or are unknown. */
  userText: string | null;
  /**
   * The full note for a tooltip, e.g.
   * `Dates shown in America/New_York (UTC -04:00). In your timezone (Asia/Tokyo): Oct 15 - 22, 2026.`
   * Null when no chip or no differing viewer zone.
   */
  note: string | null;
}

/**
 * Read a module date range for the project zone and, when the reader is
 * elsewhere, the same calendar days stated for the reader too. Module
 * dates are days, not instants: they never shift, so the note exists to
 * name the project zone and its offset — the equivalent repeats the
 * range. Both zones must resolve and the offset must be non-zero; equal
 * zones suppress the note.
 */
export function moduleRangeReading(
  range: ModuleDateRange,
  projectTimeZone: string | null | undefined,
  viewerTimeZone?: string
): ModuleZoneReading {
  const text = formatModuleRange(range);
  const anchorDay = parseModuleDay(range.start) ?? parseModuleDay(range.target);
  const anchor = anchorDay === null ? null : moduleDayAnchor(anchorDay, projectTimeZone);
  const chip = anchor === null ? null : moduleOffsetChip(projectTimeZone, anchor);
  const viewer = viewerTimeZone ?? browserZone();
  if (text === null || chip === null || anchor === null) return { text, chip, userText: null, note: null };
  if (!userZoneDiffers(projectTimeZone, viewer)) return { text, chip, userText: null, note: null };
  // A module range is calendar days, not instants: the project's day is
  // the viewer's day too, and only the zone name and offset are stated.
  // The equivalent repeats the range so the note reads as a sentence.
  const userText = text;
  return {
    text,
    chip,
    userText,
    note: `Dates shown in ${projectTimeZone} (${chip}). ${MODULE_USER_ZONE_HEADING} (${viewer}): ${userText}.`,
  };
}

/** One range from two already-parsed days, collapsing month and year. */
function formatModuleDayRange(start: ModuleZoneDay, target: ModuleZoneDay): string {
  const startText = `${MONTH_SHORT[start.month - 1]} ${String(start.day).padStart(2, "0")}`;
  const targetText = `${MONTH_SHORT[target.month - 1]} ${String(target.day).padStart(2, "0")}`;
  if (start.year === target.year && start.month === target.month) {
    return `${startText} - ${String(target.day).padStart(2, "0")}, ${start.year}`;
  }
  if (start.year === target.year) return `${startText} - ${targetText}, ${start.year}`;
  return `${startText}, ${start.year} - ${targetText}, ${target.year}`;
}

/**
 * One timestamp read in the project zone and, when it differs, on the
 * viewer's calendar too: the created/updated rows and the timeline's
 * "today" marker render through this. Unlike a module date range, an
 * instant really does fall on another day for a reader far enough away.
 */
export interface ModuleInstantReading {
  /** The instant's day in the project zone; null for a broken instant. */
  text: string | null;
  chip: string | null;
  /** The instant's day on the viewer's calendar; null when the zones agree. */
  userText: string | null;
  note: string | null;
}

export function moduleInstantReading(
  instant: string | null | undefined,
  projectTimeZone: string | null | undefined,
  viewerTimeZone?: string
): ModuleInstantReading {
  if (instant === null || instant === undefined) return { text: null, chip: null, userText: null, note: null };
  const at = new Date(instant);
  const day = moduleInstantDay(at, projectTimeZone);
  if (day === null) return { text: null, chip: null, userText: null, note: null };
  const chip = moduleOffsetChip(projectTimeZone, at);
  if (chip === null) return { text: formatModuleDay(day), chip, userText: null, note: null };
  const viewer = viewerTimeZone ?? browserZone();
  if (!userZoneDiffers(projectTimeZone, viewer)) {
    return { text: formatModuleDay(day), chip, userText: null, note: null };
  }
  const userText = formatModuleDay(dayInZone(at, viewer));
  return {
    text: formatModuleDay(day),
    chip,
    userText,
    note: `Shown in ${projectTimeZone} (${chip}). ${MODULE_USER_ZONE_HEADING} (${viewer}): ${userText}.`,
  };
}
