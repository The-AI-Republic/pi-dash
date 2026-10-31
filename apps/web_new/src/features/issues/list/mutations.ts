// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List inline writes (parity/specs/issues.md §2, ISS-014). Each editable
// strip property paints every cached copy of the row immediately, writes
// through the shared issue PATCH (or the cycle/module bridge), rolls back
// with a notice on failure and revalidates the entity's lists on settle.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  addIssueToCycle,
  removeIssueFromCycle,
  setIssueModules,
  updateIssue,
  type ApiClient,
  type IssuePatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { withIssueFields } from "../api/optimisticRows.js";

/** Failure notice for one inline property. */
export const LIST_PROPERTY_FAILED_TITLE = "Couldn't save the property";
const LIST_PROPERTY_FAILED_BODY = "The change did not stick — check your connection and try again.";

/** Optimistic single-field patch across every cached list/detail copy. */
export function useListIssuePatch(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, IssuePatch>({
    mutationFn: async (patch) => {
      const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
      for (const snapshot of snapshots) {
        queryClient.setQueryData(
          snapshot.queryKey,
          withIssueFields(snapshot.data, issueId, patch as Record<string, unknown>)
        );
      }
      try {
        await updateIssue(getAppClient(), workspaceSlug, projectId, issueId, patch);
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(LIST_PROPERTY_FAILED_TITLE, LIST_PROPERTY_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.lists(workspaceSlug, projectId) });
    },
  });
  return {
    isSaving: mutation.isPending,
    patchIssue: (patch: IssuePatch) => mutation.mutate(patch),
  };
}

/** Failure notice when a cycle attach is refused. */
export const LIST_CYCLE_FAILED_TITLE = "Couldn't add the work item to the cycle";

/** Optimistic cycle move: paints the new cycle, then the bridge write. */
export function useListCycleMove(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, { next: string | null; current: string | null }>({
    mutationFn: async ({ next, current }) => {
      const patch = { cycle_id: next };
      const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
      for (const snapshot of snapshots) {
        queryClient.setQueryData(snapshot.queryKey, withIssueFields(snapshot.data, issueId, patch));
      }
      try {
        if (next !== null) {
          await addIssueToCycle(getAppClient(), workspaceSlug, projectId, next, issueId);
        } else if (current !== null) {
          await removeIssueFromCycle(getAppClient(), workspaceSlug, projectId, current, issueId);
        }
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(LIST_CYCLE_FAILED_TITLE, LIST_PROPERTY_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.lists(workspaceSlug, projectId) });
    },
  });
  return {
    isSaving: mutation.isPending,
    moveToCycle: (next: string | null, current: string | null) => mutation.mutate({ next, current }),
  };
}

/** Failure notice when a module membership write is refused. */
export const LIST_MODULE_FAILED_TITLE = "Couldn't save the modules";

/** Optimistic module membership write through the per-issue bridge. */
export function useListModuleWrite(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, { modules: string[]; removed_modules?: string[] }>({
    mutationFn: async (input) => {
      const snapshots = snapshotQueries(queryClient, issueKeys.all(workspaceSlug));
      for (const snapshot of snapshots) {
        queryClient.setQueryData(
          snapshot.queryKey,
          withIssueFields(snapshot.data, issueId, { module_ids: input.modules })
        );
      }
      try {
        await setIssueModules(getAppClient(), workspaceSlug, projectId, issueId, input);
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(LIST_MODULE_FAILED_TITLE, LIST_PROPERTY_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.lists(workspaceSlug, projectId) });
    },
  });
  return {
    isSaving: mutation.isPending,
    writeModules: (modules: string[], removed: string[] = []) => mutation.mutate({ modules, removed_modules: removed }),
  };
}

/** Success notice shared by the create paths. */
export const LIST_CREATED_TITLE = "Work item created";

/** Attach helper used after creating inside a cycle or module context. */
export async function attachCreatedIssue(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  target: { cycleId?: string | undefined; moduleId?: string | undefined }
): Promise<void> {
  if (target.cycleId !== undefined) {
    await addIssueToCycle(client, workspaceSlug, projectId, target.cycleId, issueId);
  }
  if (target.moduleId !== undefined) {
    await setIssueModules(client, workspaceSlug, projectId, issueId, { modules: [target.moduleId] });
  }
  notifySuccess(LIST_CREATED_TITLE);
}
