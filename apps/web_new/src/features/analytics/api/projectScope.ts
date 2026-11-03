// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scoped dialog reads (parity/specs/analytics.md §13, ANL-014). The dialog
// always talks to the project family — `/projects/{id}/advance-analytics`,
// `-charts` and `-stats` — and echoes the project id as `project_ids` as
// well, so the path and the query agree. A cycle or module narrowing rides
// along as `cycle_id` / `module_id`; the workspace reads are never used,
// which is what keeps the panel's numbers scoped to the surface it opened
// from. Like the tab reads these are plain on-mount queries: no polling and
// no fetch overrides, and a refusal settles into the section's empty state.
import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  getProjectAnalyticsChart,
  getProjectAnalyticsStats,
  getProjectAnalyticsTotals,
  type AnalyticsCustomChart,
  type AnalyticsProjectScope,
  type AnalyticsProjectStats,
  type AnalyticsWorkItemsChart,
  type AnalyticsWorkItemsTotals,
  type AnalyticsXAxis,
  type AnalyticsYAxis,
  type ApiClient,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import type { AnalyticsDialogScope } from "./keys.js";
import { analyticsKeys } from "./keys.js";

/**
 * The endpoint scope for one dialog opening. `projectIds` repeats the path
 * project because the server's filter reads the query parameter, and the
 * cycle wins over the module when a caller somehow supplies both, matching
 * the backend.
 */
export function dialogRequestScope(scope: AnalyticsDialogScope): AnalyticsProjectScope {
  return {
    projectIds: [scope.projectId],
    ...(scope.cycleId ? { cycleId: scope.cycleId } : {}),
    ...(scope.moduleId ? { moduleId: scope.moduleId } : {}),
  };
}

/** The five work-items cards for one project (plus cycle/module) scope. */
export function dialogTotalsQueryOptions(client: ApiClient, workspaceSlug: string, scope: AnalyticsDialogScope) {
  return queryOptions({
    queryKey: analyticsKeys.dialog(workspaceSlug, "totals", scope),
    queryFn: (): Promise<AnalyticsWorkItemsTotals> =>
      getProjectAnalyticsTotals(client, workspaceSlug, scope.projectId, dialogRequestScope(scope)),
  });
}

/** Totals for one dialog scope; idle until the panel opens. */
export function useDialogTotals(workspaceSlug: string, scope: AnalyticsDialogScope, enabled = true) {
  return useQuery({ ...dialogTotalsQueryOptions(getAppClient(), workspaceSlug, scope), enabled });
}

/** Created-vs-resolved buckets for the dialog scope. */
export function dialogWorkItemsChartQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  scope: AnalyticsDialogScope
) {
  return queryOptions({
    queryKey: analyticsKeys.dialog(workspaceSlug, "charts", scope),
    queryFn: (): Promise<AnalyticsWorkItemsChart> =>
      getProjectAnalyticsChart(
        client,
        workspaceSlug,
        scope.projectId,
        { type: "work-items" },
        dialogRequestScope(scope)
      ),
  });
}

/** Custom-insights bars for the dialog scope and one axis choice. */
export function dialogCustomChartQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  scope: AnalyticsDialogScope,
  selection: { xAxis: AnalyticsXAxis; yAxis: AnalyticsYAxis; groupBy?: AnalyticsXAxis | undefined }
) {
  const groupBy = selection.groupBy ?? null;
  return queryOptions({
    queryKey: analyticsKeys.dialogCustomChart(workspaceSlug, scope, selection.xAxis, selection.yAxis, groupBy),
    queryFn: (): Promise<AnalyticsCustomChart> =>
      getProjectAnalyticsChart(
        client,
        workspaceSlug,
        scope.projectId,
        {
          type: "custom-work-items",
          xAxis: selection.xAxis,
          yAxis: selection.yAxis,
          ...(groupBy === null ? {} : { groupBy }),
        },
        dialogRequestScope(scope)
      ),
  });
}

/** Per-assignee state counts for the dialog scope. */
export function dialogStatsQueryOptions(client: ApiClient, workspaceSlug: string, scope: AnalyticsDialogScope) {
  return queryOptions({
    queryKey: analyticsKeys.dialog(workspaceSlug, "stats", scope),
    queryFn: (): Promise<AnalyticsProjectStats> =>
      getProjectAnalyticsStats(client, workspaceSlug, scope.projectId, dialogRequestScope(scope)),
  });
}

/** Assignee stats for one dialog scope; idle until the panel opens. */
export function useDialogStats(workspaceSlug: string, scope: AnalyticsDialogScope, enabled = true) {
  return useQuery({ ...dialogStatsQueryOptions(getAppClient(), workspaceSlug, scope), enabled });
}
