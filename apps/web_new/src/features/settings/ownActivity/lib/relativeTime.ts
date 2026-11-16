// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Own-history timestamps (parity/specs/profile.md §7, PROF-019). Rows read
// as a relative age for the recent past and as a fixed date once they are
// older than a week, in the caller's own zone.

const DAY_MS = 24 * 60 * 60 * 1000;
const WEEK_MS = 7 * DAY_MS;

/** The absolute date fallback ("Oct 14, 2026"), in the given zone. */
export function activityDate(isoTimestamp: string, timeZone: string | undefined): string {
  const at = new Date(isoTimestamp);
  if (Number.isNaN(at.getTime())) return "";
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    ...(timeZone === undefined || timeZone === "" ? {} : { timeZone }),
  }).format(at);
}

/** Relative age for the last week, an absolute date beyond it. */
export function activityAge(isoTimestamp: string, nowMs: number, timeZone: string | undefined): string {
  const at = Date.parse(isoTimestamp);
  if (Number.isNaN(at)) return "";
  const elapsed = Math.max(0, nowMs - at);
  if (elapsed >= WEEK_MS) return activityDate(isoTimestamp, timeZone);
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"} ago`;
  const hours = Math.floor(elapsed / (60 * 60_000));
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.floor(elapsed / DAY_MS);
  return days === 1 ? "yesterday" : `${days} days ago`;
}
