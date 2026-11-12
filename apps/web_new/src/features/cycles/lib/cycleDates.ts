// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-timezone date readings (parity/specs/cycles.md §17, CYC-044).
// Cycle dates are stored as instants and displayed in the project's
// timezone; this module is the only place the area converts calendar
// days to instants and back, so the list, hero, dialog, date check and
// sidebar agree on "today" and "covers today". An unknown zone degrades
// to system time with the offset chip suppressed, never a throw.

/** A calendar day in one timezone; month is 1-based. */
export interface ZoneDay {
  year: number;
  month: number;
  day: number;
}

const MONTH_SHORT = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/**
 * The zone the formatter accepts, else undefined for system time. An
 * unknown project zone is a data fault the UI degrades past: dates
 * still render, in system time, with no offset chip.
 */
function resolveZoneName(timeZone: string | null | undefined): string | undefined {
  if (timeZone === null || timeZone === undefined || timeZone.trim() === "") return undefined;
  try {
    new Intl.DateTimeFormat("en-US", { timeZone });
    return timeZone;
  } catch {
    return undefined;
  }
}

/**
 * The zone's UTC offset in minutes at one instant (DST-aware), or null
 * for an unknown zone. Renders the instant in the zone, re-reads the
 * parts as UTC, and diffs: no offset table to rot.
 */
export function zoneOffsetMinutes(timeZone: string, at: Date): number | null {
  try {
    const parts = new Intl.DateTimeFormat("en-US", {
      timeZone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    })
      .formatToParts(at)
      .reduce<Record<string, string>>((acc, part) => ({ ...acc, [part.type]: part.value }), {});
    const asUtc = Date.UTC(
      Number(parts["year"]),
      Number(parts["month"]) - 1,
      Number(parts["day"]),
      Number(parts["hour"]) % 24,
      Number(parts["minute"]),
      Number(parts["second"])
    );
    return Math.round((asUtc - at.getTime()) / 60_000);
  } catch {
    return null;
  }
}

/**
 * The offset chip text (`UTC -04:00`, `UTC +05:30`), or null when the
 * offset is zero: the suppression rule is the numeric offset alone,
 * never the zone label (a zero-offset non-UTC zone shows no chip).
 */
export function formatOffsetChip(offsetMinutes: number): string | null {
  if (offsetMinutes === 0) return null;
  const sign = offsetMinutes < 0 ? "-" : "+";
  const abs = Math.abs(offsetMinutes);
  const hours = String(Math.floor(abs / 60)).padStart(2, "0");
  const minutes = String(abs % 60).padStart(2, "0");
  return `UTC ${sign}${hours}:${minutes}`;
}

/**
 * The chip for one zone at one instant: null for an unknown zone or a
 * zero offset. Callers anchor the instant at the cycle's start date so
 * a DST-spanning range carries one stable chip.
 */
export function projectOffsetChip(timeZone: string, at: Date): string | null {
  const offset = zoneOffsetMinutes(timeZone, at);
  if (offset === null) return null;
  return formatOffsetChip(offset);
}

/** One instant's calendar day in a zone, or null for a broken instant. */
export function instantZoneDay(instant: string, timeZone: string | null | undefined): ZoneDay | null {
  const at = new Date(instant);
  if (Number.isNaN(at.getTime())) return null;
  const zone = resolveZoneName(timeZone);
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

/** `Oct 15, 2026`: the one-sided span and the per-end shape. */
export function formatZoneDay(day: ZoneDay): string {
  const month = MONTH_SHORT[day.month - 1];
  return `${month} ${String(day.day).padStart(2, "0")}, ${day.year}`;
}

/** `yyyy-MM-dd` of one instant in a zone, for date-check requests (§23). */
export function instantToProjectDate(instant: string, timeZone: string | null | undefined): string | null {
  const day = instantZoneDay(instant, timeZone);
  if (day === null) return null;
  return `${day.year}-${String(day.month).padStart(2, "0")}-${String(day.day).padStart(2, "0")}`;
}

/** Today's date-only reading in a zone (`now` is injectable for tests). */
export function projectToday(timeZone: string | null | undefined, now: Date = new Date()): string {
  const zone = resolveZoneName(timeZone);
  const parts = new Intl.DateTimeFormat("en-US", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    ...(zone === undefined ? {} : { timeZone: zone }),
  })
    .formatToParts(now)
    .reduce<Record<string, string>>((acc, part) => ({ ...acc, [part.type]: part.value }), {});
  return `${parts["year"]}-${parts["month"]}-${parts["day"]}`;
}

