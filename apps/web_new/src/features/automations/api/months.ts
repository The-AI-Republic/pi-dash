// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Idle-automation helpers (parity/specs/agents.md §8, AGT-034–036).
// Written from the spec and the four oracle scenarios: delays are whole
// months from one to twelve, presets offer 1/3/6/9/12, and the label
// reads "N month" for one and "N months" otherwise. Pure so both rows
// and the custom dialog share one rule.

/** Lowest delay the custom dialog accepts. */
export const MIN_DELAY_MONTHS = 1;
/** Highest delay the custom dialog accepts. */
export const MAX_DELAY_MONTHS = 12;
/** Preset delays the pickers offer before the custom entry. */
export const DELAY_PRESETS = [1, 3, 6, 9, 12] as const;

export const DELAY_RANGE_MESSAGE = `Select a month between ${MIN_DELAY_MONTHS} and ${MAX_DELAY_MONTHS}.`;

/** One or many: "1 month", "6 months". */
export function delayLabel(months: number): string {
  return months === 1 ? "1 month" : `${months} months`;
}

/** A delay turns an automation on exactly when it is a positive count. */
export function isDelayEnabled(months: number): boolean {
  return months > 0;
}

/**
 * Parse the custom dialog's raw input. Empty, non-numeric and
 * out-of-range values resolve to null; whole months in range resolve to
 * their number.
 */
export function parseDelayInput(raw: string): number | null {
  const trimmed = raw.trim();
  if (trimmed === "") return null;
  const months = Number(trimmed);
  if (!Number.isFinite(months) || !Number.isInteger(months)) return null;
  if (months < MIN_DELAY_MONTHS || months > MAX_DELAY_MONTHS) return null;
  return months;
}
