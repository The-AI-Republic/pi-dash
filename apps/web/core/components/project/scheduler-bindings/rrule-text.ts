/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { rrulestr } from "rrule";

/**
 * Humanize an RFC 5545 RRULE string. Returns the raw input on parse error
 * (so the UI can fall back to showing exactly what's stored), or `null`
 * when the input is empty.
 *
 * Strips the `RRULE:` line prefix because dateutil's `rrulestr` accepts
 * both forms but the rrule npm package's `toText()` prints cleaner output
 * for the bare form.
 */
export function humanizeRrule(rrule: string, dtstart?: Date | string | null): string | null {
  if (!rrule) return null;
  const cleaned = rrule.replace(/^RRULE:/i, "");
  try {
    const anchor = dtstart ? new Date(dtstart) : new Date();
    return rrulestr(cleaned, { dtstart: anchor }).toText();
  } catch {
    return rrule;
  }
}
