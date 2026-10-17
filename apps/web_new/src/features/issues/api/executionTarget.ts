// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-target writes (parity/specs/issues.md §17, ISS-218). One
// PATCH carrying the pod pin plus the executor override, as mapped
// by the pod picker's selection. Applies optimistically across list
// and detail caches, rolls back with a toast on failure.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateIssue } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import type { ExecutionTargetPatch } from "../dropdowns/podOptions.js";
import { issueKeys } from "./keys.js";
import { withIssueFields } from "./optimisticRows.js";

/** Failure title when the execution-target write does not land. */
export const EXECUTION_TARGET_FAILED_TITLE = "Couldn't save the execution target";
const EXECUTION_TARGET_FAILED_BODY = "The execution target is unchanged — check your connection and try again.";

/** Persist one issue's execution target with optimistic apply and rollback. */
export function useUpdateExecutionTarget(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, ExecutionTargetPatch>({
    mutationFn: async (input) => {
      const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
      for (const snapshot of snapshots) {
        queryClient.setQueryData(
          snapshot.queryKey,
          withIssueFields(snapshot.data, issueId, {
            assigned_pod_id: input.assigned_pod_id,
            agent_executor: input.agent_executor,
          })
        );
      }
      try {
        await updateIssue(getAppClient(), workspaceSlug, projectId, issueId, {
          assigned_pod_id: input.assigned_pod_id,
          agent_executor: input.agent_executor,
        });
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(EXECUTION_TARGET_FAILED_TITLE, EXECUTION_TARGET_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
  return {
    isSaving: mutation.isPending,
    updateExecutionTarget: (input: ExecutionTargetPatch) => mutation.mutate(input),
  };
}
