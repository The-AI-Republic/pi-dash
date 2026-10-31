// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics state-breakdown read (parity/specs/analytics.md §16.3,
// ANL-013). One workspace-wide stats read narrowed by the picker's
// project_ids; no polling or fetch-policy override. The table keeps
// the server's row order.
import { queryOptions, useQuery } from "@tanstack/react-query";
import { getAnalyticsStats, type AnalyticsStats, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { normalizeAnalyticsScope } from "../lib/scope.js";
import { analyticsKeys } from "./keys.js";

export function analyticsStatsQueryOptions(client: ApiClient, workspaceSlug: string, projectIds: readonly string[]) {
  const normalized = normalizeAnalyticsScope(projectIds);
  return queryOptions({
    queryKey: analyticsKeys.stats(workspaceSlug, normalized),
    queryFn: (): Promise<AnalyticsStats> =>
      getAnalyticsStats(client, workspaceSlug, { type: "work-items", projectIds: normalized }),
  });
}

export function useAnalyticsStats(workspaceSlug: string, projectIds: readonly string[]) {
  return useQuery(analyticsStatsQueryOptions(getAppClient(), workspaceSlug, projectIds));
}
