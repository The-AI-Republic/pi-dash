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
import { listGlobalIssues, type ApiClient, type GlobalIssueListResponse } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";

/** Rows per request for the workspace all-issues view. */
export const GLOBAL_ISSUES_PAGE_SIZE = 100;

/**
 * The workspace-wide issue list, every joined project included, one
 * cursor page at a time. `next_cursor` is an opaque position the server
 * hands back; an empty cursor (or `next_page_results: false`) ends the
 * chain.
 */
export function globalIssuesQueryOptions(client: ApiClient, workspaceSlug: string) {
  return infiniteQueryOptions({
    queryKey: [...issueKeys.all(workspaceSlug), "global"] as const,
    queryFn: ({ pageParam }) =>
      listGlobalIssues(client, workspaceSlug, {
        perPage: GLOBAL_ISSUES_PAGE_SIZE,
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
