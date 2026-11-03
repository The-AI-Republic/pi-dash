// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages screen model (parity/specs/pages.md §3.1, PAGE-001/018–021). The
// project read drives the feature gate; the collection read only starts
// once the gate is known open. A failed initial read falls through to the
// empty state, while a failed background refetch keeps cached rows.

export type PagesBodyState = "skeleton" | "gate" | "zero" | "no-results" | "list";

export interface PagesBodyInput {
  projectPending: boolean;
  projectError: boolean;
  gateOff: boolean;
  pagesPending: boolean;
  pagesError: boolean;
  pageCount: number;
  tabCount: number;
  resultCount: number;
  searchActive: boolean;
  filtersActive: boolean;
}

/**
 * Order: project, gate, collection, then the content variant. A failed
 * initial collection read falls through to the empty content state (§2.7)
 * rather than stranding an error panel. If a background refetch fails after
 * rows have cached, those stale rows remain visible (PAGE-020). An empty tab
 * is a zero state; a non-empty tab narrowed to nothing is no-results.
 */
export function resolvePagesBody({
  projectPending,
  projectError,
  gateOff,
  pagesPending,
  pagesError,
  pageCount,
  tabCount,
  resultCount,
  searchActive,
  filtersActive,
}: PagesBodyInput): PagesBodyState {
  if (projectPending) return "skeleton";
  if (projectError) return "zero";
  if (gateOff) return "gate";
  if (pagesPending) return "skeleton";
  if (pagesError && pageCount === 0) return "zero";
  if (pageCount === 0 || tabCount === 0) return "zero";
  if (resultCount === 0 && (searchActive || filtersActive)) return "no-results";
  if (resultCount === 0) return "zero";
  return "list";
}

/** Tab title; the project name leads once the row is available. */
export function pagesTitle(projectName: string): string {
  return `${projectName} - Pages`;
}

export const PAGES_CRUMB = "Pages";
