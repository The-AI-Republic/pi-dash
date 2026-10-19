// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preference row vocabulary (parity/specs/profile.md §7, PROF-016/017).
// One option table per row: the picker renders these in order, and the
// trigger shows the label of the stored value. Week-start values are the
// server's zero-based weekday index with Sunday at zero; an out-of-range
// value falls back to Sunday rather than leaving the row blank.

import { THEME_OPTIONS, type ThemeValue } from "../../../../core/theme/index.js";

export interface PreferenceOption<T extends string | number> {
  value: T;
  label: string;
}

/** Theme values in display order, labelled for the picker. */
export const THEME_SELECT_OPTIONS: ReadonlyArray<PreferenceOption<ThemeValue>> = THEME_OPTIONS.map((option) => ({
  value: option.value,
  label: option.label,
}));

export const WEEKDAY_LABELS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"] as const;

/** First-day options, Sunday at zero. */
export const WEEK_START_OPTIONS: ReadonlyArray<PreferenceOption<number>> = WEEKDAY_LABELS.map((label, value) => ({
  value,
  label,
}));

/** A valid zero-based weekday index; anything else reads as Sunday. */
export function normalizeWeekStart(raw: number | null | undefined): number {
  return typeof raw === "number" && Number.isInteger(raw) && raw >= 0 && raw <= 6 ? raw : 0;
}

/** The label of a normalized week-start index. */
export function weekStartLabel(raw: number | null | undefined): string {
  return WEEKDAY_LABELS[normalizeWeekStart(raw)]!;
}

/**
 * The label a language row shows. A tag this build ships labels normally;
 * a tag staged by a fuller build (or the server) that OSS cannot label
 * shows its raw tag, so the stored value is always visible.
 */
export function languageLabel(tag: string, supported: ReadonlyArray<{ tag: string; label: string }>): string {
  return supported.find((option) => option.tag === tag)?.label ?? tag;
}

/** The label a picker shows for a stored value (the raw value if absent). */
export function optionLabel<T extends string | number>(options: ReadonlyArray<PreferenceOption<T>>, value: T): string {
  return options.find((option) => option.value === value)!.label;
}

/** One zone row the timezone picker renders. */
export interface TimezoneChoice {
  value: string;
  label: string;
  detail: string;
}

/**
 * The timezone row's options and current label. A stored zone the
 * catalog no longer lists stays pickable at the top of the list, so the
 * row always names the value the server holds; a member with no zone
 * reads "Not set".
 */
export function timezoneChoices(
  zones: ReadonlyArray<{ value: string; label: string; utc_offset: string }>,
  timezone: string
): { options: ReadonlyArray<TimezoneChoice>; label: string } {
  const rows = zones.map((zone) => ({
    value: zone.value,
    label: zone.label,
    detail: `${zone.utc_offset} ${zone.label}`,
  }));
  if (timezone === "") return { options: rows, label: "Not set" };
  const known = rows.find((row) => row.value === timezone);
  if (known === undefined) {
    return { options: [{ value: timezone, label: timezone, detail: timezone }, ...rows], label: timezone };
  }
  return { options: rows, label: known.label };
}
