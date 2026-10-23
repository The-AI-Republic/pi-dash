// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics project-stats reads (parity/specs/analytics.md §16.4,
// ANL-007). Ordinary on-mount reads: no polling and no fetch-policy
// overrides. The read carries the picker's project_ids scope like
// every other section read (a declared additive divergence, spec
// §7) and requests only the two valid fields the list consumes.
import { useQuery } from "@tanstack/react-query";
import { queryOptions } from "@tanstack/react-query";
import { getProjectStats, type ApiClient, type ProjectStatField, type ProjectStats } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { normalizeAnalyticsScope } from "../lib/scope.js";
import { analyticsKeys } from "./keys.js";

/** Fields the active-projects list consumes (spec §7). */
const PROJECT_STATS_FIELDS: readonly ProjectStatField[] = ["total_issues", "completed_issues"];

export function analyticsProjectStatsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectIds: readonly string[]
) {
  const normalized = normalizeAnalyticsScope(projectIds);
  return queryOptions({
    queryKey: analyticsKeys.projectStats(workspaceSlug, normalized),
    queryFn: (): Promise<ProjectStats> =>
      getProjectStats(client, workspaceSlug, { projectIds: normalized, fields: PROJECT_STATS_FIELDS }),
  });
}

/** Per-project totals under the current picker scope. */
export function useAnalyticsProjectStats(workspaceSlug: string, projectIds: readonly string[]) {
  return useQuery(analyticsProjectStatsQueryOptions(getAppClient(), workspaceSlug, projectIds));
}
