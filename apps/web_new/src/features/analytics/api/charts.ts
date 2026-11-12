// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics chart reads (parity/specs/analytics.md §16.2, ANL-006,
// ANL-009–011). Ordinary on-mount reads: no polling and no
// fetch-policy overrides — the app-wide client already skips focus
// refetch and 4xx retries. Guests receive 403 and the section
// keeps its headers with no plot (the query stays failed; the
// section reads the failure as its empty state).
import { useQuery } from "@tanstack/react-query";
import { queryOptions } from "@tanstack/react-query";
import {
  getAnalyticsChart,
  type AnalyticsCustomChart,
  type AnalyticsProjectsChart,
  type AnalyticsWorkItemsChart,
  type AnalyticsXAxis,
  type AnalyticsYAxis,
  type ApiClient,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { normalizeAnalyticsScope } from "../lib/scope.js";
import { analyticsKeys } from "./keys.js";

export function analyticsProjectsChartQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectIds: readonly string[]
) {
  const normalized = normalizeAnalyticsScope(projectIds);
  return queryOptions({
    queryKey: analyticsKeys.charts(workspaceSlug, "projects", normalized),
    queryFn: (): Promise<AnalyticsProjectsChart> =>
      getAnalyticsChart(client, workspaceSlug, { type: "projects", projectIds: normalized }),
  });
}

/** Per-project radar rows under the current picker scope. */
export function useAnalyticsProjectsChart(workspaceSlug: string, projectIds: readonly string[]) {
  return useQuery(analyticsProjectsChartQueryOptions(getAppClient(), workspaceSlug, projectIds));
}

/** Created-vs-resolved buckets under the current picker scope (NEWFRONT-733, ANL-009). */
export function analyticsWorkItemsChartQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectIds: readonly string[]
) {
  const normalized = normalizeAnalyticsScope(projectIds);
  return queryOptions({
    queryKey: analyticsKeys.charts(workspaceSlug, "work-items", normalized),
    queryFn: (): Promise<AnalyticsWorkItemsChart> =>
      getAnalyticsChart(client, workspaceSlug, { type: "work-items", projectIds: normalized }),
  });
}

/** Created-vs-resolved buckets under the current picker scope. */
export function useAnalyticsWorkItemsChart(workspaceSlug: string, projectIds: readonly string[]) {
  return useQuery(analyticsWorkItemsChartQueryOptions(getAppClient(), workspaceSlug, projectIds));
}

/** One custom-graph axis choice (NEWFRONT-733, ANL-010). */
export interface AnalyticsCustomChartSelection {
  xAxis: AnalyticsXAxis;
  yAxis: AnalyticsYAxis;
  /** Second split; omitted from the request when absent. */
  groupBy?: AnalyticsXAxis | undefined;
}

/**
 * Custom-graph rows under the current picker scope and axis choice.
 * The axes ride the key, so every re-pick reads (or reuses) its own
 * cache entry.
 */
export function analyticsCustomChartQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectIds: readonly string[],
  selection: AnalyticsCustomChartSelection
) {
  const normalized = normalizeAnalyticsScope(projectIds);
  const groupBy = selection.groupBy ?? null;
  return queryOptions({
    queryKey: analyticsKeys.customCharts(workspaceSlug, normalized, selection.xAxis, selection.yAxis, groupBy),
    queryFn: (): Promise<AnalyticsCustomChart> =>
      getAnalyticsChart(client, workspaceSlug, {
        type: "custom-work-items",
        xAxis: selection.xAxis,
        yAxis: selection.yAxis,
        ...(groupBy === null ? {} : { groupBy }),
        projectIds: normalized,
      }),
  });
}

/** Custom-graph rows under the current picker scope and axis choice. */
export function useAnalyticsCustomChart(
  workspaceSlug: string,
  projectIds: readonly string[],
  selection: AnalyticsCustomChartSelection
) {
  return useQuery(analyticsCustomChartQueryOptions(getAppClient(), workspaceSlug, projectIds, selection));
}
