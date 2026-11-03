// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle completion arithmetic (parity/specs/cycles.md §2, CYC-002). The
// ring reads completed work items over every item except cancelled
// ones; finished rows read the frozen snapshot so the ring cannot drift
// after a transfer. A zero denominator renders an empty ring with 0%,
// never a full ring or a check.

/** The counters the ring reads, from the row or its frozen snapshot. */
export interface CycleProgressInput {
  progress_snapshot?: Record<string, unknown> | null | undefined;
  total_issues?: number | undefined;
  completed_issues?: number | undefined;
  cancelled_issues?: number | undefined;
}

export interface CycleProgressCounts {
  completed: number;
  total: number;
}

function readCounter(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

/**
 * The counter source: the frozen snapshot whenever it carries entries,
 * else the live row. Snapshots only exist on finished cycles, so live
 * rows always read their own counters.
 */
function progressSource(cycle: CycleProgressInput): Record<string, unknown> {
  const snapshot = cycle.progress_snapshot;
  if (snapshot !== null && snapshot !== undefined && Object.keys(snapshot).length > 0) return snapshot;
  return { ...cycle };
}

/** Completed and total work items behind the ring, cancelled excluded from the total. */
export function cycleProgressCounts(cycle: CycleProgressInput): CycleProgressCounts {
  const counters = progressSource(cycle);
  return {
    completed: readCounter(counters.completed_issues),
    total: readCounter(counters.total_issues) - readCounter(counters.cancelled_issues),
  };
}

/** Completion percent, rounded; 100 only when the counts exactly agree. */
export function cycleProgressPercent(cycle: CycleProgressInput): number {
  const { completed, total } = cycleProgressCounts(cycle);
  if (total <= 0 || completed < 0) return 0;
  if (completed > total) return 100;
  return Math.round((completed / total) * 100);
}
