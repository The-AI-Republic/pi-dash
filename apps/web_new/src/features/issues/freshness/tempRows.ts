// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Optimistic temp rows (parity/specs/issues.md §1, ISS-006 / bug
// NEWFRONT-170). A create inserts a pulsing temp row keyed by a client
// temp id into every cached list under the entity prefix the moment it
// submits; the server confirm overwrites the cache through the usual
// invalidation, and failures remove the temp row again.

import type { QueryClient, QueryKey } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";

interface ListCacheData {
  results: IssueListItem[];
  total_count: number;
}

interface GroupBucket {
  results: IssueListItem[];
  total_results: number;
}

interface GroupedListCacheData {
  results: Record<string, GroupBucket>;
  total_count: number;
}

function asListData(value: unknown): ListCacheData | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const data = value as Partial<ListCacheData>;
  if (!Array.isArray(data.results) || typeof data.total_count !== "number") return undefined;
  return data as ListCacheData;
}

function asGroupedListData(value: unknown): GroupedListCacheData | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const data = value as Partial<GroupedListCacheData>;
  if (typeof data.results !== "object" || data.results === null || Array.isArray(data.results)) return undefined;
  if (typeof data.total_count !== "number") return undefined;
  return data as GroupedListCacheData;
}

/** A synthetic list row for an unconfirmed create; layouts overlay it. */
export function tempIssueRow(
  tempId: string,
  name: string,
  dates: { startDate: string | null; targetDate: string | null } = { startDate: null, targetDate: null }
): IssueListItem {
  return {
    id: tempId,
    name,
    start_date: dates.startDate,
    target_date: dates.targetDate,
    state_id: null,
    sort_order: 0,
    completed_at: null,
    priority: "none",
    sequence_id: 0,
    project_id: "",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };
}

/**
 * Prepend a temp row to every cached list under the prefix. The total
 * tracks the row so the empty router keeps rendering content while the
 * create pends; the confirm refetch overwrites the whole entry.
 */
export function insertTempRow(
  queryClient: QueryClient,
  listsPrefix: QueryKey,
  row: IssueListItem,
  groupKey?: string | undefined
): void {
  for (const query of queryClient.getQueryCache().findAll({ queryKey: listsPrefix })) {
    const raw = queryClient.getQueryData(query.queryKey);
    const data = asListData(raw);
    if (data !== undefined) {
      if (data.results.some((entry) => entry.id === row.id)) continue;
      queryClient.setQueryData<ListCacheData>(query.queryKey, {
        ...data,
        results: [row, ...data.results],
        total_count: data.total_count + 1,
      });
      continue;
    }
    const grouped = asGroupedListData(raw);
    if (grouped === undefined || groupKey === undefined) continue;
    const bucket = grouped.results[groupKey];
    if (bucket === undefined || bucket.results.some((entry) => entry.id === row.id)) continue;
    queryClient.setQueryData<GroupedListCacheData>(query.queryKey, {
      ...grouped,
      results: {
        ...grouped.results,
        [groupKey]: {
          ...bucket,
          results: [row, ...bucket.results],
          total_results: bucket.total_results + 1,
        },
      },
      total_count: grouped.total_count + 1,
    });
  }
}

/** Drop a temp row from every cached list under the prefix. */
export function removeTempRow(queryClient: QueryClient, listsPrefix: QueryKey, tempId: string): void {
  for (const query of queryClient.getQueryCache().findAll({ queryKey: listsPrefix })) {
    const raw = queryClient.getQueryData(query.queryKey);
    const data = asListData(raw);
    if (data !== undefined) {
      if (!data.results.some((entry) => entry.id === tempId)) continue;
      queryClient.setQueryData<ListCacheData>(query.queryKey, {
        ...data,
        results: data.results.filter((entry) => entry.id !== tempId),
        total_count: Math.max(0, data.total_count - 1),
      });
      continue;
    }
    const grouped = asGroupedListData(raw);
    if (grouped === undefined) continue;
    const entries = Object.entries(grouped.results);
    if (!entries.some(([, bucket]) => bucket.results.some((entry) => entry.id === tempId))) continue;
    const results = Object.fromEntries(
      entries.map(([key, bucket]) => {
        if (!bucket.results.some((entry) => entry.id === tempId)) return [key, bucket];
        return [
          key,
          {
            ...bucket,
            results: bucket.results.filter((entry) => entry.id !== tempId),
            total_results: Math.max(0, bucket.total_results - 1),
          },
        ];
      })
    );
    queryClient.setQueryData<GroupedListCacheData>(query.queryKey, {
      ...grouped,
      results,
      total_count: Math.max(0, grouped.total_count - 1),
    });
  }
}
