/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ISchedulerOccurrence } from "@pi-dash/services";

// Past-run greys — chosen once so the month and week views stay in sync
// with the rail's "past = grey" convention from the decisions doc.
const PAST_BG = "#e5e7eb";
const PAST_FG = "#6b7280";
const PAST_BORDER = "#9ca3af";

/**
 * Inline-style object for an occurrence block. Past occurrences are flat
 * grey regardless of which scheduler fired them; future occurrences use
 * the scheduler's color (tinted background, full-strength border/text).
 */
export function getOccurrenceStyle(occurrence: ISchedulerOccurrence, now: Date) {
  const isPast = new Date(occurrence.dtstart) < now;
  if (isPast) {
    return {
      backgroundColor: PAST_BG,
      color: PAST_FG,
      borderLeft: `3px solid ${PAST_BORDER}`,
    };
  }
  return {
    backgroundColor: `${occurrence.scheduler_color}22`,
    color: occurrence.scheduler_color,
    borderLeft: `3px solid ${occurrence.scheduler_color}`,
  };
}
