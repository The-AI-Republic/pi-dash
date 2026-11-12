// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Compact minute counts ("45m", "1h", "1h 30m") for worklog surfaces:
// feed sentences, sidebar totals, and entry rows share one ladder so the
// wording stays identical everywhere.

/**
 * Whole minutes rendered compactly. Sub-hour counts read as minutes,
 * exact hours as hours, and the rest as hours plus minutes. Anything
 * below one minute reads as zero.
 */
export function formatMinutes(minutes: number): string {
  const whole = Math.max(0, Math.floor(minutes));
  if (whole < 60) return `${whole}m`;
  const hours = Math.floor(whole / 60);
  const rest = whole % 60;
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`;
}
