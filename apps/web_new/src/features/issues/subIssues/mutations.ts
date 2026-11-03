// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issue writes (parity/specs/issues.md §15, ISS-175–177). Attaching
// existing children, detaching one (parent id null), destroying one after
// the confirm, and one-field inline edits. Each paints the cached pages
// in place and rolls them back with a notice when the server refuses.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  addEpicWorkItems,
  addSubIssues,
  deleteIssue,
  updateIssue,
  type ApiClient,
  type AddedSubIssue,
  type IssuePatch,
  type SubIssueItem,
  type SubIssuesResponse,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries, type QuerySnapshot } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { failureDetail, subIssuesKey, type SubIssueScope } from "./queries.js";

/** Notice titles for each write. */
export const SUB_ISSUES_ADDED_TITLE = "Sub-work items added";
export const SUB_ISSUES_ADD_FAILED_TITLE = "Couldn't add the sub-work items";
export const SUB_ISSUE_REMOVED_TITLE = "Sub-work item removed";
export const SUB_ISSUE_REMOVE_FAILED_TITLE = "Couldn't remove the sub-work item";
export const SUB_ISSUE_DELETED_TITLE = "Work item deleted";
export const SUB_ISSUE_DELETE_FAILED_TITLE = "Couldn't delete the work item";
export const SUB_ISSUE_UPDATE_FAILED_TITLE = "Couldn't save the sub-work item";

/** Cached infinite envelopes for one parent. */
interface PageCache {
  pages: SubIssuesResponse[];
  pageParams?: unknown[];
}

function isPageCache(data: unknown): data is PageCache {
  return typeof data === "object" && data !== null && Array.isArray((data as PageCache).pages);
}

/** Snapshot and rewrite every page of one parent. */
function paintPages(
  queryClient: ReturnType<typeof useQueryClient>,
  key: readonly unknown[],
  rewrite: (page: SubIssuesResponse) => SubIssuesResponse
): QuerySnapshot[] {
  const snapshots = snapshotQueries(queryClient, key);
  for (const snapshot of snapshots) {
    if (!isPageCache(snapshot.data)) continue;
    queryClient.setQueryData(snapshot.queryKey, {
      ...snapshot.data,
      pages: snapshot.data.pages.map(rewrite),
    });
  }
  return snapshots;
}

/** Next page after removing one row: rows, distribution and total shrink. */
export function withRowRemoved(page: SubIssuesResponse, rowId: string): SubIssuesResponse {
  return {
    ...page,
    sub_issues: page.sub_issues.filter((row) => row.id !== rowId),
    state_distribution: Object.fromEntries(
      Object.entries(page.state_distribution).map(([group, ids]) => [group, ids.filter((id) => id !== rowId)])
    ),
    ...(page.total_count === undefined ? {} : { total_count: Math.max(0, page.total_count - 1) }),
  };
}

/** Next page after one field change on a row. */
export function withRowPatched(
  page: SubIssuesResponse,
  rowId: string,
  patch: Partial<SubIssueItem>
): SubIssuesResponse {
  return { ...page, sub_issues: page.sub_issues.map((row) => (row.id === rowId ? { ...row, ...patch } : row)) };
}

function stateGroupIn(distribution: Record<string, string[]>, rowId: string): string | null {
  for (const [group, ids] of Object.entries(distribution)) {
    if (ids.includes(rowId)) return group;
  }
  return null;
}

/**
 * Next first page after attaching rows: new rows append, the distribution
 * is the server's refreshed one, and the total grows by what is new.
 */
export function withRowsAttached(
  page: SubIssuesResponse,
  added: SubIssueItem[],
  distribution: Record<string, string[]>
): SubIssuesResponse {
  const known = new Set(page.sub_issues.map((row) => row.id));
  const fresh = added.filter((row) => !known.has(row.id));
  const total = page.total_count === undefined ? undefined : page.total_count + fresh.length;
  return {
    ...page,
    sub_issues: [...page.sub_issues, ...fresh],
    state_distribution: distribution,
    ...(total === undefined ? {} : { total_count: total }),
  };
}

/**
 * Rows the add response names, shaped as list rows. The POST answers
 * detail rows without the state group or the annotations a page carries;
 * derive the group from the distribution the answer brings.
 */
