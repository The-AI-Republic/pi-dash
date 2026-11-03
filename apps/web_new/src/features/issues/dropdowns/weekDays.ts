// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Week-start primitives (parity/specs/issues.md §17, ISS-214). Split out
// of calendar.ts (NEWFRONT-852, size gate): the first-paint week-start
// read needs only the fallback and the clamp, while the pickers' month
// names, grids and formatters must stay in lazy chunks. calendar.ts
// re-exports both, so existing importers keep working.

/** Fallback week start (Sunday) when the profile carries none. */
export const DEFAULT_WEEK_START = 0;

/** Clamp an unknown week-start value onto the 0-6 Sunday-first range. */
export function normalizeWeekStart(value: unknown): number {
  if (typeof value !== "number" || Number.isNaN(value)) return DEFAULT_WEEK_START;
  const floored = Math.floor(value);
  if (floored < 0 || floored > 6) return DEFAULT_WEEK_START;
  return floored;
}
