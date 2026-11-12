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

import {
  DATE_PRESET_VALUES,
  customDateMode,
  customRange,
  dateEntries,
  dateValueParts,
  isCustomDateEntry,
  isDatePreset,
  type CustomDateMode,
} from "./dateValue.js";

export {
  customDateEntries,
  customDateMode,
  customRange,
  dateEntries,
  dateValueParts,
  isCustomDateEntry,
  isDatePreset,
  type CustomDateMode,
} from "./dateValue.js";

/** A calendar day in the ISO form the calendar and the server share. */
const ISO_DAY = /^\d{4}-\d{2}-\d{2}$/;

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

/** Labels for the tokens the shared date grammar defines. */
const PRESET_LABELS: Record<string, string> = {
  "1_weeks;after;fromnow": "1 week from now",
  "2_weeks;after;fromnow": "2 weeks from now",
  "1_months;after;fromnow": "1 month from now",
  "2_months;after;fromnow": "2 months from now",
};

/**
 * The relative windows both date dimensions offer, in display order. The
 * tokens are the shared grammar's presets, so this list and the parser that
 * reads stored values cannot drift apart.
 */
export const DATE_PRESETS: readonly DatePreset[] = DATE_PRESET_VALUES.map((value) => ({
  value,
  label: PRESET_LABELS[value]!,
}));

/** A custom day's lower bound. */
export function customStartEntry(day: string): string {
  return `${day};after`;
}

/** A custom day's upper bound. */
export function customEndEntry(day: string): string {
  return `${day};before`;
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
