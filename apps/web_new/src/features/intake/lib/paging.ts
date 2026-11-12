// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Paging trigger for the intake list (parity/specs/intake.md INT-007).
// The next page is requested once the scroll position nears the pane's
// end; the fetch itself stays with the query.

/** How close to the pane's end (px) starts the next page read. */
export const INTAKE_PAGING_THRESHOLD_PX = 200;

export interface IntakeScrollMetrics {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}

/** Whether the scroll position has neared the list's end. */
export function nearListEnd(metrics: IntakeScrollMetrics): boolean {
  return metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight <= INTAKE_PAGING_THRESHOLD_PX;
}
