// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Calendar writes (parity/specs/issues.md §4, ISS-024/026). Drag drops
// re-date one issue without touching its start date; quick-add posts a
// new issue already dated to the tile; add-existing moves a chosen issue
// onto the day. Every write reports failure through the shared toast.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createIssue, deleteIssue, updateIssue, type IssueListItem } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { decideDrop, type DropDecision } from "./model.js";

/** Failure copy for a refused or failed re-date. */
export const REDATE_FAILED_TITLE = "Couldn't move the work item";
/** Success copy for a drop that landed. */
export const REDATE_SUCCESS_TITLE = "Work item moved";
/** Failure copy when the per-day create does not land. */
export const CALENDAR_CREATE_FAILED_TITLE = "Couldn't add the work item";
/** Success copy when the per-day create lands. */
export const CALENDAR_CREATE_SUCCESS_TITLE = "Work item added";
/** Failure copy when an existing item cannot be moved onto a day. */
export const CALENDAR_ATTACH_FAILED_TITLE = "Couldn't add the work item to this day";
/** Success copy when existing items land on a day. */
export const CALENDAR_ATTACH_SUCCESS_TITLE = "Work items added";
/** Failure copy when a block's delete action fails. */
export const CALENDAR_DELETE_FAILED_TITLE = "Couldn't delete the work item";
/** Success copy when a block's delete action lands. */
export const CALENDAR_DELETE_SUCCESS_TITLE = "Work item deleted";

export interface RedateInput {
  issue: Pick<IssueListItem, "id" | "target_date" | "start_date">;
  destination: string;
}

/** Apply a drop decision without sending a no-op write. */
export function dropOutcome(input: RedateInput): DropDecision {
  return decideDrop(input.issue, input.destination);
}

/** Re-date one issue; no-op and rejection return before the network. */
export function useRedateIssue(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<DropDecision, Error, RedateInput>({
    mutationFn: async (input) => {
      const decision = dropOutcome(input);
      if (decision.kind !== "apply") return decision;
      await updateIssue(getAppClient(), workspaceSlug, projectId, input.issue.id, {
        target_date: decision.targetDate,
      });
      return decision;
    },
    onSuccess: (decision) => {
      if (decision.kind === "refuse") notifyError(REDATE_FAILED_TITLE, decision.reason);
      if (decision.kind === "apply") notifySuccess(REDATE_SUCCESS_TITLE);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.lists(workspaceSlug, projectId) });
    },
  });
  return {
    isSaving: mutation.isPending,
    reDate: (input: RedateInput) => mutation.mutate(input),
  };
}

export interface CalendarCreateInput {
  name: string;
  day: string;
}

/** Create a work item already dated to the tile that launched it. */
export function useCalendarCreate(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CalendarCreateInput) =>
      createIssue(getAppClient(), workspaceSlug, projectId, {
        name: input.name,
        target_date: input.day,
      }),
    onSuccess: () => {
      notifySuccess(CALENDAR_CREATE_SUCCESS_TITLE);
    },
    onError: () => {
      notifyError(CALENDAR_CREATE_FAILED_TITLE, "Nothing was created — check your connection and try again.");
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.lists(workspaceSlug, projectId) });
    },
  });
}

export interface CalendarAttachInput {
  issueIds: string[];
  day: string;
}

/** Add existing work items to a day by re-dating each selected item. */
export function useCalendarAttach(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CalendarAttachInput) =>
      Promise.all(
        input.issueIds.map((issueId) =>
          updateIssue(getAppClient(), workspaceSlug, projectId, issueId, { target_date: input.day })
        )
      ),
    onSuccess: () => {
      notifySuccess(CALENDAR_ATTACH_SUCCESS_TITLE);
    },
    onError: () => {
      notifyError(CALENDAR_ATTACH_FAILED_TITLE, "Nothing was changed — check your connection and try again.");
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.lists(workspaceSlug, projectId) });
    },
  });
}

/** Delete a work item from its block quick actions. */
export function useCalendarDelete(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (issueId: string) => deleteIssue(getAppClient(), workspaceSlug, projectId, issueId),
    onSuccess: () => {
      notifySuccess(CALENDAR_DELETE_SUCCESS_TITLE);
    },
    onError: () => {
      notifyError(CALENDAR_DELETE_FAILED_TITLE, "Nothing was deleted — check your permissions and try again.");
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.lists(workspaceSlug, projectId) });
    },
  });
}
