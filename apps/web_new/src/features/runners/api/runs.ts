// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runs reads and the cancel write (NEWFRONT-545, RUN-016–019/RUN-021).
// The list page is part of its cache key so pages never share rows; the
// detail polls only while the run can still change, and both scopes send
// the same workspace id with an optional project narrow.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { cancelRun, getRun, listRuns, type AgentRun, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { runnersKeys, type RunnersScope } from "./keys.js";

/** The list refreshes on a short cadence so new runs appear without a reload (RUN-016). */
export const RUNS_POLL_INTERVAL_MS = 5_000;
/** The detail refreshes faster while the run is live, then stops (RUN-017). */
export const RUN_DETAIL_POLL_INTERVAL_MS = 3_000;
/** Page size the list asks for, matching the product's page envelope. */
export const RUNS_PAGE_SIZE = 30;

/** Statuses after which a run can no longer change. */
export const TERMINAL_RUN_STATUSES: readonly AgentRun["status"][] = [
  "blocked",
  "completed",
  "failed",
  "cancelled",
  "refused",
];

/** True once a run has settled and the detail should stop polling. */
export function isTerminalRun(status: AgentRun["status"]): boolean {
  return TERMINAL_RUN_STATUSES.includes(status);
}

/** Runs page for one scope and page (1-based). */
export function runsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  workspaceId: string,
  projectId: RunnersScope,
  page: number
) {
  return queryOptions({
    queryKey: runnersKeys.runs(workspaceSlug, projectId, page),
    queryFn: () =>
      listRuns(client, {
        workspaceId,
        page,
        perPage: RUNS_PAGE_SIZE,
        ...(projectId === null ? {} : { projectId }),
      }),
    staleTime: 0,
    refetchInterval: RUNS_POLL_INTERVAL_MS,
  });
}

/**
 * One run with its transcript. Polling follows the latest snapshot: live
 * runs refresh on the short cadence, terminal runs not at all (RUN-017/018).
 */
export function runQueryOptions(client: ApiClient, workspaceSlug: string, runId: string) {
  return queryOptions({
    queryKey: runnersKeys.run(workspaceSlug, runId),
    queryFn: () => getRun(client, runId, { includeEvents: true }),
    staleTime: 0,
    retry: false,
    refetchInterval: (query) => {
      const current = query.state.data;
      return current !== undefined && isTerminalRun(current.status) ? false : RUN_DETAIL_POLL_INTERVAL_MS;
    },
  });
}

/** The list for the current scope and page. */
export function useRuns(workspaceSlug: string, workspaceId: string, projectId: RunnersScope, page: number) {
  return useQuery(runsQueryOptions(getAppClient(), workspaceSlug, workspaceId, projectId, page));
}

/** One run's live detail; disabled until a run is selected. */
export function useRun(workspaceSlug: string, runId: string | null) {
  return useQuery({
    ...runQueryOptions(getAppClient(), workspaceSlug, runId ?? ""),
    enabled: runId !== null,
  });
}

/**
 * Cancel a run, then refresh the detail and every list page in the
 * workspace after the server answers (RUN-021). Failures reject so the
 * caller can toast and keep its confirmation open.
 */
export function useCancelRun(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { runId: string; reason?: string }) => cancelRun(getAppClient(), input.runId, input.reason),
    onSuccess: (run) => {
      queryClient.setQueryData(runnersKeys.run(workspaceSlug, run.id), run);
      return queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) });
    },
  });
}
