// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Relative card ages (written from parity/specs/notifications.md §4,
// NTF-005). Sub-minute cards read as "just now"; everything older reads
// as "<n> <unit> ago". Unparseable stamps degrade to "just now" so a bad
// payload never blanks the card.
const SECOND_MS = 1000;
const MINUTE_MS = 60 * SECOND_MS;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const WEEK_MS = 7 * DAY_MS;
const MONTH_MS = 30 * DAY_MS;

const formatter = new Intl.RelativeTimeFormat("en", { numeric: "always" });

export function formatNotificationAge(createdAt: string, nowMs: number = Date.now()): string {
  const thenMs = Date.parse(createdAt);
  if (Number.isNaN(thenMs)) return "just now";
  const elapsedMs = nowMs - thenMs;
  if (elapsedMs < MINUTE_MS) return "just now";
  const minutes = Math.floor(elapsedMs / MINUTE_MS);
  if (minutes < 60) return formatter.format(-minutes, "minute");
  const hours = Math.floor(elapsedMs / HOUR_MS);
  if (hours < 24) return formatter.format(-hours, "hour");
  const days = Math.floor(elapsedMs / DAY_MS);
  if (days < 7) return formatter.format(-days, "day");
  const weeks = Math.floor(elapsedMs / WEEK_MS);
  if (weeks < 5) return formatter.format(-weeks, "week");
  const months = Math.floor(elapsedMs / MONTH_MS);
  if (months < 12) return formatter.format(-months, "month");
  return formatter.format(-Math.floor(months / 12), "year");
}
