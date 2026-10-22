// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk write hook (parity/specs/issues.md §10, ISS-112–115). One request
// per action across the whole selection, then every list under the
// project is revalidated so grouping and counts settle. Update, subscribe
// and delete toasts name the outcome; archive reports the recoverable
// archive. Failures keep the selection and report a retryable notice.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  bulkArchiveIssues,
  bulkDeleteIssues,
  bulkSubscribeIssues,
  bulkUpdateIssues,
  isApiError,
  type BulkIssueProperties,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

/** Failure copy shared by the four bulk actions. */
export const BULK_FAILED_TITLE = "Couldn't apply the bulk action";
const BULK_FAILED_BODY = "Nothing changed — check your connection and try again.";

/** The message shown under the shared failure title. */
export function bulkFailureDescription(error: unknown): string {
  return isApiError(error) && error.message !== "" ? error.message : BULK_FAILED_BODY;
}

export interface BulkMutationContext {
  workspaceSlug: string;
  projectId: string;
}

export function useBulkMutations({ workspaceSlug, projectId }: BulkMutationContext) {
  const queryClient = useQueryClient();
  const refresh = (): void => {
    void queryClient.invalidateQueries({ queryKey: issueKeys.lists(workspaceSlug, projectId) });
  };
  const fail = (error: unknown): void => notifyError(BULK_FAILED_TITLE, bulkFailureDescription(error));

  const update = useMutation<void, unknown, { issueIds: string[]; properties: BulkIssueProperties }>({
    mutationFn: async ({ issueIds, properties }) => {
      await bulkUpdateIssues(getAppClient(), workspaceSlug, projectId, { issue_ids: issueIds, properties });
    },
    onSuccess: () => notifySuccess("Work items updated"),
    onError: fail,
    onSettled: refresh,
  });

  const remove = useMutation<void, unknown, string[]>({
    mutationFn: async (issueIds) => {
      await bulkDeleteIssues(getAppClient(), workspaceSlug, projectId, issueIds);
    },
    onSuccess: () => notifySuccess("Work items deleted"),
    onError: fail,
    onSettled: refresh,
  });

  const archive = useMutation<void, unknown, string[]>({
    mutationFn: async (issueIds) => {
      await bulkArchiveIssues(getAppClient(), workspaceSlug, projectId, issueIds);
    },
    onSuccess: () => notifySuccess("Work items archived", "Find them again under Archives."),
    onError: fail,
    onSettled: refresh,
  });

  const subscribe = useMutation<void, unknown, string[]>({
    mutationFn: async (issueIds) => {
      await bulkSubscribeIssues(getAppClient(), workspaceSlug, projectId, issueIds);
    },
    onSuccess: () => notifySuccess("Subscribed to work items"),
    onError: fail,
    onSettled: refresh,
  });

  return { update, remove, archive, subscribe };
}
