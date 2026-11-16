// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar membership writes (parity/specs/issues.md §13, ISS-155/156).
// Modules and cycles are bridges, not issue fields: a module toggle sends
// the add/remove pair to the issue-modules endpoint, and a cycle pick
// assigns through the cycle bridge or removes through it. Both paint the
// row immediately, roll back and notify on refusal, and expose a busy
// flag so the control locks while the request is in flight.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { addIssueToCycle, removeIssueFromCycle, setIssueModules } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "./keys.js";
import { pickerKeys } from "../../../shared/pickers/reference.js";

/** Failure title for a refused module change. */
export const MODULE_UPDATE_FAILED_TITLE = "Couldn't update the modules";
const MODULE_UPDATE_FAILED_BODY = "The modules are unchanged — check your connection and try again.";

/** Copy for the cycle bridge's outcome notices. */
export const CYCLE_ASSIGNED_TITLE = "Work item added to the cycle";
export const CYCLE_REMOVED_TITLE = "Work item removed from the cycle";
export const CYCLE_UPDATE_FAILED_TITLE = "Couldn't update the cycle";
export const CYCLE_ASSIGN_FAILED_BODY = "Work item could not be added to the cycle. Please try again.";
export const CYCLE_REMOVE_FAILED_BODY = "Work item could not be removed from the cycle. Please try again.";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Next cached shape with one issue's field patched, for detail and envelopes. */
function withRowField(value: unknown, issueId: string, field: string, next: unknown): unknown {
  if (Array.isArray(value)) return value.map((row) => withRowField(row, issueId, field, next));
  if (!isRecord(value)) return value;
  if (value["id"] === issueId) return { ...value, [field]: next };
  if (Array.isArray(value["results"])) {
    return { ...value, results: value["results"].map((row) => withRowField(row, issueId, field, next)) };
  }
  return value;
}

/** Paint one membership field across every cached copy; rollback restores it. */
function paintMembership(
  queryClient: ReturnType<typeof useQueryClient>,
  workspaceSlug: string,
  issueId: string,
  field: "cycle_id" | "module_ids",
  next: unknown
) {
  const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
  for (const snapshot of snapshots) {
    queryClient.setQueryData(snapshot.queryKey, withRowField(snapshot.data, issueId, field, next));
  }
  return snapshots;
}

export interface CycleWriteScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** The cycle the item sits in now, or null. */
  currentCycleId: string | null;
}

/**
 * Cycle row write. Unchanged picks never reach the network; a pick
 * assigns through the bridge, a clear removes through it, and both
 * directions confirm with a notice once the server agrees.
 */
export function useIssueCycleWrite(scope: CycleWriteScope) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, { cycleId: string | null }>({
    mutationFn: async ({ cycleId }) => {
      if (cycleId === scope.currentCycleId) return;
      const snapshots = paintMembership(queryClient, scope.workspaceSlug, scope.issueId, "cycle_id", cycleId);
      try {
        if (cycleId === null) {
          if (scope.currentCycleId !== null) {
            await removeIssueFromCycle(
              getAppClient(),
              scope.workspaceSlug,
              scope.projectId,
              scope.currentCycleId,
              scope.issueId
            );
          }
        } else {
          await addIssueToCycle(getAppClient(), scope.workspaceSlug, scope.projectId, cycleId, scope.issueId);
        }
        notifySuccess(cycleId === null ? CYCLE_REMOVED_TITLE : CYCLE_ASSIGNED_TITLE);
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        const body = cycleId === null ? CYCLE_REMOVE_FAILED_BODY : CYCLE_ASSIGN_FAILED_BODY;
        notifyError(CYCLE_UPDATE_FAILED_TITLE, body);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(scope.workspaceSlug) });
      void queryClient.invalidateQueries({ queryKey: pickerKeys.cycles(scope.workspaceSlug, scope.projectId) });
    },
  });
  return {
    isSavingCycle: mutation.isPending,
    updateCycle: (cycleId: string | null) => mutation.mutate({ cycleId }),
  };
}

export interface ModuleWriteScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/**
 * Module row write. The picker reports the full next selection; the
 * bridge only needs the ids that moved, so the diff rides here.
 */
export function useIssueModulesWrite(scope: ModuleWriteScope) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, { moduleIds: string[]; previousIds: string[] }>({
    mutationFn: async ({ moduleIds, previousIds }) => {
      const snapshots = paintMembership(queryClient, scope.workspaceSlug, scope.issueId, "module_ids", moduleIds);
      const removed = previousIds.filter((id) => !moduleIds.includes(id));
      try {
        await setIssueModules(getAppClient(), scope.workspaceSlug, scope.projectId, scope.issueId, {
          modules: moduleIds.filter((id) => !previousIds.includes(id)),
          ...(removed.length === 0 ? {} : { removed_modules: removed }),
        });
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(MODULE_UPDATE_FAILED_TITLE, MODULE_UPDATE_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(scope.workspaceSlug) });
      void queryClient.invalidateQueries({ queryKey: pickerKeys.modules(scope.workspaceSlug, scope.projectId) });
    },
  });
  return {
    isSavingModules: mutation.isPending,
    updateModules: (moduleIds: string[], previousIds: string[]) => mutation.mutate({ moduleIds, previousIds }),
  };
}
