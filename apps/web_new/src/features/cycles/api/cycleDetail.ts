// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Single-cycle detail read (NEWFRONT-859). The cycle detail route
// renders the stored start/end dates from this read; the cycles slices
// own the wider list/peek reads and the freshness helpers (CYC-045),
// which may adopt these keys when they land.

import { queryOptions, useQuery } from "@tanstack/react-query";

import type { ApiClient, CycleRow } from "@pidash/api-client";
import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/index.js";

export const cycleDetailKeys = {
  detail: (workspaceSlug: string, projectId: string, cycleId: string) =>
    [...workspaceScope(workspaceSlug), "cycles", "detail", projectId, cycleId] as const,
};

export function cycleDetailQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, cycleId: string) {
  return queryOptions<CycleRow>({
    queryKey: cycleDetailKeys.detail(workspaceSlug, projectId, cycleId),
    // The contract loads with the first read: eager route modules import
    // this module, and a static contract import would put the whole cycles
    // schema set on the initial chain (NEWFRONT-871, size gate).
    queryFn: async () => {
      const { getCycle } = await import("@pidash/api-client");
      return getCycle(client, workspaceSlug, projectId, cycleId);
    },
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

export function useCycleDetail(workspaceSlug: string, projectId: string, cycleId: string) {
  return useQuery(cycleDetailQueryOptions(getAppClient(), workspaceSlug, projectId, cycleId));
}
