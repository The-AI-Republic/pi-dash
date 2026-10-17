// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Relative timestamps ("3 hours ago") for activity-adjacent surfaces:
// description history, comments, and the feed share one ladder so the
// wording stays identical everywhere. Native Intl only; no date
// library is pulled in for this.

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const WEEK_MS = 7 * DAY_MS;
const MONTH_MS = 30 * DAY_MS;
const YEAR_MS = 365 * DAY_MS;

/** Whole units between two timestamps, floored, never negative. */
function wholeUnits(elapsedMs: number, unitMs: number): number {
  return Math.max(0, Math.floor(elapsedMs / unitMs));
}

/**
 * Relative age of an ISO timestamp against now ("just now", "4 minutes
 * ago", "yesterday", ...). Unparseable or future input reads as now.
 */
export function timeAgo(isoTimestamp: string, nowMs: number = Date.now()): string {
  const atMs = Date.parse(isoTimestamp);
  if (Number.isNaN(atMs)) return "just now";
  const elapsedMs = nowMs - atMs;
  if (elapsedMs < MINUTE_MS) return "just now";
  if (elapsedMs < HOUR_MS) {
    const minutes = wholeUnits(elapsedMs, MINUTE_MS);
    return `${minutes} minute${minutes === 1 ? "" : "s"} ago`;
  }
  if (elapsedMs < DAY_MS) {
    const hours = wholeUnits(elapsedMs, HOUR_MS);
    return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  }
  const days = wholeUnits(elapsedMs, DAY_MS);
  if (days === 1) return "yesterday";
  if (days < 7) return `${days} days ago`;
  if (elapsedMs < MONTH_MS) {
    const weeks = wholeUnits(elapsedMs, WEEK_MS);
    return `${weeks} week${weeks === 1 ? "" : "s"} ago`;
  }
  if (elapsedMs < YEAR_MS) {
    const months = wholeUnits(elapsedMs, MONTH_MS);
    return `${months} month${months === 1 ? "" : "s"} ago`;
  }
  const years = wholeUnits(elapsedMs, YEAR_MS);
  return `${years} year${years === 1 ? "" : "s"} ago`;
}
