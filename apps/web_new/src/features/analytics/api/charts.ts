// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics projects-chart reads (parity/specs/analytics.md §16.2,
// ANL-006). Ordinary on-mount reads: no polling and no fetch-policy
// overrides — the app-wide client already skips focus refetch and
// 4xx retries. Guests receive 403 and the section keeps its
// headers with no plot (the query stays failed; the section reads
// the failure as its headers-only state).
import { useQuery } from "@tanstack/react-query";
import { queryOptions } from "@tanstack/react-query";
import { getAnalyticsChart, type AnalyticsProjectsChart, type ApiClient } from "@pidash/api-client";

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
