// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar project order (parity/specs/shell.md §4, SHELL-051). A drop
// writes the dragged row's stored position with one PATCH; the list
// cache applies it optimistically, rolls back on failure with an
// error toast, and re-reads after settling.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProjectUserProperties, type ProjectList } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import { projectKeys } from "./keys.js";

/** Failure title the parity driver reads (SHELL-051). */
export const PROJECT_ORDER_FAILED_TITLE = "Something went wrong";
const PROJECT_ORDER_FAILED_BODY = "The new project order didn't save — the list is back to its previous order.";

/** Next list cache with the dragged row's stored position applied. */
export function applyProjectOrder(
  previous: ProjectList | undefined,
  projectId: string,
  sortOrder: number
): ProjectList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) => (row.id === projectId ? { ...row, sort_order: sortOrder } : row));
}

export interface ProjectOrderInput {
  projectId: string;
  sort_order: number;
}

/** Persist one dragged row's stored position with rollback on failure. */
export function useUpdateProjectOrder(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const key = projectKeys.list(workspaceSlug);
  return useMutation<void, Error, ProjectOrderInput>({
    mutationFn: async (input) => {
      const rollback = applyOptimisticUpdate<ProjectList>(queryClient, key, (previous) =>
        applyProjectOrder(previous, input.projectId, input.sort_order)
      );
      try {
        await updateProjectUserProperties(getAppClient(), workspaceSlug, input.projectId, {
          sort_order: input.sort_order,
        });
      } catch (error) {
        rollback();
        notifyError(PROJECT_ORDER_FAILED_TITLE, PROJECT_ORDER_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: key });
    },
  });
}
