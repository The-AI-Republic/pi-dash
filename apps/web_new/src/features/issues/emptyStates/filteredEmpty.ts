// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Which card an empty list shows (parity/specs/issues.md §8, ISS-068–071).
// The cycle list keys off the stored rich expression as soon as it loads.
// The project, module and archived lists instead keep the first-run card
// for a stored expression and switch only after a condition was added
// through the filter row during this visit. A page with no complete
// condition always shows the first-run card, so clearing every filter
// restores it.
//
// The completeness test stays with the expression module: callers that
// already parse the expression pass the answer in, so this module never
// pulls the parser into the route's chunk (Quality gates, NEWFRONT-637).

/** How a context decides that an empty list is a filtered result. */
export type FilteredEmptyMode = "stored" | "visit";

export interface FilteredEmptyInput {
  mode: FilteredEmptyMode;
  /** At least one condition holds a property, operator and a usable value. */
  hasCompleteCondition: boolean;
  /** A condition was added through the filter row in this visit. */
  addedThisVisit: boolean;
  /** Rows the server returned for the current expression. */
  visibleCount: number;
}

/** True when an empty list should offer clearing filters. */
export function isFilteredEmpty({
  mode,
  hasCompleteCondition,
  addedThisVisit,
  visibleCount,
}: FilteredEmptyInput): boolean {
  if (visibleCount > 0) return false;
  if (!hasCompleteCondition) return false;
  return mode === "stored" || addedThisVisit;
}
