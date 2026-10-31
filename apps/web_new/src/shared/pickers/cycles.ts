// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle option shaping (parity/specs/issues.md §17, ISS-212). Project
// cycles in server order with completed cycles filtered out; the
// assigned cycle stays listed unless the caller excludes it. A No-cycle
// clear row leads the list when allowed. The list stays thin: full
// cycle ownership lives in the cycles area.
import type { CycleList } from "@pidash/api-client";

/** One cycle row in display order. */
export interface CycleOption {
  id: string;
  name: string;
  /** True for the leading No-cycle clear row. */
  clear: boolean;
}

export interface CycleOptionsInput {
  cycles: CycleList;
  excludeIds?: readonly string[] | undefined;
  allowClear?: boolean | undefined;
  /**
   * True lists completed cycles too (the cycle detail switcher,
   * CYC-033). Assignment callers leave it off (ISS-212).
   */
  includeCompleted?: boolean | undefined;
}

/** Completed cycles never list; a missing status keeps the row. */
export function isCompletedCycle(status: string | undefined): boolean {
  return status !== undefined && status.toLowerCase().includes("complet");
}

export function selectCycleOptions(input: CycleOptionsInput): CycleOption[] {
  const options = input.cycles
    .filter(
      (cycle) =>
        ((input.includeCompleted ?? false) || !isCompletedCycle(cycle.status)) &&
        !(input.excludeIds?.includes(cycle.id) ?? false)
    )
    .map((cycle) => ({ id: cycle.id, name: cycle.name, clear: false }));
  if (input.allowClear ?? true) options.unshift({ id: "", name: "No cycle", clear: true });
  return options;
}
