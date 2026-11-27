// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake date-filter values (parity/specs/intake.md INT-010). Both date
// sections share the same five options — four presets plus an explicit
// range — and the same wire encoding: one `FROM;after,TO;before` pair
// per parameter. Days are the viewer's local calendar days, matching the
// old app: the token builder reads the local day, and rows render the
// local day of their timestamp.

import { formatDay, formatMergedRange } from "../../issues/index.js";

/** Day string of the viewer's local clock (`YYYY-MM-DD`). */
export function todayLocalIsoDay(now: Date = new Date()): string {
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

const ISO_DAY_ONLY_PATTERN = /^(\d{4})-(\d{2})-(\d{2})$/;

/**
 * Local calendar day (`YYYY-MM-DD`) of a timestamp in the viewer's
 * timezone. Date-only values carry no time of day and pass through
 * unchanged; anything unparseable falls back to its day prefix.
 */
export function timestampToLocalDay(value: string): string {
  if (ISO_DAY_ONLY_PATTERN.test(value)) return value;
  const instant = new Date(value);
  if (Number.isNaN(instant.getTime())) return value.slice(0, 10);
  const year = instant.getFullYear();
  const month = String(instant.getMonth() + 1).padStart(2, "0");
  const day = String(instant.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/** Shift an ISO day by whole calendar days. */
export function shiftIsoDay(iso: string, deltaDays: number): string {
  const shifted = new Date(`${iso}T00:00:00Z`);
  shifted.setUTCDate(shifted.getUTCDate() + deltaDays);
  return shifted.toISOString().slice(0, 10);
}

/**
 * The same calendar day one month on, for the custom dialog's range
 * seed. Month-end days roll into the following month the way the
 * platform calendar rolls them.
 */
export function sameDayNextMonth(today: string): string {
  const [year, month, day] = today.split("-").map(Number) as [number, number, number];
  return new Date(Date.UTC(year, month, day)).toISOString().slice(0, 10);
}

/** Quick presets in section order, as local day offsets from today. */
export const INTAKE_DATE_PRESETS = [
  { id: "today", label: "Today", fromOffset: 0, toOffset: 0 },
  { id: "yesterday", label: "Yesterday", fromOffset: -1, toOffset: -1 },
  { id: "last7", label: "Last 7 days", fromOffset: -7, toOffset: 0 },
  { id: "last30", label: "Last 30 days", fromOffset: -30, toOffset: 0 },
] as const;

export type IntakeDatePresetId = (typeof INTAKE_DATE_PRESETS)[number]["id"];

/** Label of the custom-range row that opens the dialog. */
export const INTAKE_CUSTOM_RANGE_LABEL = "Custom";

/** The three explicit-range shapes the custom dialog offers. */
export const INTAKE_CUSTOM_MODES = [
  { mode: "before", label: "Before" },
  { mode: "after", label: "After" },
  { mode: "range", label: "Range" },
] as const;

export type IntakeCustomMode = (typeof INTAKE_CUSTOM_MODES)[number]["mode"];

/** The dialog opens on the range pair (INT-010). */
export const DEFAULT_INTAKE_CUSTOM_MODE: IntakeCustomMode = "range";

/** One date section's applied value: a preset or an explicit bound. */
export type IntakeDateValue =
  | { kind: "preset"; preset: IntakeDatePresetId }
  | { kind: "custom-before"; day: string }
  | { kind: "custom-after"; day: string }
  | { kind: "custom-range"; from: string; to: string };

/** Day bounds a preset resolves to against `today`. */
export function presetDayRange(preset: IntakeDatePresetId, today: string): { from: string; to: string } {
  const entry = INTAKE_DATE_PRESETS.find((candidate) => candidate.id === preset) ?? INTAKE_DATE_PRESETS[0]!;
  return { from: shiftIsoDay(today, entry.fromOffset), to: shiftIsoDay(today, entry.toOffset) };
}

/**
 * The wire token for a date value: exactly one `FROM;after,TO;before`
 * pair, or the single bound token for a one-sided explicit range
 * (INT-010, NEWFRONT-274 intended).
 */
export function intakeDateToken(value: IntakeDateValue, today: string): string {
  switch (value.kind) {
    case "preset": {
      const bounds = presetDayRange(value.preset, today);
      return `${bounds.from};after,${bounds.to};before`;
    }
    case "custom-before":
      return `${value.day};before`;
    case "custom-after":
      return `${value.day};after`;
    case "custom-range":
      return `${value.from};after,${value.to};before`;
  }
}

/** Chip text for a date value: the preset label or the bound days. */
export function intakeDateChipLabel(value: IntakeDateValue): string {
  switch (value.kind) {
    case "preset":
      return (INTAKE_DATE_PRESETS.find((candidate) => candidate.id === value.preset) ?? INTAKE_DATE_PRESETS[0]!).label;
    case "custom-before":
      return `Before ${formatDay(value.day)}`;
    case "custom-after":
      return `After ${formatDay(value.day)}`;
    case "custom-range":
      return formatMergedRange(value.from, value.to);
  }
}

/**
 * Whether a draft range pair can apply: both ends picked and the start
 * no later than the end. The comparison reads the real second day
 * (INT-010 intended; the old guard compared the first day with itself).
 */
export function isIntakeRangeComplete(from: string | null, to: string | null): boolean {
  return from !== null && to !== null && from <= to;
}

/**
 * Which custom shape a stored value reopens in, or null for a preset:
 * the dialog returns to the shape the value holds.
 */
export function customModeForValue(value: IntakeDateValue): IntakeCustomMode | null {
  switch (value.kind) {
    case "preset":
      return null;
    case "custom-before":
      return "before";
    case "custom-after":
      return "after";
    case "custom-range":
      return "range";
  }
}
