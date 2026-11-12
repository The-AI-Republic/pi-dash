// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lifecycle writes (parity/specs/issues.md §11, ISS-134–136): archive,
// delete and move for one work item. Each mutation refreshes the
// workspace's issue reads on settle; none paints optimistically, because
// every outcome (the row leaves the list, the identifier changes) is
// owned by the server. Dialogs render the notices from rules.ts.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { archiveIssue, deleteIssue, moveIssueToProject, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";

/** The record a lifecycle write targets. */
export interface WorkItemScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/** Archive one work item; resolves with the stamped date. */
export function archiveWorkItem(client: ApiClient, scope: WorkItemScope): Promise<{ archived_at: string }> {
  return archiveIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId);
}

/** Delete one work item; gated by the server on creator-or-admin standing. */
export function deleteWorkItem(client: ApiClient, scope: WorkItemScope): Promise<void> {
  return deleteIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId);
}

/** Move one work item into another same-workspace project. */
export function moveWorkItem(client: ApiClient, scope: WorkItemScope, targetProject: string) {
  return moveIssueToProject(client, scope.workspaceSlug, scope.projectId, scope.issueId, targetProject);
}

/** The query helper every lifecycle write shares after it settles. */
function useLifecycleInvalidation(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return () => queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
}

/** Archive mutation for one record. */
export function useArchiveWorkItem(scope: WorkItemScope) {
  const invalidate = useLifecycleInvalidation(scope.workspaceSlug);
  return useMutation<{ archived_at: string }, Error, void>({
    mutationFn: () => archiveWorkItem(getAppClient(), scope),
    onSettled: () => void invalidate(),
  });
}

/** Delete mutation for one record. */
export function useDeleteWorkItem(scope: WorkItemScope) {
  const invalidate = useLifecycleInvalidation(scope.workspaceSlug);
  return useMutation<void, Error, void>({
    mutationFn: () => deleteWorkItem(getAppClient(), scope),
    onSettled: () => void invalidate(),
  });
}

/** Move mutation for one record; `mutateAsync(projectId)` answers the moved row. */
export function useMoveWorkItem(scope: WorkItemScope) {
  const invalidate = useLifecycleInvalidation(scope.workspaceSlug);
  return useMutation({
    mutationFn: (targetProject: string) => moveWorkItem(getAppClient(), scope, targetProject),
    onSettled: () => void invalidate(),
  });
}
