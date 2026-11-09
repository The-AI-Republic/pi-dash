// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Free-lane meter wording (NEWPIDASH-149). The meter is one sentence —
// how much of today the allowance holds, and when it rolls over — built
// from the Hub quota's own numbers. Token counts abbreviate (2M, 123K)
// so the sentence stays scannable; the reset renders in the reader's
// local time because the Hub's midnight-UTC rollover lands at a
// different clock time for every reader.

/** One count in the sentence's abbreviation: `2000000 -> "2M"`. */
export function formatTokenCount(count: number): string {
  if (!Number.isFinite(count) || count < 0) return "0";
  const scaled = (value: number, unit: number): string => {
    const rounded = Math.round((value / unit) * 10) / 10;
    return Number.isInteger(rounded) ? `${rounded}` : `${rounded.toFixed(1)}`;
  };
  if (count >= 1_000_000) return `${scaled(count, 1_000_000)}M`;
  if (count >= 1_000) return `${scaled(count, 1_000)}K`;
  return `${Math.floor(count)}`;
}

/** The reset instant as local `HH:MM`, or null when it cannot be read. */
export function formatResetTime(resetsAt: string): string | null {
  const instant = new Date(resetsAt);
  if (Number.isNaN(instant.getTime())) return null;
  const parts = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", hour12: false }).formatToParts(
    instant
  );
  const hour = parts.find((part) => part.type === "hour")?.value ?? null;
  const minute = parts.find((part) => part.type === "minute")?.value ?? null;
  if (hour === null || minute === null) return null;
  return `${hour}:${minute}`;
}

/**
 * The meter sentence: "X of Y free tokens used today, resets at HH:MM
 * (local)". An unreadable reset drops its clause rather than showing a
 * broken time — the counts are the point, the reset is the footnote.
 */
export function freeTokenMeterText(used: number, limit: number, resetsAt: string): string {
  const counts = `${formatTokenCount(used)} of ${formatTokenCount(limit)} free tokens used today`;
  const reset = formatResetTime(resetsAt);
  return reset === null ? counts : `${counts}, resets at ${reset} (local)`;
}
