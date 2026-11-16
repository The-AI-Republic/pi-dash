// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Own-history read (parity/specs/profile.md §7, PROF-019). The settings
// activity tab walks the caller's cross-workspace history one hundred
// rows at a time; a page only reads the next cursor when the member asks
// for more, and the server's own flag decides whether "Load more" shows.

import { infiniteQueryOptions, useInfiniteQuery } from "@tanstack/react-query";
import { listMyActivities, type ApiClient, type UserActivityPage } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";

export const OWN_ACTIVITY_PAGE_SIZE = 100;
export const OWN_ACTIVITY_FIRST_CURSOR = "";

export const ownActivityKeys = {
  pages: ["profile", "own-activity", "pages"] as const,
};

/** The next opaque cursor, or no continuation once the server is exhausted. */
export function nextOwnActivityCursor(page: UserActivityPage): string | undefined {
  return page.next_page_results && page.next_cursor !== "" ? page.next_cursor : undefined;
}

/** Every page of the caller's history, in server order. */
export function ownActivityPagesQueryOptions(client: ApiClient) {
  return infiniteQueryOptions({
    queryKey: ownActivityKeys.pages,
    queryFn: ({ pageParam }: { pageParam: string }) =>
      listMyActivities(client, {
        perPage: OWN_ACTIVITY_PAGE_SIZE,
        ...(pageParam === "" ? {} : { cursor: pageParam }),
      }),
    initialPageParam: OWN_ACTIVITY_FIRST_CURSOR,
    getNextPageParam: nextOwnActivityCursor,
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** Connected cursor stream used by the settings activity tab. */
export function useOwnActivityPages() {
  return useInfiniteQuery(ownActivityPagesQueryOptions(getAppClient()));
}
