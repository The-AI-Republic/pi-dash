// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycles body states (parity/specs/archives.md §4,
// ARCH-014, ARCH-017). Pure: the one body decision the tab renders.
// The pending read shows the skeleton and a failed read settles into
// the error view — a skeleton never outlives a settled request. An
// empty archive reads the zero state; narrowing to zero rows names
// the cause, search (alone or combined) or filters alone, so each
// offers its own clear.

/** The one body the archived-cycles tab renders. */
export type ArchivedCyclesBodyState = "skeleton" | "error" | "zero" | "list" | "noMatchSearch" | "noMatchFilters";

export interface ArchivedCyclesBodyInput {
  /** The list read has no data yet (a background refetch keeps its rows). */
  listPending: boolean;
  listError: boolean;
  /** Archived cycles on the server, before narrowing. */
  totalCount: number;
  /** Rows left after the search query and the filter predicate. */
  visibleCount: number;
  /** Current search text; empty means filters alone hid every row. */
  query: string;
}

export function resolveArchivedCyclesBody({
  listPending,
  listError,
  totalCount,
  visibleCount,
  query,
}: ArchivedCyclesBodyInput): ArchivedCyclesBodyState {
  if (listPending) return "skeleton";
  if (listError) return "error";
  if (totalCount === 0) return "zero";
  if (visibleCount === 0) return query === "" ? "noMatchFilters" : "noMatchSearch";
  return "list";
}
