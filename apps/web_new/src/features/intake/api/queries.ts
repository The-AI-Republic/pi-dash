// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake list read (parity/specs/intake.md INT-004/007/012/031, Data
// layer). One infinite read per tab, filter set and order; the tab's
// default status set is always named on the wire so the server never
// falls back to its own default. The read is fetch-triggered only: no
// focus refetch, no reconnect refetch, no polling interval. Slice B
// renders the pages and drives the filters; slice A needs the first
// page's ids and the read's total for the badge and auto-selection.

import { useInfiniteQuery, type QueryClient } from "@tanstack/react-query";
import {
  listIntakeIssues,
  type ApiClient,
  type IntakeIssueListResponse,
  type IntakeIssueRow,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { intakeKeys, INTAKE_DEFAULT_ORDER, type IntakeListFilters } from "./keys.js";
import { statusSetForTab, type IntakeTab } from "../lib/tabs.js";

/** Rows per page (INT-007). */
export const INTAKE_PAGE_SIZE = 10;

/**
 * The fetch policy every intake read shares (INT-031). The app default
 * already turns focus refetch off; intake states it explicitly so a
 * future default change cannot reintroduce a live re-read here.
 */
export const INTAKE_QUERY_POLICY = {
  staleTime: LIST_STALE_TIME_MS,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  refetchInterval: false,
  refetchIntervalInBackground: false,
} as const;

export interface IntakeListRead {
  tab: IntakeTab;
  /** Filters on top of the tab default; slice B owns building them. */
  filters?: IntakeListFilters | undefined;
  orderBy?: string | undefined;
  /** False while the feature gate shows: nothing may read then (INT-002). */
  enabled?: boolean | undefined;
}

/** The tab's status set followed by any widening the filter panel adds. */
export function listStatuses(tab: IntakeTab, filters: IntakeListFilters | undefined): readonly number[] {
  return filters?.statuses ?? statusSetForTab(tab);
}

function keyInput(read: IntakeListRead) {
  return {
    tab: read.tab,
    filters: { ...read.filters, statuses: listStatuses(read.tab, read.filters) },
    orderBy: read.orderBy ?? INTAKE_DEFAULT_ORDER,
  };
}

/** Query options for one tab's list, shaped for `useInfiniteQuery`. */
export function intakeListQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  read: IntakeListRead
) {
  const key = keyInput(read);
  return {
    queryKey: intakeKeys.list(workspaceSlug, projectId, key),
    queryFn: ({ pageParam }: { pageParam: string | undefined }) =>
      listIntakeIssues(client, workspaceSlug, projectId, {
        statuses: [...key.filters.statuses],
        priorities: key.filters.priorities === undefined ? undefined : [...key.filters.priorities],
        assignees: key.filters.assignees === undefined ? undefined : [...key.filters.assignees],
        createdBy: key.filters.createdBy === undefined ? undefined : [...key.filters.createdBy],
        labels: key.filters.labels === undefined ? undefined : [...key.filters.labels],
        createdAt: key.filters.createdAt,
        updatedAt: key.filters.updatedAt,
        orderBy: key.orderBy,
        cursor: pageParam,
        perPage: INTAKE_PAGE_SIZE,
      }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last: IntakeIssueListResponse) =>
      last.next_page_results && last.next_cursor !== "" ? last.next_cursor : undefined,
    ...INTAKE_QUERY_POLICY,
    enabled: read.enabled ?? true,
  };
}

/** The fetched rows across every loaded page, in server order. */
export function intakeRows(pages: readonly IntakeIssueListResponse[] | undefined): IntakeIssueRow[] {
  return (pages ?? []).flatMap((page) => page.results);
}

/** The active read's total (INT-004 badge source); absent before data lands. */
export function intakeTotal(pages: readonly IntakeIssueListResponse[] | undefined): number | undefined {
  return pages?.[0]?.total_results;
}

/** One tab's list for the screen. */
export function useIntakeList(workspaceSlug: string, projectId: string, read: IntakeListRead) {
  return useInfiniteQuery(intakeListQueryOptions(getAppClient(), workspaceSlug, projectId, read));
}

/**
 * Route-loader prefetch of a tab's first page. `ensureInfiniteQueryData`
 * reuses a fresh cache entry and swallows a failing read, so a denied or
 * offline project still renders the screen's own error instead of a
 * loader failure. Selection is not part of the key, so navigating
 * between requests never prefetches the list again.
 */
export async function prefetchIntakeList(
  queryClient: QueryClient,
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  read: IntakeListRead
): Promise<void> {
  await queryClient
    .ensureInfiniteQueryData(intakeListQueryOptions(client, workspaceSlug, projectId, read))
    .catch(() => undefined);
}
