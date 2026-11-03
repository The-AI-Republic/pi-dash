// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recents queries (SHELL-014/015; written from parity/specs/shell.md §2).
// One cache entry per filter key: selecting a filter reads (and refetches)
// its own entry. Home data never refetches on focus or reconnect
// (SHELL-023); a failed list fetch is logged and keeps prior state with
// no retry affordance. Components read through the hook, never the client.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { listRecentVisits, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { recentsEntityParam, type RecentFilter } from "../lib/recentRows.js";
import { logHomeFetch } from "./fetch.js";

export const recentsKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "home", "recents"] as const,
  list: (workspaceSlug: string, filter: RecentFilter) => [...recentsKeys.all(workspaceSlug), filter] as const,
};

export function recentsQueryOptions(client: ApiClient, workspaceSlug: string, filter: RecentFilter) {
  return queryOptions({
    queryKey: recentsKeys.list(workspaceSlug, filter),
    queryFn: () =>
      logHomeFetch(`recents/${workspaceSlug}/${filter}`, () =>
        listRecentVisits(client, workspaceSlug, recentsEntityParam(filter))
      ),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** Recent visits for one workspace under one filter key. */
export function useRecentVisits(workspaceSlug: string, filter: RecentFilter) {
  return useQuery(recentsQueryOptions(getAppClient(), workspaceSlug, filter));
}