/**
 * Whether the dated window covers today in the project's zone
 * (date-only compare; a missing end stays open). A cycle with neither
 * date has no window, so it never covers today.
 */
export function cycleCoversToday(
  startDate: string | null,
  endDate: string | null,
  timeZone: string | null | undefined,
  now: Date = new Date()
): boolean {
  if (startDate === null && endDate === null) return false;
  const today = projectToday(timeZone, now);
  const start = startDate === null ? null : instantToProjectDate(startDate, timeZone);
  const end = endDate === null ? null : instantToProjectDate(endDate, timeZone);
  if (start !== null && start > today) return false;
  if (end !== null && end < today) return false;
  return true;
}

/** One compact range in a zone, or null when the cycle has no dates. */
export function formatCycleSpanText(
  startDate: string | null,
  endDate: string | null,
  timeZone: string | null | undefined
): string | null {
  const start = startDate === null ? null : instantZoneDay(startDate, timeZone);
  const end = endDate === null ? null : instantZoneDay(endDate, timeZone);
  if (start !== null && end !== null) {
    if (start.year === end.year && start.month === end.month) {
      return `${MONTH_SHORT[start.month - 1]} ${String(start.day).padStart(2, "0")} - ${String(end.day).padStart(2, "0")}, ${start.year}`;
    }
    if (start.year === end.year) {
      return `${MONTH_SHORT[start.month - 1]} ${String(start.day).padStart(2, "0")} - ${MONTH_SHORT[end.month - 1]} ${String(end.day).padStart(2, "0")}, ${start.year}`;
    }
    return `${formatZoneDay(start)} - ${formatZoneDay(end)}`;
  }
  if (start !== null) return formatZoneDay(start);
  if (end !== null) return formatZoneDay(end);
  return null;
}

export interface CycleSpanModel {
  /** The compact range, or null when the cycle has no dates. */
  text: string | null;
  /** The offset chip, or null for a zero/unknown offset. */
  chip: string | null;
  /**
   * The tooltip, or null when no chip shows or the viewer shares the
   * project zone: the zone comparison gates the tooltip only, never
   * the chip.
   */
  tooltip: string | null;
}

/**
 * Everything one date span renders: the project-zone range, the offset
 * chip anchored at the start date, and the viewer-zone tooltip. The
 * viewer zone defaults to the browser's and is injectable for tests.
 */
export function cycleSpanModel(
  startDate: string | null,
  endDate: string | null,
  projectTimeZone: string,
  viewerTimeZone?: string | undefined
): CycleSpanModel {
  const text = formatCycleSpanText(startDate, endDate, projectTimeZone);
  const anchor = startDate ?? endDate;
  const chip =
    anchor === null || Number.isNaN(new Date(anchor).getTime())
      ? null
      : projectOffsetChip(projectTimeZone, new Date(anchor));
  const viewer = viewerTimeZone === undefined ? Intl.DateTimeFormat().resolvedOptions().timeZone : viewerTimeZone;
  const zonesDiffer =
    chip !== null &&
    viewer !== undefined &&
    resolveZoneName(viewer) !== undefined &&
    resolveZoneName(projectTimeZone) !== undefined &&
    viewer !== projectTimeZone;
  if (text === null || chip === null || !zonesDiffer) {
    return { text, chip, tooltip: null };
  }
  const viewerText = formatCycleSpanText(startDate, endDate, viewer);
  return {
    text,
    chip,
    tooltip: `Dates shown in ${projectTimeZone} (${chip}). In your timezone: ${viewerText}.`,
  };
}
