// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues read (parity/specs/issues.md §18, ISS-223). The
// route filters stay out of the query: this build renders the same
// unfiltered list with or without them, which the oracle pins. The
// identity still keys on nothing but the workspace so the plain view
// and a filtered deep link share one cache entry. The read pages with
// the server cursor until a page reports no successor, so the view can
// reach every visible issue rather than the first window of them.

import { infiniteQueryOptions } from "@tanstack/react-query";
import {
  listGlobalIssues,
  type ApiClient,
  type GlobalIssueListResponse,
  type IssueListQuery,
} from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";

/** Rows per request for the workspace all-issues view. */
export const GLOBAL_ISSUES_PAGE_SIZE = 100;

/** A read with no scope beyond the workspace: the plain all-issues list. */
function isPlainGlobalQuery(query: IssueListQuery): boolean {
  const filters = query.filters;
  return (
    query.orderBy === undefined &&
    query.groupBy === undefined &&
    query.subGroupBy === undefined &&
    query.perPage === undefined &&
    (filters === undefined || Object.keys(filters).length === 0)
  );
}

/**
 * Extra key parts for a scoped read. Empty for the plain all-issues read
 * so its key (and cache entry) stays exactly what ISS-223 pins; a view's
 * stored expression, a default's static condition or an order keys its
 * own entry.
 */
function scopedGlobalKeyParts(query: IssueListQuery): unknown[] {
  if (isPlainGlobalQuery(query)) return [];
  return [
    {
      orderBy: query.orderBy ?? null,
      groupBy: query.groupBy ?? null,
      subGroupBy: query.subGroupBy ?? null,
      perPage: query.perPage ?? GLOBAL_ISSUES_PAGE_SIZE,
      filters: query.filters ?? null,
    },
  ];
}

/**
 * The workspace-wide issue list, every joined project included, one
 * cursor page at a time. `next_cursor` is an opaque position the server
 * hands back; an empty cursor (or `next_page_results: false`) ends the
 * chain. The optional scope (a saved view's expression, a default's
 * static condition, an order) rides the request and the key; without it
 * the read is the plain all-issues list.
 */
export function globalIssuesQueryOptions(client: ApiClient, workspaceSlug: string, query: IssueListQuery = {}) {
  return infiniteQueryOptions({
    queryKey: [...issueKeys.all(workspaceSlug), "global", ...scopedGlobalKeyParts(query)] as const,
    queryFn: ({ pageParam }) =>
      // The cursor stays the page param's: a scope never carries one.
      listGlobalIssues(client, workspaceSlug, {
        perPage: query.perPage ?? GLOBAL_ISSUES_PAGE_SIZE,
        ...(query.orderBy === undefined ? {} : { orderBy: query.orderBy }),
        ...(query.groupBy === undefined ? {} : { groupBy: query.groupBy }),
        ...(query.subGroupBy === undefined ? {} : { subGroupBy: query.subGroupBy }),
        ...(query.filters === undefined ? {} : { filters: query.filters }),
        ...(pageParam === null ? {} : { cursor: pageParam }),
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (last: GlobalIssueListResponse): string | null => {
      if (!last.next_page_results) return null;
      const cursor = last.next_cursor.trim();
      return cursor === "" ? null : cursor;
    },
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** The server's total from the first page; zero before any page lands. */
export function globalTotalCount(data: { pages: GlobalIssueListResponse[] } | undefined): number {
  return data?.pages[0]?.total_count ?? 0;
}

/** Flatten cursor pages into one row list, dropping any repeated id. */
export function flattenGlobalPages(
  data: { pages: GlobalIssueListResponse[] } | undefined
): GlobalIssueListResponse["results"] {
  if (data === undefined) return [];
  const rows: GlobalIssueListResponse["results"] = [];
  const seen = new Set<string>();
  for (const page of data.pages) {
    for (const row of page.results) {
      if (seen.has(row.id)) continue;
      seen.add(row.id);
      rows.push(row);
    }
  }
  return rows;
}
