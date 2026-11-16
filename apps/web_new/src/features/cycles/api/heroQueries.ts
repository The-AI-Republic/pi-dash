// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-cycle hero reads (parity/specs/cycles.md §15 and §23,
// CYC-041). The hero names the cycle covering today through the
// only current-scoped list read, then progress, analytics, and the
// urgent/high issues pages behind the priority tab. Every read keys
// under the shared cycle keys so writes refresh the hero with the rest.

import { queryOptions } from "@tanstack/react-query";
import {
  getCycleAnalytics,
  getCycleProgress,
  listCycleIssues,
  listCycleRows,
  type ApiClient,
  type CycleAnalyticsType,
  type IssueListItem,
  type IssueListResponse,
} from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { cycleKeys } from "./keys.js";

/** Page width behind the priority tab. */
export const HERO_ISSUES_PAGE_SIZE = 100;

/** The legacy priority filter the priority tab reads through. */
export const HERO_PRIORITY_FILTER = "urgent,high";

/** The cycle covering today, if any; the server answers a bare array. */
export function heroQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: cycleKeys.hero(workspaceSlug, projectId),
    queryFn: () => listCycleRows(client, workspaceSlug, projectId, { cycleView: "current" }),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Per-group counts behind the progress card and the pending line. */
export function heroProgressQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, cycleId: string) {
  return queryOptions({
    queryKey: cycleKeys.progress(workspaceSlug, projectId, cycleId),
    queryFn: () => getCycleProgress(client, workspaceSlug, projectId, cycleId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Distribution and chart series behind the burn-down and breakdown cards. */
export function heroAnalyticsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  cycleId: string,
  type: CycleAnalyticsType
) {
  return queryOptions({
    queryKey: cycleKeys.analytics(workspaceSlug, projectId, cycleId, type),
    queryFn: () => getCycleAnalytics(client, workspaceSlug, projectId, cycleId, type),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/**
 * Urgent/high issues pages behind the priority tab, shaped for
 * `useInfiniteQuery`. The server filters on priority so older
 * urgent/high items outside the first unfiltered page still list;
 * paging follows `next_cursor` like the old active-cycle store.
 */
export function heroIssuesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, cycleId: string) {
  return {
    queryKey: cycleKeys.heroIssues(workspaceSlug, projectId, cycleId),
    queryFn: ({ pageParam }: { pageParam: string | undefined }) =>
      listCycleIssues(client, workspaceSlug, projectId, cycleId, {
        perPage: HERO_ISSUES_PAGE_SIZE,
        filters: { priority: HERO_PRIORITY_FILTER },
        ...(pageParam === undefined ? {} : { cursor: pageParam }),
      }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last: IssueListResponse) =>
      last.next_page_results && last.next_cursor !== "" ? last.next_cursor : undefined,
    staleTime: LIST_STALE_TIME_MS,
  };
}

/** The fetched rows across every loaded page, in server order. */
export function heroIssuesRows(pages: readonly IssueListResponse[] | undefined): IssueListItem[] {
  return (pages ?? []).flatMap((page) => page.results);
}
