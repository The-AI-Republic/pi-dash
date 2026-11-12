// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parent reads and writes (parity/specs/issues.md §13, ISS-157/158). The
// Parent row and the banner name the item's parent through the shared
// detail read, and the sibling menu lists the parent's children through
// the sub-issues collection, excluding the item itself. Both parent
// changes go through the shared issue PATCH; once it settles the parent
// detail and its children are re-read so the row and banner agree with
// the server.

import { queryOptions, useQueryClient } from "@tanstack/react-query";
import { listSubIssues, type ApiClient, type SubIssuesResponse } from "@pidash/api-client";

import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "./keys.js";
import { useUpdateIssue } from "./mutations.js";
import { issueDetailQueryOptions } from "./queries.js";

/** Failure notice title for a refused parent change. */
export const PARENT_SAVE_FAILED_TITLE = "Couldn't update the work item";

/** One parent item, read through the shared detail endpoint. */
export function parentIssueQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, parentId: string) {
  return { ...issueDetailQueryOptions(client, workspaceSlug, projectId, parentId), enabled: parentId !== "" };
}

/** A parent's children, for the banner's sibling menu (ISS-158). */
export function parentSiblingsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  parentProjectId: string,
  parentId: string
) {
  return queryOptions({
    queryKey: [...issueKeys.all(workspaceSlug), "parent-siblings", parentId] as const,
    queryFn: () => listSubIssues(client, workspaceSlug, parentProjectId, parentId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** Siblings for the current item: every child of the parent except itself. */
export function siblingRows(
  subIssues: SubIssuesResponse["sub_issues"] | undefined,
  currentIssueId: string
): SubIssuesResponse["sub_issues"] {
  if (subIssues === undefined) return [];
  return subIssues.filter((row) => row.id !== currentIssueId);
}

export interface ParentWriteScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/**
 * Parent row and banner write. The PATCH paints the item at once through
 * the shared update hook; afterwards the parent's own detail and its
 * sub-issues are re-read so the banner and sibling menu are current.
 */
export function useParentWrite(scope: ParentWriteScope) {
  const queryClient = useQueryClient();
  const update = useUpdateIssue({
    workspaceSlug: scope.workspaceSlug,
    projectId: scope.projectId,
    issueId: scope.issueId,
    failureTitle: PARENT_SAVE_FAILED_TITLE,
  });

  async function setParent(parentId: string | null): Promise<void> {
    await update.mutateAsync({ parent_id: parentId });
    const settled = [
      queryClient.invalidateQueries({ queryKey: [...issueKeys.all(scope.workspaceSlug), "parent-siblings"] }),
    ];
    if (parentId !== null) {
      settled.push(queryClient.invalidateQueries({ queryKey: issueKeys.detail(scope.workspaceSlug, parentId) }));
    }
    await Promise.all(settled);
  }

  return { setParent, isSavingParent: update.isPending };
}
