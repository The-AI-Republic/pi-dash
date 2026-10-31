// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Date value grammar (parity/specs/issues.md §9, ISS-089–092). Pure: the
// stored date-value form the editor and the list query both read — preset
// tokens, custom entries with a direction, and the shape a stored value has.
// It sits apart from the editor's preset labels and row helpers because the
// issues list query imports it on the eager path, while those load with the
// filter row (NEWFRONT-871, size gate).

/** The preset tokens both date dimensions offer, in display order. */
export const DATE_PRESET_VALUES: readonly string[] = [
  "1_weeks;after;fromnow",
  "2_weeks;after;fromnow",
  "1_months;after;fromnow",
  "2_months;after;fromnow",
];

const PRESET_VALUES = new Set(DATE_PRESET_VALUES);

export function isDatePreset(entry: string): boolean {
  return PRESET_VALUES.has(entry);
}

/** A calendar day in the ISO form the calendar and the server share. */
const ISO_DAY = /^\d{4}-\d{2}-\d{2}$/;

/**
 * Custom entries carry their direction, so they are the ones with a
 * hyphenated day; presets carry none. This is also how a stored value
 * tells the two apart.
 */
export function isCustomDateEntry(entry: string): boolean {
  const [day] = entry.split(";");
  return day !== undefined && ISO_DAY.test(day) && !isDatePreset(entry);
}

/** Applied entries of a stored date value, in stored order. */
export function dateEntries(value: unknown): string[] {
  const raw = typeof value === "string" ? value.split(",") : Array.isArray(value) ? value : [];
  return raw
    .filter((entry): entry is string => typeof entry === "string")
    .map((entry) => entry.trim())
    .filter((entry) => entry !== "");
}

/** Presets and custom entries of a stored value, in stored order. */
export function dateValueParts(value: unknown): { presets: string[]; custom: string[] } {
  const parts = { presets: [] as string[], custom: [] as string[] };
  for (const entry of dateEntries(value)) {
    if (isDatePreset(entry)) parts.presets.push(entry);
    else if (isCustomDateEntry(entry)) parts.custom.push(entry);
  }
  return parts;
}

/**
 * The shape of a stored custom half: one exact day, one open bound in a
 * direction, or two ends. Nothing custom reads as null. The old custom
 * dialog offered After, Before and Range, so a stored value tells the
 * editor which mode to reopen in.
 */
export function customDateMode(value: unknown): CustomDateMode | null {
  const entries = dateValueParts(value).custom;
  if (entries.length === 0) return null;
  if (entries.length > 1) return "range";
  const [, direction] = entries[0]!.split(";");
  if (direction === "after") return "after";
  if (direction === "before") return "before";
  return "exact";
}

/** The custom entries a date value stores, directions included. */
export function customDateEntries(value: unknown): string[] {
  return dateValueParts(value).custom;
}

/** Which shape the custom half of a date value holds. */
export type CustomDateMode = "exact" | "after" | "before" | "range";

/** Both custom ends, read back for the calendar. */
export function customRange(value: unknown): { start: string | null; end: string | null } {
  const entries = dateEntries(value).filter((entry) => !isDatePreset(entry));
  let start: string | null = null;
  let end: string | null = null;
  const undirected: string[] = [];
  for (const entry of entries) {
    const [day, direction] = entry.split(";");
    if (day === undefined || !ISO_DAY.test(day)) continue;
    if (direction === "before") {
      if (end === null) end = day;
    } else if (direction === "after") {
      if (start === null) start = day;
    } else {
      // A bare day (how a stored range or one-day lookup is written) takes
      // its place by position: first the lower bound, then the upper.
      undirected.push(day);
    }
  }
  const [first, second] = undirected;
  if (start === null && first !== undefined) start = first;
  if (end === null && second !== undefined) end = second;
  return { start, end };
}