export function attachedRows(rows: readonly AddedSubIssue[], distribution: Record<string, string[]>): SubIssueItem[] {
  return rows.map((row) => ({
    ...row,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_by: null,
    updated_by: null,
    is_draft: false,
    state_group: stateGroupIn(distribution, row.id),
  }));
}

/** Attach existing issues as children (ISS-175). */
export function useAttachSubIssues(scope: SubIssueScope) {
  const queryClient = useQueryClient();
  const client: ApiClient = getAppClient();
  return useMutation({
    mutationFn: async (ids: string[]) =>
      scope.isEpic
        ? addEpicWorkItems(client, scope.workspaceSlug, scope.projectId, scope.parentId, ids)
        : addSubIssues(client, scope.workspaceSlug, scope.projectId, scope.parentId, ids),
    onSuccess: (answer, ids) => {
      const key = subIssuesKey(scope.workspaceSlug, scope.projectId, scope.parentId);
      queryClient.setQueryData(key, (previous: unknown) => {
        if (!isPageCache(previous) || previous.pages.length === 0) return previous;
        const [first, ...rest] = previous.pages;
        if (first === undefined) return previous;
        const rows = attachedRows(answer.sub_issues, answer.state_distribution);
        return {
          ...previous,
          pages: [
            {
              ...withRowsAttached(first, rows, answer.state_distribution),
              state_distribution: answer.state_distribution,
            },
            ...rest,
          ],
        };
      });
      notifySuccess(SUB_ISSUES_ADDED_TITLE, `${ids.length} ${ids.length === 1 ? "item" : "items"} attached.`);
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(scope.workspaceSlug) });
    },
    onError: (error: Error) => {
      notifyError(
        SUB_ISSUES_ADD_FAILED_TITLE,
        failureDetail(error, "Nothing was attached — check your connection and try again.")
      );
    },
  });
}

/** Detach one child without destroying it (ISS-176). */
export function useRemoveSubIssue(scope: SubIssueScope) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (row: SubIssueItem) => {
      const snapshots = paintPages(
        queryClient,
        subIssuesKey(scope.workspaceSlug, scope.projectId, scope.parentId),
        (page) => withRowRemoved(page, row.id)
      );
      try {
        await updateIssue(getAppClient(), scope.workspaceSlug, row.project_id, row.id, { parent_id: null });
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(
          SUB_ISSUE_REMOVE_FAILED_TITLE,
          failureDetail(error, "The child is unchanged — check your connection and try again.")
        );
        throw error;
      }
      return row;
    },
    onSuccess: (row) => {
      notifySuccess(SUB_ISSUE_REMOVED_TITLE, `${row.name} is no longer a sub-work item.`);
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(scope.workspaceSlug) });
    },
  });
}

/** Destroy one child after the confirm (ISS-176). */
export function useDeleteSubIssue(scope: SubIssueScope) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (row: SubIssueItem) => {
      const snapshots = paintPages(
        queryClient,
        subIssuesKey(scope.workspaceSlug, scope.projectId, scope.parentId),
        (page) => withRowRemoved(page, row.id)
      );
      try {
        await deleteIssue(getAppClient(), scope.workspaceSlug, row.project_id, row.id);
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(
          SUB_ISSUE_DELETE_FAILED_TITLE,
          failureDetail(error, "The item is unchanged — check your connection and try again.")
        );
        throw error;
      }
      return row;
    },
    onSuccess: (row) => {
      notifySuccess(SUB_ISSUE_DELETED_TITLE, `${row.name} was deleted.`);
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(scope.workspaceSlug) });
    },
  });
}

/** One-field inline edit on a row (ISS-177). */
export function usePatchSubIssue(scope: SubIssueScope) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: { row: SubIssueItem; patch: IssuePatch }) => {
      const snapshots = paintPages(
        queryClient,
        subIssuesKey(scope.workspaceSlug, scope.projectId, scope.parentId),
        (page) => withRowPatched(page, input.row.id, input.patch as Partial<SubIssueItem>)
      );
      try {
        await updateIssue(getAppClient(), scope.workspaceSlug, input.row.project_id, input.row.id, input.patch);
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(
          SUB_ISSUE_UPDATE_FAILED_TITLE,
          failureDetail(error, "The change did not land — check your connection and try again.")
        );
        throw error;
      }
      return input;
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(scope.workspaceSlug) });
    },
  });
}
