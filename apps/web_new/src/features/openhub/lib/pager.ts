// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Catalog cursor pager (written from parity/specs/cloud.md §5,
// CLOUD-023). Windows chain over a cursor history stack: stepping back
// re-reads the earlier window (page one reads cursorless), and a new
// query rewinds the stack so result sets never mix.
import * as React from "react";

export interface CatalogPagerState {
  /** Settled query this stack belongs to; a change rewinds it. */
  query: string;
  /** Cursor per visited page; the first page reads without one. */
  cursors: Array<string | null>;
  /** Index into cursors of the window on screen. */
  at: number;
}

/** Fresh stack on the first window of one query. */
export function createCatalogPager(query: string): CatalogPagerState {
  return { query, cursors: [null], at: 0 };
}

/** Cursor for the window on screen; null reads the first window. */
export function currentPagerCursor(pager: CatalogPagerState): string | null {
  return pager.cursors[pager.at] ?? null;
}

/** One-based page number for the indicator. */
export function pagerPage(pager: CatalogPagerState): number {
  return pager.at + 1;
}

/**
 * Step onto the next window. Without a next cursor the pager stays put;
 * stepping forward past a visited page drops its stale tail.
 */
export function nextCatalogPage(pager: CatalogPagerState, nextCursor: string | null | undefined): CatalogPagerState {
  if (nextCursor === null || nextCursor === undefined) return pager;
  return { query: pager.query, cursors: [...pager.cursors.slice(0, pager.at + 1), nextCursor], at: pager.at + 1 };
}

/** Step back one window; the first window stays put. */
export function previousCatalogPage(pager: CatalogPagerState): CatalogPagerState {
  if (pager.at === 0) return pager;
  return { ...pager, at: pager.at - 1 };
}

export interface CatalogPager {
  /** Cursor for the window on screen; null reads the first window. */
  cursor: string | null;
  /** One-based page number for the indicator. */
  page: number;
  /** Step onto the window the cursor opens; ignored without one. */
  goNext: (nextCursor: string | null | undefined) => void;
  /** Step back one window; ignored on the first. */
  goPrevious: () => void;
}

/**
 * Pager for one settled query. The reset happens at render time and only
 * on an actual query change, so a settling debounce can never wipe a
 * window mid-page (CLOUD-023: pager steps with reset on new queries).
 */
export function useCatalogPager(query: string): CatalogPager {
  const [pager, setPager] = React.useState(() => createCatalogPager(query));
  const active = pager.query === query ? pager : createCatalogPager(query);
  if (pager.query !== query) {
    setPager(active);
  }
  return {
    cursor: currentPagerCursor(active),
    page: pagerPage(active),
    // State matches the query by paint time (the reset above lands
    // before it), so steps always run against the current stack.
    goNext: (nextCursor: string | null | undefined) => {
      setPager((previous) => nextCatalogPage(previous, nextCursor));
    },
    goPrevious: () => {
      setPager((previous) => previousCatalogPage(previous));
    },
  };
}
