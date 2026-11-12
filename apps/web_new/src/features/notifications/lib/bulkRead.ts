// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk mark-all-read cache rule, written from
// parity/specs/notifications.md §7 and §11 (NTF-014). Pure: given the
// cached list pages and the scope the header posted, return the pages
// that fall inside the scope with every row read. The unread totals are
// not computed here — the mutation refetches them so the tab badges agree
// with the server totals.
import type { MarkAllReadScope, NotificationListQuery, NotificationListResponse } from "@pidash/api-client";

import { mapStreamPages, type StreamData } from "./streamData.js";

/**
 * Whether a cached stream falls inside a posted scope. Mark-all-read
 * reads unread rows only, so the read flag never narrows the scope; the
 * origin, snoozed and archived facets must match, and an absent facet
 * reads as its default (no origin, both exclusions off).
 */
export function isRowInBulkReadScope(query: NotificationListQuery, scope: MarkAllReadScope): boolean {
  return (
    (query.type ?? null) === (scope.type ?? null) &&
    (query.snoozed ?? false) === (scope.snoozed ?? false) &&
    (query.archived ?? false) === (scope.archived ?? false)
  );
}

/** One list page with every row in it read. */
export function markPageRead(page: NotificationListResponse, readAt: string): NotificationListResponse {
  return {
    ...page,
    results: page.results.map((row) => (row.read_at === null ? { ...row, read_at: readAt } : row)),
  };
}

export interface CachedListPage {
  key: readonly unknown[];
  query: NotificationListQuery;
  page: StreamData;
}

/**
 * The in-scope cached streams, each with the rows of every page read. A
 * cursor stream keeps its page layout; only the rows change, so the
 * continuation cursors stay usable.
 */
export function applyBulkRead(
  pages: readonly CachedListPage[],
  scope: MarkAllReadScope,
  readAt: string = new Date().toISOString()
): [readonly unknown[], StreamData][] {
  return pages
    .filter((entry) => isRowInBulkReadScope(entry.query, scope))
    .map((entry) => [entry.key, mapStreamPages(entry.page, (page) => markPageRead(page, readAt))] as const);
}
