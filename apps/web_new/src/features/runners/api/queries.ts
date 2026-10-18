// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners reads (NEWFRONT-540, RUN-002/RUN-004/RUN-012). The overview table
// and the side rail read the same list through different hooks: the table
// uses `useRunners`, the rail `useRunnersRail`, which share the cache entry
// but carry their own polling cadence. Pods poll slower than runners.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { listPods, listRunners, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { runnersKeys, type RunnersScope } from "./keys.js";

/** How often the runner list re-reads while mounted (RUN-002). */
export const RUNNERS_POLL_INTERVAL_MS = 5_000;
/** The pod section moves slower; pods change on human actions (RUN-012). */
export const PODS_POLL_INTERVAL_MS = 30_000;

/**
 * The runner list for one scope. A project scope narrows server-side; the
 * workspace scope is the whole aggregate. Always live data: the list
 * states appear without navigation, so no stale window applies.
 */
export function runnersQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  workspaceId: string,
  projectId: RunnersScope
) {
  return queryOptions({
    queryKey: runnersKeys.list(workspaceSlug, projectId),
    queryFn: () => listRunners(client, workspaceId, projectId === null ? {} : { projectId }),
    staleTime: 0,
    refetchInterval: RUNNERS_POLL_INTERVAL_MS,
  });
}

/**
 * Pods for one scope. The project filter wins server-side when both are
 * sent, so the scoped read passes only the project id; the aggregate read
 * passes the workspace. Failures surface as the section's error line.
 */
export function runnersPodsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  workspaceId: string,
  projectId: RunnersScope
) {
  return queryOptions({
    queryKey: runnersKeys.pods(workspaceSlug, projectId),
    queryFn: () => listPods(client, projectId === null ? { workspaceId } : { projectId }),
    staleTime: 0,
    refetchInterval: PODS_POLL_INTERVAL_MS,
  });
}

/** Active runner list for the current scope. */
export function useRunners(workspaceSlug: string, workspaceId: string, projectId: RunnersScope, enabled = true) {
  return useQuery({ ...runnersQueryOptions(getAppClient(), workspaceSlug, workspaceId, projectId), enabled });
}

/**
 * Rail contacts (RUN-004). Shares the list cache with the table but
 * polls on its own short cadence, so a contact appears even when the
 * table is not mounted (child routes keep the rail).
 */
export function useRunnersRail(workspaceSlug: string, workspaceId: string, projectId: RunnersScope, enabled = true) {
  return useQuery({
    ...runnersQueryOptions(getAppClient(), workspaceSlug, workspaceId, projectId),
    refetchInterval: RUNNERS_POLL_INTERVAL_MS,
    enabled,
  });
}

/** Pod tiles for the current scope (RUN-012). */
export function useRunnersPods(workspaceSlug: string, workspaceId: string, projectId: RunnersScope, enabled = true) {
  return useQuery({ ...runnersPodsQueryOptions(getAppClient(), workspaceSlug, workspaceId, projectId), enabled });
}
