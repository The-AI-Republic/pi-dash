// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy filter dimension details (parity/specs/issues.md §9, ISS-089–092).
// The condition-row builder owns the editing surface; this module holds the
// parts specific to these dimensions and carries no React: which
// lifecycle a cycle reports (and which lifecycle pulses), and the relative
// windows the start/target dimensions offer beside their custom range.
// Row ordering lives with the other value-list shaping in filterValues.
//
// Date entries stay in the list endpoint's legacy date grammar — a preset
// is `<n>_<unit>s;after;fromnow`, a custom day is `<day>;after` or
// `<day>;before` — because that is the only form the server resolves
// against today. The query bridge forwards those entries as the bare
// `start_date` / `target_date` parameters and keeps them out of the JSON
// filter tree, whose filter set accepts calendar days only.

/** Cycle lifecycles the dimension marks. */
export type CycleLifecycle = "draft" | "upcoming" | "current" | "completed";

/**
 * The lifecycle one cycle row reports. The server sends upper-case names;
 * a missing or unknown status reads as a draft, which is how cycles that
 * have not started list in the old filter. "ACTIVE" is the running state.
 */
export function cycleLifecycle(status: string | undefined): CycleLifecycle {
  switch ((status ?? "").trim().toLowerCase()) {
    case "current":
    case "active":
      return "current";
    case "upcoming":
      return "upcoming";
    case "completed":
      return "completed";
    default:
      return "draft";
  }
}

/** One relative window a date dimension offers. */
export interface DatePreset {
  /** Token the list endpoint resolves relative to today. */
  value: string;
  label: string;
}

/**
 * The relative windows both date dimensions offer. They mirror the
 * server's accepted token grammar (`<n>_<unit>s;after;fromnow`) and read
 * as forward-looking windows, so a preset is "in this span from now".
 */
export const DATE_PRESETS: readonly DatePreset[] = [
  { value: "1_weeks;after;fromnow", label: "1 week from now" },
  { value: "2_weeks;after;fromnow", label: "2 weeks from now" },
  { value: "1_months;after;fromnow", label: "1 month from now" },
  { value: "2_months;after;fromnow", label: "2 months from now" },
];

const PRESET_VALUES = new Set(DATE_PRESETS.map((preset) => preset.value));

export function isDatePreset(entry: string): boolean {
  return PRESET_VALUES.has(entry);
}

/** Which shape the custom half of a date value holds. */
export type CustomDateMode = "exact" | "after" | "before" | "range";

/** A custom day's lower bound. */
export function customStartEntry(day: string): string {
  return `${day};after`;
}

/** A custom day's upper bound. */
export function customEndEntry(day: string): string {
  return `${day};before`;
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

/** Whether a custom day is set (used to tick the Custom row). */
export function hasCustomDate(value: unknown): boolean {
  return customDays(value).length > 0;
}

/** Rebuild a stored value; no entries read as no value at all. */
function joinDateValue(entries: readonly string[]): unknown {
  if (entries.length === 0) return null;
  return entries.length === 1 ? entries[0] : entries;
}

/** Toggle one preset into a value, keeping custom entries as they were. */
export function toggleDatePreset(value: unknown, preset: string): unknown {
  const entries = dateEntries(value);
  if (entries.includes(preset)) return joinDateValue(entries.filter((entry) => entry !== preset));
  return joinDateValue([...entries, preset]);
}

/** Remove every custom entry, leaving presets untouched. */
export function clearCustomDate(value: unknown): unknown {
  return joinDateValue(dateValueParts(value).presets);
}

/**
 * Custom days only (presets removed), in stored order, as bare ISO days.
 * `dateEntries` drops blank entries, so every split has a day to read.
 */
export function customDays(value: unknown): string[] {
  return dateEntries(value)
    .filter((entry) => !isDatePreset(entry))
    .map((entry) => entry.split(";")[0]!)
    .filter((entry) => ISO_DAY.test(entry));
}

/**
 * Rebuild a date value from its presets and one custom mode. After and
 * Before keep one directioned day in the list endpoint's legacy date
 * grammar, so they can travel as its bare parameter; Range keeps two plain
 * days for the JSON range leaf; an exact day carries no direction.
 */
export function composeCustomDate(
  presets: readonly string[],
  mode: CustomDateMode,
  start: string | null,
  end: string | null
): unknown {
  const custom =
    mode === "range"
      ? // A pair under the between lookup is two plain days: `__range`
        // already means both ends, so no direction is stored.
        [start, end]
      : mode === "after"
        ? [start === null ? null : customStartEntry(start)]
        : mode === "before"
          ? [start === null ? null : customEndEntry(start)]
          : [start];
  // Nothing picked leaves no custom entry; the presets stay.
  return joinDateValue([...presets, ...custom.filter((entry): entry is string => entry !== null)]);
}

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

/** Trigger text and chip label: preset names, then the custom range. */
// A value always carries at least one entry when a label is asked for, so
// the presets and the custom half never both read empty.
export function dateValueLabel(value: unknown): string | null {
  const parts = dateValueParts(value);
  // A preset's label always exists: the list above is the only source of
  // preset values, and `dateValueParts` only classifies those as presets.
  const labels = parts.presets.map((preset) => DATE_PRESETS.find((candidate) => candidate.value === preset)!.label);
  const range = customRange(value);
  const mode = customDateMode(value);
  if (mode === "range" && range.start !== null && range.end !== null) labels.push(`${range.start} → ${range.end}`);
  else if (mode === "after" && range.start !== null) labels.push(`After ${range.start}`);
  else if (mode === "before" && range.end !== null) labels.push(`Before ${range.end}`);
  // One custom entry always reads into the start: an exact day and an
  // after-bound are lower bounds, and a lone before-bound is the second
  // undirected day that `customRange` assigns to the start when empty.
  else if (range.start !== null) labels.push(range.start);
  for (const entry of dateEntries(value)) {
    if (!isDatePreset(entry) && !isCustomDateEntry(entry)) labels.push(entry);
  }
  return labels.length === 0 ? null : labels.join(", ");
}
