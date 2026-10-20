// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item writes (parity/specs/issues.md §13). The detail fields save
// through useUpdateIssue: the detail, lite, and list caches paint
// immediately, roll back on failure with a notice, and revalidate on
// settle. The endpoint answers 204 with no body, so the re-read proves
// the write.

import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  isApiError,
  updateIssue,
  type ApiClient,
  type Issue,
  type IssueListResponse,
  type IssuePatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries, type QuerySnapshot } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import type { LiteResolution } from "./browse.js";
import { issueKeys } from "./keys.js";
import { withIssueFields } from "./optimisticRows.js";

/** Notice title when a title save is refused. */
export const TITLE_SAVE_FAILED_TITLE = "Couldn't save the title";
/** Notice title when a description save is refused. */
export const DESCRIPTION_SAVE_FAILED_TITLE = "Couldn't save the description";
/** Fallback detail when a failure carries no message. */
export const ISSUE_SAVE_FALLBACK = "Check your connection and try again.";

/** Server detail for a failure notice, falling back when empty. */
export function saveFailureDetail(error: unknown): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return ISSUE_SAVE_FALLBACK;
}

/**
 * Row fields a property patch may paint optimistically. `git_work_branch`
 * stays out (write-only), and every field named here is read back by the
 * detail and list projections, so a painted value matches the server row.
 */
const PATCHED_ROW_FIELDS = [
  "name",
  "state_id",
  "priority",
  "assignee_ids",
  "label_ids",
  "estimate_point",
  "start_date",
  "target_date",
  "parent_id",
  "sort_order",
] as const satisfies readonly (keyof IssuePatch)[];

function pickRowPatch(patch: IssuePatch): Record<string, unknown> {
  const picked: Record<string, unknown> = {};
  for (const field of PATCHED_ROW_FIELDS) {
    if (patch[field] !== undefined) picked[field] = patch[field];
  }
  return picked;
}

/** Next detail cache with the patch painted on. */
export function applyIssuePatchDetail(previous: Issue | undefined, patch: IssuePatch): Issue | undefined {
  if (previous === undefined) return undefined;
  const painted = pickRowPatch(patch);
  return {
    ...previous,
    ...painted,
    ...(patch.description_html !== undefined ? { description_html: patch.description_html } : {}),
  };
}

/** Next lite resolution with the patch painted on the matching row. */
export function applyIssuePatchLite(
  previous: LiteResolution | undefined,
  issueId: string,
  patch: IssuePatch
): LiteResolution | undefined {
  if (previous?.lite === undefined || previous.lite === null || previous.lite.id !== issueId) return previous;
  return {
    lite: withIssueFields(previous.lite, issueId, patch as Record<string, unknown>, {
      knownFieldsOnly: true,
    }) as LiteResolution["lite"],
  };
}

/** Next list page with the patched row fields painted on. */
export function applyIssuePatchList(
  previous: IssueListResponse | undefined,
  issueId: string,
  patch: IssuePatch
): IssueListResponse | undefined {
  if (previous === undefined) return previous;
  const painted = pickRowPatch(patch);
  if (Object.keys(painted).length === 0) return previous;
  return {
    ...previous,
    results: previous.results.map((row) => (row.id === issueId ? { ...row, ...painted } : row)),
  };
}

/**
 * Paint the patch on every cached copy of the item (detail, lite
 * resolutions, list pages). Returns the snapshots a rollback restores.
 */
export function paintIssuePatch(
  queryClient: QueryClient,
  workspaceSlug: string,
  issueId: string,
  patch: IssuePatch
): QuerySnapshot[] {
  const detailKey = issueKeys.detail(workspaceSlug, issueId);
  const browsePrefix = [...issueKeys.all(workspaceSlug), "browse"];
  const listPrefix = [...issueKeys.all(workspaceSlug), "list"];
  const snapshots = [
    ...snapshotQueries(queryClient, detailKey),
    ...snapshotQueries(queryClient, browsePrefix),
    ...snapshotQueries(queryClient, listPrefix),
  ];
  queryClient.setQueryData<Issue | undefined>(detailKey, (previous) => applyIssuePatchDetail(previous, patch));
  for (const snapshot of snapshotQueries(queryClient, browsePrefix)) {
    queryClient.setQueryData<LiteResolution | undefined>(snapshot.queryKey, (previous) =>
      applyIssuePatchLite(previous, issueId, patch)
    );
  }
  for (const snapshot of snapshotQueries(queryClient, listPrefix)) {
    queryClient.setQueryData<IssueListResponse | undefined>(snapshot.queryKey, (previous) =>
      applyIssuePatchList(previous, issueId, patch)
    );
  }
  return snapshots;
}

export interface UpdateIssueScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Notice title when the save is refused. */
  failureTitle: string;
}

/** Optimistic single-item PATCH with rollback, notice, and revalidate. */
export function useUpdateIssue(scope: UpdateIssueScope) {
  const client: ApiClient = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (patch: IssuePatch) =>
      updateIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId, patch),
    onMutate: async (patch) => {
      await queryClient.cancelQueries({ queryKey: issueKeys.detail(scope.workspaceSlug, scope.issueId) });
      return paintIssuePatch(queryClient, scope.workspaceSlug, scope.issueId, patch);
    },
    onError: (error: Error, _patch, snapshots) => {
      if (snapshots !== undefined) rollbackSnapshots(queryClient, snapshots);
      notifyError(scope.failureTitle, saveFailureDetail(error));
    },
    onSettled: (_row, _error, patch) => {
      const settled = [
        queryClient.invalidateQueries({ queryKey: issueKeys.detail(scope.workspaceSlug, scope.issueId) }),
        queryClient.invalidateQueries({ queryKey: [...issueKeys.all(scope.workspaceSlug), "browse"] }),
        queryClient.invalidateQueries({ queryKey: [...issueKeys.all(scope.workspaceSlug), "list"] }),
        // A property write lands as its own feed row (ISS-194).
        queryClient.invalidateQueries({
          queryKey: issueKeys.activity(scope.workspaceSlug, scope.projectId, scope.issueId),
        }),
      ];
      if (patch.description_html !== undefined) {
        settled.push(
          queryClient.invalidateQueries({
            queryKey: issueKeys.descriptionVersions(scope.workspaceSlug, scope.projectId, scope.issueId),
          })
        );
      }
      return Promise.all(settled).then(() => undefined);
    },
  });
}
