// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail and transfer writes for the completed-cycle prompt
// (parity/specs/issues.md §18, ISS-224). The detail read carries the
// progress snapshot and the transferable counts; the transfer POSTs the
// target cycle and then re-reads both cycles so their counts settle.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getCycle, transferCycleIssues, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { pickerKeys } from "../../../shared/pickers/reference.js";

/** Keys for the transfer slice, under the workspace picker scope. */
export const transferKeys = {
  cycle: (workspaceSlug: string, projectId: string, cycleId: string) =>
    [...pickerKeys.all(workspaceSlug), "cycle-detail", projectId, cycleId] as const,
};

/** One cycle's detail: snapshot plus transferable counts. */
export function cycleDetailQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, cycleId: string) {
  return queryOptions({
    queryKey: transferKeys.cycle(workspaceSlug, projectId, cycleId),
    queryFn: () => getCycle(client, workspaceSlug, projectId, cycleId),
    // The prompt's state must follow a transfer immediately; the mutation
    // invalidates, and focus/reconnect never refresh it (ISS-221).
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** Read one cycle's detail for the transfer gate. */
export function useCycleDetail(workspaceSlug: string, projectId: string, cycleId: string, enabled: boolean) {
  return useQuery({ ...cycleDetailQueryOptions(getAppClient(), workspaceSlug, projectId, cycleId), enabled });
}

/** Success notice after the server accepted a transfer. */
export const TRANSFER_SUCCESS_TITLE = "Success!";
export const TRANSFER_SUCCESS_BODY = "Work items have been transferred successfully";
/** Failure notice when the transfer is refused. */
export const TRANSFER_FAILED_TITLE = "Error!";
export const TRANSFER_FAILED_BODY = "Unable to transfer work items. Please try again.";

/**
 * Move the source cycle's remaining issues to another cycle. On success
 * both cycles re-read (the source's snapshot freezes, the target's count
 * rises) and the picker list refreshes its names/statuses. A refusal
 * notices and leaves the surfaces as they were.
 */
export function useTransferCycleIssues(workspaceSlug: string, projectId: string, cycleId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (newCycleId: string) =>
      transferCycleIssues(getAppClient(), workspaceSlug, projectId, cycleId, newCycleId),
    onSuccess: (_result, newCycleId) => {
      notifySuccess(TRANSFER_SUCCESS_TITLE, TRANSFER_SUCCESS_BODY);
      void queryClient.invalidateQueries({ queryKey: transferKeys.cycle(workspaceSlug, projectId, cycleId) });
      void queryClient.invalidateQueries({ queryKey: transferKeys.cycle(workspaceSlug, projectId, newCycleId) });
      void queryClient.invalidateQueries({ queryKey: pickerKeys.cycles(workspaceSlug, projectId) });
    },
    onError: () => notifyError(TRANSFER_FAILED_TITLE, TRANSFER_FAILED_BODY),
  });
}
