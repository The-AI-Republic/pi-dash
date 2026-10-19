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

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess, notifySuccessWithAction } from "../../../shared/shell/index.js";
import { createTempId } from "../freshness/fresh.js";
import { insertTempRow, removeTempRow, tempIssueRow } from "../freshness/tempRows.js";
import { issueKeys } from "./keys.js";

/** Success title after a work item lands. */
export const WORK_ITEM_CREATED_TITLE = "Work item created successfully";
/** Failure title when the work item does not land. */
export const WORK_ITEM_CREATE_FAILED_TITLE = "Couldn't create the work item";
const WORK_ITEM_CREATE_FAILED_BODY = "Nothing was created — check your connection and try again.";

/** Browse route for a created row, used by the toast's follow-up action. */
export function createdWorkItemPath(workspaceSlug: string, projectIdentifier: string, sequenceId: number): string {
  return `/${workspaceSlug}/browse/${projectIdentifier}-${sequenceId}`;
}

/** One submit: the target project plus everything the dialog collected. */
export interface WorkItemCreateInput {
  projectId: string;
  payload: IssueCreateInput;
  /** Grouped section bucket for the optimistic row (ISS-011). */
  tempGroupKey?: string | undefined;
  /** Identifier prefix for the toast's "View work item" follow-up. */
  projectIdentifier?: string | undefined;
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
      // The spec's ISS-011 toast offers a follow-up action. The list knows
      // the project prefix; callers without one keep the plain toast.
      if (input.projectIdentifier === undefined) {
        notifySuccess(WORK_ITEM_CREATED_TITLE);
        return;
      }
      const identifier = input.projectIdentifier;
      notifySuccessWithAction(WORK_ITEM_CREATED_TITLE, undefined, {
        actionLabel: "View work item",
        onAction: () => {
          globalThis.location.assign(createdWorkItemPath(workspaceSlug, identifier, created.sequence_id));
        },
      });
    },
    onError: () => {
      notifyError(WORK_ITEM_CREATE_FAILED_TITLE, WORK_ITEM_CREATE_FAILED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
}
