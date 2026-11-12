// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item rename from the home preview (SHELL-016; written from
// parity/specs/shell.md §2 and §10). The peek panel's editable title
// commits through the thin work-item update the shell owns for renames;
// the issues epic reuses the panel with its own commit when full peek
// lands. The rename paints optimistically across every recents filter
// cache, rolls back with a failure notice on error, and refreshes the
// recents reads once settled.

import { mutationOptions, useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { updateIssue, type ApiClient, type RecentVisitList } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { recentsKeys } from "./recents.js";

function patchRecentsTitle(
  queryClient: QueryClient,
  workspaceSlug: string,
  issueId: string,
  title: string
): Rollback[] {
  const rollbacks: Rollback[] = [];
  const queries = queryClient.getQueryCache().findAll({ queryKey: recentsKeys.all(workspaceSlug) });
  for (const query of queries) {
    rollbacks.push(
      applyOptimisticUpdate<RecentVisitList>(queryClient, query.queryKey, (previous) =>
        previous?.map((visit) => {
          if (visit.entity_name !== "issue") return visit;
          const payload = visit.entity_data;
          if (payload === null || payload["id"] !== issueId) return visit;
          return { ...visit, entity_data: { ...payload, name: title } };
        })
      )
    );
  }
  return rollbacks;
}

export function previewRenameMutationOptions(
  client: ApiClient,
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string
) {
  return mutationOptions({
    mutationFn: (title: string) => updateIssue(client, workspaceSlug, projectId, issueId, { name: title }),
    onMutate: (title: string) => ({
      rollbacks: patchRecentsTitle(queryClient, workspaceSlug, issueId, title),
    }),
    onError: (error: unknown, _title: string, context: { rollbacks: Rollback[] } | undefined) => {
      context?.rollbacks.forEach((rollback) => {
        rollback();
      });
      notifyError("Work item rename failed", error instanceof Error ? error.message : undefined);
    },
    onSuccess: () => {
      notifySuccess("Work item renamed");
    },
    onSettled: () => {
      // Recents only: the issues area owns its reads' freshness, and
      // features never reach into another feature's keys.
      void queryClient.invalidateQueries({ queryKey: recentsKeys.all(workspaceSlug) });
    },
  });
}

/** Renames the previewed work item with an optimistic recents paint. */
export function usePreviewRename(workspaceSlug: string, projectId: string, issueId: string) {
  return useMutation(previewRenameMutationOptions(getAppClient(), useQueryClient(), workspaceSlug, projectId, issueId));
}
