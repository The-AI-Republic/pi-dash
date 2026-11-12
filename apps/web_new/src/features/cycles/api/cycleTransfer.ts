// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer write (parity/specs/cycles.md §12, CYC-031). One hook for
// both entry points: selecting a destination moves the finished
// cycle's remaining work at once. Success confirms and refreshes both
// sides — the destination's rising counts and the source's banner,
// counts and ring — plus both cycles' work-item lists, without a
// reload (the NEWFRONT-267 intended behavior). A refusal reports and
// moves nothing, leaving the dialog's state intact.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { transferCycleIssues } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { entityListsKeyPart, issueKeys, type IssuesEntity } from "../../issues/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { invalidateAfterCycleTransfer } from "./invalidate.js";

/** Success title; the line carries the pinned `transferred successfully`. */
export const CYCLE_TRANSFERRED_TITLE = "Success!";
export const CYCLE_TRANSFERRED_LINE = "The remaining work items transferred successfully.";
/** Failure title; the dialog stays open for another pick. */
export const CYCLE_TRANSFER_FAILED_TITLE = "Unable to transfer work items. Please try again.";

export interface CycleTransferTarget {
  workspaceSlug: string;
  projectId: string;
  sourceCycleId: string;
}

/** Invalidate one cycle entity's work-item lists through the issues keys. */
export function cycleTransferListsKey(workspaceSlug: string, projectId: string, cycleId: string): readonly unknown[] {
  const entity: IssuesEntity = { scope: "cycle", projectId, cycleId };
  return issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity));
}

export function useTransferCycleWork(target: CycleTransferTarget): {
  /** Move the remainder to the destination; true when the move landed. */
  transfer: (destinationCycleId: string) => Promise<boolean>;
  pending: boolean;
} {
  const queryClient = useQueryClient();
  const mutation = useMutation<boolean, Error, string>({
    mutationFn: async (destinationCycleId) => {
      try {
        await transferCycleIssues(
          getAppClient(),
          target.workspaceSlug,
          target.projectId,
          target.sourceCycleId,
          destinationCycleId
        );
      } catch {
        notifyError(CYCLE_TRANSFER_FAILED_TITLE);
        return false;
      }
      notifySuccess(CYCLE_TRANSFERRED_TITLE, CYCLE_TRANSFERRED_LINE);
      await invalidateAfterCycleTransfer(
        queryClient,
        target.workspaceSlug,
        target.projectId,
        target.sourceCycleId,
        destinationCycleId
      );
      await queryClient.invalidateQueries({
        queryKey: cycleTransferListsKey(target.workspaceSlug, target.projectId, target.sourceCycleId),
      });
      await queryClient.invalidateQueries({
        queryKey: cycleTransferListsKey(target.workspaceSlug, target.projectId, destinationCycleId),
      });
      return true;
    },
  });

  return {
    transfer: (destinationCycleId: string) => mutation.mutateAsync(destinationCycleId),
    pending: mutation.isPending,
  };
}
