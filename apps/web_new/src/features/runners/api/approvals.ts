// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-approval reads and the per-card decide write (NEWFRONT-547,
// RUN-023/RUN-024). The queue key splits by scope so the workspace
// aggregate and a project tab never share rows; deciding refreshes the
// area so the acted card drops off on refetch.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { decideApproval, listApprovals, type ApiClient, type ApprovalDecision } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { runnersKeys, type RunnersScope } from "./keys.js";

/** The queue re-polls on the fastest cadence in the area (RUN-023). */
export const APPROVALS_POLL_INTERVAL_MS = 2_000;

/** Pending approvals for one scope. */
export function approvalsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: RunnersScope) {
  return queryOptions({
    queryKey: runnersKeys.approvals(workspaceSlug, projectId),
    queryFn: () => listApprovals(client, projectId ?? undefined),
    staleTime: 0,
    refetchInterval: APPROVALS_POLL_INTERVAL_MS,
  });
}

/** The pending queue for the current scope. */
export function useApprovals(workspaceSlug: string, projectId: RunnersScope) {
  return useQuery(approvalsQueryOptions(getAppClient(), workspaceSlug, projectId));
}

/**
 * Decide one approval, then refresh the area after the server answers
 * (RUN-024). Failures reject so the caller can toast and keep the card.
 */
export function useDecideApproval(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { approvalId: string; decision: ApprovalDecision }) =>
      decideApproval(getAppClient(), input.approvalId, input.decision),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) }),
  });
}
