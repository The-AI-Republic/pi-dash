// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item create submit (parity/specs/issues.md §11, ISS-117). One
// mutation behind every creation entry — the sidebar quick-create, the
// issues header, the palette command and the layout quick-adds. The
// caller hands over the whole form payload; the submit paints an
// optimistic temp row first (ISS-006), then confirms through the usual
// list invalidation or removes the temp row again.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createIssue, type IssueCreateInput, type IssueCreateResult } from "@pidash/api-client";

import { showToast } from "@pidash/kit";

import { getAppClient } from "../../../core/api/client.js";
import { getToastManager, notifyError } from "../../../shared/shell/index.js";
import { copyShareAddress, workItemPath } from "../../../shared/shell/shareLink.js";
import { createTempId } from "../freshness/fresh.js";
import { insertTempRow, removeTempRow, tempIssueRow } from "../freshness/tempRows.js";
import { executionTargetFieldError } from "./executionTarget.js";
import { issueKeys } from "./keys.js";

/** Success title after a work item lands. */
export const WORK_ITEM_CREATED_TITLE = "Work item created successfully";
/** Toast action that opens the new work item in another tab (ISS-131). */
export const CREATE_TOAST_VIEW_LABEL = "View work item";
/** Toast action that copies the new work item's address (ISS-131). */
export const CREATE_TOAST_COPY_LABEL = "Copy link";
/** What the copy action shows once the clipboard write lands (ISS-131). */
export const CREATE_TOAST_COPIED_LABEL = "Copied!";
/** Failure title when the work item does not land. */
export const WORK_ITEM_CREATE_FAILED_TITLE = "Couldn't create the work item";
const WORK_ITEM_CREATE_FAILED_BODY = "Nothing was created — check your connection and try again.";

/** One submit: the target project plus everything the dialog collected. */
export interface WorkItemCreateInput {
  projectId: string;
  payload: IssueCreateInput;
  /** Grouped section bucket for the optimistic row (ISS-011). */
  tempGroupKey?: string | undefined;
  /** Short code of the target project, so the toast can name the new key. */
  projectIdentifier?: string | undefined;
}

/**
 * Success notices for a landed create (ISS-131): the toast carries a
 * new-tab "View work item" link and a "Copy link" action that confirms
 * on the toast itself. Without a project code there is no address to
 * offer, so the plain notice stays.
 */
export function announceCreatedWorkItem(
  workspaceSlug: string,
  projectIdentifier: string | undefined,
  created: IssueCreateResult
): string {
  if (projectIdentifier === undefined || projectIdentifier === "") {
    return showToast(getToastManager(), { title: WORK_ITEM_CREATED_TITLE });
  }
  const path = workItemPath(workspaceSlug, `${projectIdentifier}-${created.sequence_id}`);
  return showToast(getToastManager(), {
    title: WORK_ITEM_CREATED_TITLE,
    actions: [
      { label: CREATE_TOAST_VIEW_LABEL, href: path },
      // The toast's own "Copied!" is the feedback; a failed write must not
      // claim success, so the copy resolves false and the label stays.
      {
        label: CREATE_TOAST_COPY_LABEL,
        confirmLabel: CREATE_TOAST_COPIED_LABEL,
        run: async () => {
          try {
            await copyShareAddress(path);
            return true;
          } catch {
            return false;
          }
        },
      },
    ],
  });
}

/** Post one work item, refreshing the workspace lists after. */
export function useCreateWorkItem(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<IssueCreateResult, Error, WorkItemCreateInput>({
    mutationFn: async (input) => {
      const tempId = createTempId();
      const listsPrefix = issueKeys.lists(workspaceSlug, input.projectId);
      insertTempRow(
        queryClient,
        listsPrefix,
        tempIssueRow(tempId, input.payload.name, {
          startDate: input.payload.start_date ?? null,
          targetDate: input.payload.target_date ?? null,
        }),
        input.tempGroupKey
      );
      try {
        return await createIssue(getAppClient(), workspaceSlug, input.projectId, input.payload);
      } catch (error) {
        removeTempRow(queryClient, listsPrefix, tempId);
        throw error;
      }
    },
    onSuccess: (created, input) => {
      announceCreatedWorkItem(workspaceSlug, input.projectIdentifier, created);
    },
    onError: (error) => {
      // Execution-target rejections report in place next to the picker
      // (design §7.1); the caller's own onError shows them. Every other
      // failure toasts here, as before.
      if (executionTargetFieldError(error) === null) {
        notifyError(WORK_ITEM_CREATE_FAILED_TITLE, WORK_ITEM_CREATE_FAILED_BODY);
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
}
