// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request writes (parity/specs/intake.md INT-019/029, Data layer). Title,
// description and property edits PATCH the intake endpoint with the
// work-item fields nested under `issue`; triage moves (slice E) reuse
// this hook with the top-level intake fields. The detail cache paints
// immediately, rolls back on failure with a notice, and revalidates on
// settle; description writes also revalidate the shared versions list.

import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  updateIntakeIssue,
  type IntakeIssueDetail,
  type IntakeIssueDetailItem,
  type IntakeIssueUpdateInput,
  type IntakeIssueUpdateIssue,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries, type QuerySnapshot } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys, saveFailureDetail } from "../../issues/index.js";
import { intakeKeys } from "./keys.js";

function applyIntakePatchDetail(
  previous: IntakeIssueDetail | undefined,
  patch: IntakeIssueUpdateIssue
): IntakeIssueDetail | undefined {
  if (previous === undefined) return previous;
  // The contract types the painted priority loosely; the server holds
  // the enum, and the optimistic row mirrors the request verbatim.
  const painted = { ...previous.issue, ...patch } as IntakeIssueDetailItem;
  return { ...previous, issue: painted };
}

/**
 * Paint the patch on the cached detail. Returns the snapshots a
 * rollback restores.
 */
export function paintIntakeDetailPatch(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  patch: IntakeIssueUpdateIssue
): QuerySnapshot[] {
  const detailKey = intakeKeys.detail(workspaceSlug, projectId, issueId);
  const snapshots = snapshotQueries(queryClient, detailKey);
  queryClient.setQueryData<IntakeIssueDetail | undefined>(detailKey, (previous) =>
    applyIntakePatchDetail(previous, patch)
  );
  return snapshots;
}

export interface UpdateIntakeIssueScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Notice title when the save is refused. */
  failureTitle: string;
}

/** Optimistic intake PATCH with rollback, notice, and revalidate. */
export function useUpdateIntakeIssue(scope: UpdateIntakeIssueScope) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: IntakeIssueUpdateInput) =>
      updateIntakeIssue(client, scope.workspaceSlug, scope.projectId, scope.issueId, input),
    onMutate: async (input) => {
      await queryClient.cancelQueries({
        queryKey: intakeKeys.detail(scope.workspaceSlug, scope.projectId, scope.issueId),
      });
      return paintIntakeDetailPatch(
        queryClient,
        scope.workspaceSlug,
        scope.projectId,
        scope.issueId,
        input.issue ?? {}
      );
    },
    onError: (error: Error, _input, snapshots) => {
      if (snapshots !== undefined) rollbackSnapshots(queryClient, snapshots);
      notifyError(scope.failureTitle, saveFailureDetail(error));
    },
    onSettled: (_row, _error, input) => {
      const settled = [
        queryClient.invalidateQueries({
          queryKey: intakeKeys.detail(scope.workspaceSlug, scope.projectId, scope.issueId),
        }),
      ];
      if (input.issue?.description_html !== undefined) {
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
