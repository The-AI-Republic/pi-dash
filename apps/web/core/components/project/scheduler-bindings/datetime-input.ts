/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Round-trip helpers between `<input type="datetime-local">` (which speaks
 * "YYYY-MM-DDTHH:mm" in the browser's local wall time) and the UTC ISO
 * strings the scheduler API persists.
 */

const pad2 = (n: number): string => n.toString().padStart(2, "0");

/** Parse `"YYYY-MM-DDTHH:mm"` as local time, return UTC ISO. Empty → "". */
export function localToIsoUTC(local: string): string {
  if (!local) return "";
  const d = new Date(local);
  if (Number.isNaN(d.getTime())) return "";
  return d.toISOString();
}

/** Convert a UTC ISO string into the local-wall-time form the input expects. */
export function isoUTCToLocalInput(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return (
    `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}` +
    `T${pad2(d.getHours())}:${pad2(d.getMinutes())}`
  );
}

/** "YYYY-MM-DDTHH:mm" string for tomorrow at 9am local. Used as install default. */
export function defaultDtstartLocal(): string {
  const tomorrow = new Date();
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(9, 0, 0, 0);
  return (
    `${tomorrow.getFullYear()}-${pad2(tomorrow.getMonth() + 1)}-${pad2(tomorrow.getDate())}` +
    `T${pad2(tomorrow.getHours())}:${pad2(tomorrow.getMinutes())}`
  );
}
