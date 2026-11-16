// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics headline totals reads (parity/specs/analytics.md §16.1,
// ANL-005, ANL-008). Ordinary on-mount reads: no polling, no loader
// prefetch, and no fetch-policy overrides — the app-wide client
// already skips focus refetch and 4xx retries. Guests receive 403
// and the grid settles to zeros (the query stays failed; the cards
// read missing metrics as zero).

import { useQuery } from "@tanstack/react-query";
import { queryOptions } from "@tanstack/react-query";
import { getAnalyticsTotals, type AnalyticsTotals, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { normalizeAnalyticsScope } from "../lib/scope.js";
import type { AnalyticsTabId } from "../lib/tabs.js";
import { analyticsKeys } from "./keys.js";

export function analyticsTotalsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  tab: AnalyticsTabId,
  projectIds: readonly string[]
) {
  const normalized = normalizeAnalyticsScope(projectIds);
  return queryOptions({
    queryKey: analyticsKeys.totals(workspaceSlug, tab, normalized),
    queryFn: (): Promise<AnalyticsTotals> => getAnalyticsTotals(client, workspaceSlug, { tab, projectIds: normalized }),
  });
}

/** One tab's headline totals under the current picker scope. */
export function useAnalyticsTotals(workspaceSlug: string, tab: AnalyticsTabId, projectIds: readonly string[]) {
  return useQuery(analyticsTotalsQueryOptions(getAppClient(), workspaceSlug, tab, projectIds));
}
