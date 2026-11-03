// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle progress and transfer writes for the completed-cycle prompt
// (parity/specs/issues.md §18, ISS-224). Status comes from the cycle
// picker row; the unfinished counts come from the progress endpoint,
// which is the only read carrying the per-group breakdown. After a
// transfer the server freezes the source's counts into its progress
// snapshot and this endpoint keeps serving them, so the source keeps
// reporting its old count and the prompt stays up.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getCycleProgress, transferCycleIssues, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { pickerKeys } from "../../../shared/pickers/reference.js";

/** Keys for the transfer slice, under the workspace picker scope. */
export const transferKeys = {
  progress: (workspaceSlug: string, projectId: string, cycleId: string) =>
    [...pickerKeys.all(workspaceSlug), "cycle-progress", projectId, cycleId] as const,
};

/** One cycle's progress counts, the transfer prompt's only count source. */
export function cycleProgressQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  cycleId: string
) {
  return queryOptions({
    queryKey: transferKeys.progress(workspaceSlug, projectId, cycleId),
    queryFn: () => getCycleProgress(client, workspaceSlug, projectId, cycleId),
    // Focus and reconnect never refresh issue data (ISS-221). The prompt
    // deliberately keeps this read: the source's frozen counts keep the
    // action up, and the target's own row is what a move refreshes.
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** Read one cycle's progress for the transfer gate. */
export function useCycleProgress(workspaceSlug: string, projectId: string, cycleId: string, enabled: boolean) {
  return useQuery({ ...cycleProgressQueryOptions(getAppClient(), workspaceSlug, projectId, cycleId), enabled });
}

/** Success notice after the server accepted a move. */
export const TRANSFER_SUCCESS_TITLE = "Work moved";
export const TRANSFER_SUCCESS_BODY = "The remaining work items now belong to the cycle you picked.";
/** Failure notice when the move is refused. */
export const TRANSFER_FAILED_TITLE = "Move failed";
export const TRANSFER_FAILED_BODY = "The chosen cycle did not take the work. Pick another one and retry.";

/**
 * Move the source cycle's remaining issues to another cycle. On success
 * the target re-reads its rising counts and the picker list refreshes its
 * names/statuses; the source is deliberately not re-read, because the
 * server froze its pre-move counts and the prompt keeps offering the
 * action (ISS-224). A refusal notices and leaves the surfaces as they
 * were.
 */
export function useTransferCycleIssues(workspaceSlug: string, projectId: string, cycleId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (newCycleId: string) =>
      transferCycleIssues(getAppClient(), workspaceSlug, projectId, cycleId, newCycleId),
    onSuccess: (_result, newCycleId) => {
      notifySuccess(TRANSFER_SUCCESS_TITLE, TRANSFER_SUCCESS_BODY);
      // The source row keeps its counts (the server froze them), so the
      // prompt and its action stay up; only the target's count changes.
      void queryClient.invalidateQueries({
        queryKey: transferKeys.progress(workspaceSlug, projectId, newCycleId),
      });
      void queryClient.invalidateQueries({ queryKey: pickerKeys.cycles(workspaceSlug, projectId) });
    },
    onError: () => notifyError(TRANSFER_FAILED_TITLE, TRANSFER_FAILED_BODY),
  });
}
