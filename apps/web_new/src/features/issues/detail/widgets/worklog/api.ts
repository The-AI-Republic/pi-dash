// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Worklog reads and writes (parity/specs/issues.md §16, ISS-206, cloud
// leg). The collection is per issue, newest first; logging takes the
// minutes, the day worked, and an optional note, while edits patch any
// of the three. Every write invalidates the collection, the item, and
// the activity feed: the server records a worklog activity the feed
// reads through its own key.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  deleteWorklogEntry,
  listWorklogEntries,
  logWorklogEntry,
  updateWorklogEntry,
  type ApiClient,
  type WorklogEntry,
  type WorklogEntryCreate,
  type WorklogEntryPatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../../../core/query/client.js";
import { notifyError, notifySuccess } from "../../../../../shared/shell/index.js";
import { issueKeys } from "../../../api/keys.js";
import {
  WORKLOG_DELETED_TITLE,
  WORKLOG_DELETE_FAILED_TITLE,
  WORKLOG_LOGGED_TITLE,
  WORKLOG_LOG_FAILED_TITLE,
  WORKLOG_UPDATED_TITLE,
  WORKLOG_UPDATE_FAILED_TITLE,
} from "./format.js";

export const worklogKeys = {
  all: (workspaceSlug: string, issueId: string) => [...issueKeys.detail(workspaceSlug, issueId), "worklogs"] as const,
  list: (workspaceSlug: string, projectId: string, issueId: string) =>
    [...worklogKeys.all(workspaceSlug, issueId), projectId] as const,
};

export function worklogQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, issueId: string) {
  return queryOptions({
    queryKey: worklogKeys.list(workspaceSlug, projectId, issueId),
    queryFn: () => listWorklogEntries(client, workspaceSlug, projectId, issueId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** The issue's logged time entries, newest first. */
export function useWorklogEntries(workspaceSlug: string, projectId: string, issueId: string) {
  return useQuery(worklogQueryOptions(getAppClient(), workspaceSlug, projectId, issueId));
}

function useInvalidateWorklog(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({ queryKey: worklogKeys.all(workspaceSlug, issueId) });
    void queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) });
    void queryClient.invalidateQueries({ queryKey: issueKeys.activity(workspaceSlug, projectId, issueId) });
  };
}

/** Log one entry, then refresh the collection, the item, and the feed. */
export function useLogWorklog(workspaceSlug: string, projectId: string, issueId: string) {
  const invalidate = useInvalidateWorklog(workspaceSlug, projectId, issueId);
  return useMutation<WorklogEntry, Error, WorklogEntryCreate>({
    mutationFn: (input) => logWorklogEntry(getAppClient(), workspaceSlug, projectId, issueId, input),
    onSuccess: () => {
      notifySuccess(WORKLOG_LOGGED_TITLE);
    },
    onError: () => {
      notifyError(WORKLOG_LOG_FAILED_TITLE);
    },
    onSettled: () => {
      invalidate();
    },
  });
}

export interface UpdateWorklogInput {
  entryId: string;
  patch: WorklogEntryPatch;
}

/** Edit one entry, then refresh the collection, the item, and the feed. */
export function useUpdateWorklogEntry(workspaceSlug: string, projectId: string, issueId: string) {
  const invalidate = useInvalidateWorklog(workspaceSlug, projectId, issueId);
  return useMutation<WorklogEntry, Error, UpdateWorklogInput>({
    mutationFn: (input) =>
      updateWorklogEntry(getAppClient(), workspaceSlug, projectId, issueId, input.entryId, input.patch),
    onSuccess: () => {
      notifySuccess(WORKLOG_UPDATED_TITLE);
    },
    onError: () => {
      notifyError(WORKLOG_UPDATE_FAILED_TITLE);
    },
    onSettled: () => {
      invalidate();
    },
  });
}

/** Remove one entry, then refresh the collection, the item, and the feed. */
export function useDeleteWorklogEntry(workspaceSlug: string, projectId: string, issueId: string) {
  const invalidate = useInvalidateWorklog(workspaceSlug, projectId, issueId);
  return useMutation<void, Error, string>({
    mutationFn: (entryId) => deleteWorklogEntry(getAppClient(), workspaceSlug, projectId, issueId, entryId),
    onSuccess: () => {
      notifySuccess(WORKLOG_DELETED_TITLE);
    },
    onError: () => {
      notifyError(WORKLOG_DELETE_FAILED_TITLE);
    },
    onSettled: () => {
      invalidate();
    },
  });
}
