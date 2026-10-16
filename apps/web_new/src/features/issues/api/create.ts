// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quick-create submit (parity/specs/shell.md §4, SHELL-047). Posts the
// dialog's title to the target project, toasts the outcome, and
// refreshes the workspace's issue lists. The issues epic extends the
// dialog with the full field set; the entry point stays shared.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createIssue, type IssueCreateResult } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "./keys.js";

/** Success title after the quick-create lands. */
export const WORK_ITEM_CREATED_TITLE = "Work item created";
const WORK_ITEM_CREATED_BODY = "It is at the top of the project's list.";
/** Failure title when the quick-create does not land. */
export const WORK_ITEM_CREATE_FAILED_TITLE = "Couldn't create the work item";
const WORK_ITEM_CREATE_FAILED_BODY = "Nothing was created — check your connection and try again.";

export interface WorkItemCreateInput {
  projectId: string;
  name: string;
}

/** Post one quick-create title, refreshing the workspace lists after. */
export function useCreateWorkItem(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<IssueCreateResult, Error, WorkItemCreateInput>({
    mutationFn: (input) => createIssue(getAppClient(), workspaceSlug, input.projectId, { name: input.name }),
    onSuccess: () => {
      notifySuccess(WORK_ITEM_CREATED_TITLE, WORK_ITEM_CREATED_BODY);
    },
    onError: () => {
      notifyError(WORK_ITEM_CREATE_FAILED_TITLE, WORK_ITEM_CREATE_FAILED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
}
