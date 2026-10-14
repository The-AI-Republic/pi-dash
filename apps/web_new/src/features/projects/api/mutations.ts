// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project mutations (SHELL-032). The favorite star toggles through here:
// the card flips optimistically, rolls back on failure, and the first
// successful add opens the sidebar favorites section.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  addFavoriteProject,
  removeFavoriteProject,
  type ProjectDetails,
  type ProjectDetailsList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../../core/query/optimistic.js";
import {
  isFavoritesSectionOpen,
  notifyError,
  notifySuccess,
  setFavoritesSectionOpen,
} from "../../../shared/shell/index.js";
import { projectKeys } from "./keys.js";

export const FAVORITE_ADDED_TITLE = "Added to favorites";
export const FAVORITE_REMOVED_TITLE = "Removed from favorites";
export const FAVORITE_FAILED_TITLE = "Couldn't update favorites";

export interface ToggleFavoriteInput {
  workspaceSlug: string;
  project: ProjectDetails;
}

function flipStar(rows: ProjectDetailsList | undefined, projectId: string): ProjectDetailsList | undefined {
  return rows?.map((row) => (row.id === projectId ? { ...row, is_favorite: !row.is_favorite } : row));
}

/** The card mid-toggle, or null when no toggle is flying. Pure for tests. */
export function selectPendingProjectId(isPending: boolean, variables: ToggleFavoriteInput | undefined): string | null {
  if (!isPending) return null;
  return variables?.project.id ?? null;
}

/**
 * Star/unstar a project (SHELL-032): the card flips at once, the write
 * lands through the favorite endpoints, and the first add opens the
 * favorites section. A failed write toasts and rolls the card back.
 */
export function useToggleFavoriteProject() {
  const queryClient = useQueryClient();
  const mutation = useMutation<unknown, Error, ToggleFavoriteInput, { rollback: Rollback }>({
    mutationFn: async (input) => {
      const client = getAppClient();
      try {
        if (input.project.is_favorite) {
          const status = await removeFavoriteProject(client, input.workspaceSlug, input.project.id);
          notifySuccess(FAVORITE_REMOVED_TITLE, `${input.project.name} is no longer a favorite.`);
          return status;
        }
        const status = await addFavoriteProject(client, input.workspaceSlug, input.project.id);
        notifySuccess(FAVORITE_ADDED_TITLE, `${input.project.name} is now a favorite.`);
        if (!isFavoritesSectionOpen()) setFavoritesSectionOpen(true);
        return status;
      } catch (error) {
        notifyError(FAVORITE_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onMutate: async (input) => {
      const key = projectKeys.details(input.workspaceSlug);
      await queryClient.cancelQueries({ queryKey: key });
      return {
        rollback: applyOptimisticUpdate(queryClient, key, (rows: ProjectDetailsList | undefined) =>
          flipStar(rows, input.project.id)
        ),
      };
    },
    onError: (_error, _input, context) => {
      context?.rollback();
    },
    onSettled: (_data, _error, input) => {
      void queryClient.invalidateQueries({ queryKey: projectKeys.details(input.workspaceSlug) });
    },
  });
  return {
    ...mutation,
    /** The card mid-toggle, so the view pins the pending state on it. */
    pendingProjectId: selectPendingProjectId(mutation.isPending, mutation.variables),
  };
}
