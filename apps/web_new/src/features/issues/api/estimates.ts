// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimate-point writes (parity/specs/issues.md §17, ISS-216). One
// PATCH carrying the point id, null from the picker's No estimate
// row. Applies optimistically across list and detail caches, rolls
// back with a toast on failure.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateIssue } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys } from "./keys.js";
import { withIssueFields } from "./optimisticRows.js";

/** Failure title when the estimate write does not land. */
export const ESTIMATE_POINT_FAILED_TITLE = "Couldn't save the estimate";
const ESTIMATE_POINT_FAILED_BODY = "The issue estimate is unchanged — check your connection and try again.";

/** Persist one issue's estimate point with optimistic apply and rollback. */
export function useUpdateEstimatePoint(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, string | null>({
    mutationFn: async (pointId) => {
      const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
      for (const snapshot of snapshots) {
        queryClient.setQueryData(
          snapshot.queryKey,
          withIssueFields(snapshot.data, issueId, { estimate_point: pointId })
        );
      }
      try {
        await updateIssue(getAppClient(), workspaceSlug, projectId, issueId, { estimate_point: pointId });
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(ESTIMATE_POINT_FAILED_TITLE, ESTIMATE_POINT_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
  return {
    isSaving: mutation.isPending,
    updateEstimatePoint: (pointId: string | null) => mutation.mutate(pointId),
  };
}
