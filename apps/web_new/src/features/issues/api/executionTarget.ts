// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-target writes (parity/specs/issues.md §17, ISS-218). One
// PATCH carrying the pod pin, the runner pin and the executor
// override, as mapped by the pod picker's selection. Applies
// optimistically across list and detail caches; validation failures
// report their field message in place, other failures roll back with
// a toast.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { isApiError, updateIssue } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import type { ExecutionTargetPatch } from "../../../shared/pickers/index.js";
import { issueKeys } from "./keys.js";
import { withIssueFields } from "./optimisticRows.js";

/** Failure title when the execution-target write does not land. */
export const EXECUTION_TARGET_FAILED_TITLE = "Couldn't save the execution target";
const EXECUTION_TARGET_FAILED_BODY = "The execution target is unchanged — check your connection and try again.";

/** Execution-target columns, in the order their messages win. */
const EXECUTION_TARGET_FIELDS = ["assigned_runner_id", "assigned_pod_id", "agent_executor"] as const;

/**
 * The API's validation message for an execution-target write, or null
 * when the failure is not a field rejection (design §7.1: the message
 * shows in place next to the picker).
 */
export function executionTargetFieldError(error: unknown): string | null {
  if (!isApiError(error) || error.status !== 400 || error.fields === undefined) return null;
  for (const field of EXECUTION_TARGET_FIELDS) {
    const first = error.fields[field]?.[0];
    if (first !== undefined) return first;
  }
  return null;
}

/** Persist one issue's execution target with optimistic apply and rollback. */
export function useUpdateExecutionTarget(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  const [validationError, setValidationError] = React.useState<string | null>(null);
  const mutation = useMutation<void, Error, ExecutionTargetPatch>({
    mutationFn: async (input) => {
      setValidationError(null);
      const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
      for (const snapshot of snapshots) {
        queryClient.setQueryData(
          snapshot.queryKey,
          withIssueFields(snapshot.data, issueId, {
            assigned_pod_id: input.assigned_pod_id,
            assigned_runner_id: input.assigned_runner_id,
            agent_executor: input.agent_executor,
          })
        );
      }
      try {
        await updateIssue(getAppClient(), workspaceSlug, projectId, issueId, {
          assigned_pod_id: input.assigned_pod_id,
          assigned_runner_id: input.assigned_runner_id,
          agent_executor: input.agent_executor,
        });
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        const fieldError = executionTargetFieldError(error);
        if (fieldError === null) {
          notifyError(EXECUTION_TARGET_FAILED_TITLE, EXECUTION_TARGET_FAILED_BODY);
        } else {
          setValidationError(fieldError);
        }
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
  return {
    isSaving: mutation.isPending,
    validationError,
    updateExecutionTarget: (input: ExecutionTargetPatch) => mutation.mutate(input),
  };
}
