// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Freshness helpers (parity/specs/issues.md §1, ISS-006). Issues created
// within the freshness window render immediately and carry the transient
// highlight; dropped issues are highlighted explicitly. Layouts mount
// fresh rows outside their virtualized content so the bypass cannot be
// cleared by an intersection observer (bug NEWFRONT-171).

/** Freshness window: issues created within the last half minute. */
export const FRESH_WINDOW_MS = 30_000;

/**
 * Bypass cap (bug NEWFRONT-1014): at most this many fresh rows mount in
 * the unvirtualized strip; the rest virtualize like stale rows. Ten roomy
 * rows are ~440 px, so even a screenful of fresh rows leaves the flat
 * virtualizer a usable share of its section instead of squeezing it to
 * zero height and killing pagination.
 */
export const FRESH_BYPASS_LIMIT = 10;

/** Whether an issue counts as fresh at `now`. */
export function isFresh(createdAt: string, now: number = Date.now()): boolean {
  return now - Date.parse(createdAt) < FRESH_WINDOW_MS;
}

/** DOM id of one issue row; drops scroll it into view. */
export function rowDomId(issueId: string): string {
  return `issue-${issueId}`;
}

/**
 * Whether a row carries the transient highlight: fresh issues always do
 * (bug NEWFRONT-168), otherwise only the explicitly highlighted one.
 */
export function isRowHighlighted(
  issue: { id: string; created_at: string },
  highlightedId: string | null,
  now: number = Date.now()
): boolean {
  return isFresh(issue.created_at, now) || issue.id === highlightedId;
}

export interface FreshPartition<T> {
  /** Fresh rows, in input order, for the unvirtualized strip. */
  fresh: T[];
  /** The remaining rows, in input order, for virtualized content. */
  rest: T[];
}

/**
 * Split fresh rows from the rest, preserving order in both halves. Only
 * the first `limit` fresh rows bypass; the overflow joins `rest` so the
 * strip stays bounded no matter how many rows are fresh.
 */
export function partitionFresh<T extends { created_at: string }>(
  issues: readonly T[],
  now: number = Date.now(),
  limit: number = FRESH_BYPASS_LIMIT
): FreshPartition<T> {
  const fresh: T[] = [];
  const rest: T[] = [];
  for (const issue of issues) {
    if (isFresh(issue.created_at, now) && fresh.length < limit) fresh.push(issue);
    else rest.push(issue);
  }
  return { fresh, rest };
}

/** A client-side id for an unconfirmed create (bug NEWFRONT-170). */
export function createTempId(): string {
  return `temp-${crypto.randomUUID()}`;
}

/** Whether a row id belongs to an unconfirmed temp row. */
export function isTempRowId(id: string): boolean {
  return id.startsWith("temp-");
}
