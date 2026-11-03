// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member-timezone date readings for cycles (PROF-017 stored-dates fix,
// NEWFRONT-859). The profile parity scenario reads the cycle's stored
// start/end dates rendered in the signed-in member's timezone; this
// module is that conversion and nothing else. The full CYC-044 span
// (project-zone dates, offset chip, viewer tooltip) belongs to the
// cycles slices, which may consolidate this helper when their shared
// date helper lands.

/**
 * A timezone the formatter accepts, else undefined for the system zone.
 * Broken zones are rejected by the API, so this is only a backstop: the
 * UI degrades to local time instead of throwing.
 */
function resolveTimeZone(timeZone: string | null | undefined): string | undefined {
  if (timeZone === null || timeZone === undefined || timeZone.trim() === "") return undefined;
  try {
    new Intl.DateTimeFormat("en-US", { timeZone });
    return timeZone;
  } catch {
    return undefined;
  }
}

/**
 * A stored cycle instant rendered "MMM dd, yyyy" in the member timezone
 * (e.g. "Nov 19, 2026"). Matches the parity spec's expected conversion
 * exactly; an absent or broken zone falls back to system local time.
 */
export function formatMemberDate(stored: string, timeZone: string | null | undefined): string {
  const zone = resolveTimeZone(timeZone);
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "2-digit",
    year: "numeric",
    ...(zone === undefined ? {} : { timeZone: zone }),
  }).format(new Date(stored));
}
