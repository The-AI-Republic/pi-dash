// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle delete/archive/restore mutations (parity/specs/cycles.md §8
// and §9, CYC-022, CYC-024, CYC-026). One hook per write: each sends
// at most one request, toasts the pinned outcome strings, refreshes
// the shared cycle reads on success only, and resolves whether the
// write landed so callers close their dialog on success and keep it
// on failure. Delete and archive return to the plain live list (from
// detail, and dropping an open peek); restore stays where it is and
// reports failures honestly, never navigating on a failed write.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { archiveCycle, deleteCycle, isApiError, restoreCycle } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { cyclesListHref } from "../lib/cycleScreen.js";
import { invalidateAfterCycleWrite } from "./invalidate.js";

/** Delete success; the row and its group count leave with it. */
export const CYCLE_DELETED_TITLE = "Cycle deleted successfully.";
/** Delete failure that is not a permission refusal. */
export const CYCLE_DELETE_FAILED_TITLE = "Failed to delete cycle";
export const CYCLE_DELETE_FAILED_LINE = "The cycle is still there. Try again.";
/** A forbidden write maps here, never to the generic failure. */
export const CYCLE_PERMISSION_TITLE = "You don't have permission to perform this action.";

/** Archive success plus the pointer at project archives. */
export const CYCLE_ARCHIVE_TITLE = "Archive success";
export const CYCLE_ARCHIVE_LINE = "Your archives can be found in project archives.";
export const CYCLE_ARCHIVE_FAILED_TITLE = "Couldn't archive the cycle";
export const CYCLE_ARCHIVE_FAILED_LINE = "The cycle is still in the live list. Try again.";

/** Restore success; the cycle reads live again. */
export const CYCLE_RESTORED_TITLE = "Cycle restored";
export const CYCLE_RESTORED_LINE = "The cycle has been restored.";
/** Restore failure keeps the row archived and retryable (NEWFRONT-244 intended). */
export const CYCLE_RESTORE_FAILED_TITLE = "Cycle restore failed";
export const CYCLE_RESTORE_FAILED_LINE = "The cycle is still archived. Try again.";

export interface CycleMutationTarget {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
}

export interface CycleMutation {
  pending: boolean;
}

function useCycleMutationPending(): [boolean, () => boolean, () => void] {
  const [pending, setPending] = React.useState(false);
  // The ref guards overlapping calls: state alone would still let a
  // second call through before the re-render commits.
  const busy = React.useRef(false);
  const claim = (): boolean => {
    if (busy.current) return false;
    busy.current = true;
    setPending(true);
    return true;
  };
  const release = (): void => {
    busy.current = false;
    setPending(false);
  };
  return [pending, claim, release];
}

/**
 * Permanently remove the cycle. A 403 answers the dedicated
 * permission message; rows are never dropped optimistically.
 */
export function useDeleteCycle(target: CycleMutationTarget): CycleMutation & { remove: () => Promise<boolean> } {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [pending, claim, release] = useCycleMutationPending();

  const remove = async (): Promise<boolean> => {
    if (!claim()) return false;
    try {
      await deleteCycle(getAppClient(), target.workspaceSlug, target.projectId, target.cycleId);
    } catch (error) {
      release();
      if (isApiError(error) && error.status === 403) notifyError(CYCLE_PERMISSION_TITLE);
      else notifyError(CYCLE_DELETE_FAILED_TITLE, CYCLE_DELETE_FAILED_LINE);
      return false;
    }
    release();
    notifySuccess(CYCLE_DELETED_TITLE);
    await invalidateAfterCycleWrite(queryClient, target.workspaceSlug, target.projectId, target.cycleId);
    await navigate({ to: cyclesListHref(target.workspaceSlug, target.projectId), replace: true });
    return true;
  };
  return { remove, pending };
}

/** Archive the finished cycle; failures stay on the row with no navigation. */
export function useArchiveCycle(target: CycleMutationTarget): CycleMutation & { archive: () => Promise<boolean> } {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [pending, claim, release] = useCycleMutationPending();

  const archive = async (): Promise<boolean> => {
    if (!claim()) return false;
    try {
      await archiveCycle(getAppClient(), target.workspaceSlug, target.projectId, target.cycleId);
    } catch {
      release();
      notifyError(CYCLE_ARCHIVE_FAILED_TITLE, CYCLE_ARCHIVE_FAILED_LINE);
      return false;
    }
    release();
    notifySuccess(CYCLE_ARCHIVE_TITLE, CYCLE_ARCHIVE_LINE);
    await invalidateAfterCycleWrite(queryClient, target.workspaceSlug, target.projectId, target.cycleId);
    await navigate({ to: cyclesListHref(target.workspaceSlug, target.projectId), replace: true });
    return true;
  };
  return { archive, pending };
}

/**
 * Restore the archived cycle in place: success keeps the view where
 * it is so the row is seen leaving the archived list, and a failure
 * reports without navigating or dropping the row.
 */
export function useRestoreCycle(target: CycleMutationTarget): CycleMutation & { restore: () => Promise<boolean> } {
  const queryClient = useQueryClient();
  const [pending, claim, release] = useCycleMutationPending();

  const restore = async (): Promise<boolean> => {
    if (!claim()) return false;
    try {
      await restoreCycle(getAppClient(), target.workspaceSlug, target.projectId, target.cycleId);
    } catch {
      release();
      notifyError(CYCLE_RESTORE_FAILED_TITLE, CYCLE_RESTORE_FAILED_LINE);
      return false;
    }
    release();
    notifySuccess(CYCLE_RESTORED_TITLE, CYCLE_RESTORED_LINE);
    await invalidateAfterCycleWrite(queryClient, target.workspaceSlug, target.projectId, target.cycleId);
    return true;
  };
  return { restore, pending };
}
