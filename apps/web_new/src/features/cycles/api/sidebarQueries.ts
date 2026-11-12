// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar progress reads (parity/specs/cycles.md §14 and §23, CYC-038).
// The snapshot behind the counts and the per-measure breakdown
// distributions behind the entries and the burndown series. Keys come
// from the shared cycle key set so edits settle through the freshness
// module (invalidateCycleReads); the cloud-only detailed read stays out
// — the OSS backend serves no such route, and the snapshot plus the
// analytics completion chart carry the same sidebar.

import { queryOptions } from "@tanstack/react-query";
import { getCycleAnalytics, getCycleProgress, type ApiClient, type CycleAnalyticsType } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { cycleKeys } from "./keys.js";

/** Snapshot counts behind the rows and the state breakdown (CYC-038). */
export function cycleProgressQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  cycleId: string
) {
  return queryOptions({
    queryKey: cycleKeys.progress(workspaceSlug, projectId, cycleId),
    queryFn: () => getCycleProgress(client, workspaceSlug, projectId, cycleId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Breakdown distributions and burndown series for one measure (CYC-038). */
export function cycleAnalyticsQueryOptions(
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
