// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle favorite toggle (parity/specs/cycles.md §11, CYC-029). The row
// star flips at once, the write lands through the workspace favorites
// tree (POST/DELETE WS/user-favorites/ — never the project-scoped
// favorite-cycles endpoints), and the row re-reads after the toggle
// settles so the list and the sidebar agree. Both directions resolve
// against the live favorites list first, so a stale row can neither
// duplicate an entry nor delete a missing one. Progress shows while
// the write flies, then the outcome; a failure rolls the star back.
// The first add opens the sidebar favorites section live.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createFavorite, deleteFavorite, listFavorites, type CycleRow } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../../core/query/optimistic.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { isFavoritesSectionOpen, setFavoritesSectionOpen } from "../../../shared/shell/favorites.js";
import { notifyPending } from "../../../shared/shell/toaster.js";
import { cycleDetailKeys } from "./cycleDetail.js";
import { invalidateAfterCycleWrite } from "./invalidate.js";
import { cycleKeys } from "./keys.js";

/** Progress notice while the add/remove write flies. */
export const CYCLE_FAVORITE_ADDING_TITLE = "Adding to favorites…";
export const CYCLE_FAVORITE_REMOVING_TITLE = "Removing from favorites…";
/** Outcome notices; the oracle matches /added/ and /removed/ on them. */
export const CYCLE_FAVORITE_ADDED_TITLE = "Added to favorites";
export const CYCLE_FAVORITE_REMOVED_TITLE = "Removed from favorites";
/** Failure notice; the star rolls back with it. */
export const CYCLE_FAVORITE_FAILED_TITLE = "Couldn't update favorites";

export interface CycleFavoriteTarget {
  workspaceSlug: string;
  projectId: string;
  cycle: Pick<CycleRow, "id" | "name" | "is_favorite">;
}

/** Flip one cached row's mark; pure for tests. */
export function flipCycleFavorite<T extends { id: string; is_favorite: boolean }>(row: T | undefined): T | undefined {
  if (row === undefined) return undefined;
  return { ...row, is_favorite: !row.is_favorite };
}

/** Flip the matching row inside a cached list; pure for tests. */
export function flipCycleFavoriteInList<T extends { id: string; is_favorite: boolean }>(
  rows: readonly T[] | undefined,
  cycleId: string
): T[] | undefined {
  if (rows === undefined) return undefined;
  return rows.map((row) => (row.id === cycleId ? { ...row, is_favorite: !row.is_favorite } : row));
}

/**
 * Star/unstar a cycle. The display mark follows the cache (flipped
 * optimistically, settled by the re-read); `pending` pins the progress
 * state on the star while the write flies. Overlapping toggles are
 * refused: the second click waits for the first write to settle.
 */
export function useCycleFavorite(target: CycleFavoriteTarget): {
  starred: boolean;
  pending: boolean;
  toggle: () => void;
} {
  const queryClient = useQueryClient();
  const listKey = cycleKeys.list(target.workspaceSlug, target.projectId);
  const heroKey = cycleKeys.hero(target.workspaceSlug, target.projectId);
  const detailKey = cycleDetailKeys.detail(target.workspaceSlug, target.projectId, target.cycle.id);

  const mutation = useMutation<void, Error, Pick<CycleRow, "id" | "name" | "is_favorite">, { rollback: Rollback }>({
    mutationFn: async (cycle) => {
      const client = getAppClient();
      const notice = notifyPending(cycle.is_favorite ? CYCLE_FAVORITE_REMOVING_TITLE : CYCLE_FAVORITE_ADDING_TITLE);
      try {
        const favorites = await listFavorites(client, target.workspaceSlug);
        const existing = favorites.find((row) => row.entity_type === "cycle" && row.entity_identifier === cycle.id);
        if (cycle.is_favorite) {
          if (existing !== undefined) await deleteFavorite(client, target.workspaceSlug, existing.id);
          notice.settle({ title: CYCLE_FAVORITE_REMOVED_TITLE, description: `${cycle.name} is no longer a favorite.` });
        } else {
          if (existing === undefined) {
            await createFavorite(client, target.workspaceSlug, {
              entity_type: "cycle",
              entity_identifier: cycle.id,
              project_id: target.projectId,
              entity_data: { name: cycle.name },
            });
          }
          notice.settle({ title: CYCLE_FAVORITE_ADDED_TITLE, description: `${cycle.name} is now a favorite.` });
          if (!isFavoritesSectionOpen()) setFavoritesSectionOpen(true);
        }
      } catch (error) {
        notice.settle({
          title: CYCLE_FAVORITE_FAILED_TITLE,
          ...(error instanceof Error ? { description: error.message } : {}),
          failed: true,
        });
        throw error;
      }
    },
    onMutate: async (cycle) => {
      await queryClient.cancelQueries({ queryKey: listKey });
      await queryClient.cancelQueries({ queryKey: heroKey });
      await queryClient.cancelQueries({ queryKey: detailKey });
      const rollbackList = applyOptimisticUpdate(queryClient, listKey, (rows: CycleRow[] | undefined) =>
        flipCycleFavoriteInList(rows, cycle.id)
      );
      const rollbackHero = applyOptimisticUpdate(queryClient, heroKey, (rows: CycleRow[] | undefined) =>
        flipCycleFavoriteInList(rows, cycle.id)
      );
      const rollbackDetail = applyOptimisticUpdate(queryClient, detailKey, (row: CycleRow | undefined) =>
        flipCycleFavorite(row)
      );
      return {
        rollback: () => {
          rollbackList();
          rollbackHero();
          rollbackDetail();
        },
      };
    },
    onError: (_error, _cycle, context) => {
      context?.rollback();
    },
    onSettled: (_data, _error, cycle) => {
      // The re-read carries is_favorite, so the star, the row and the
      // sidebar agree; the sidebar list re-reads so the first add
      // surfaces without a reload.
      void invalidateAfterCycleWrite(queryClient, target.workspaceSlug, target.projectId, cycle.id);
      void queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(target.workspaceSlug) });
    },
  });

  // Mid-flight the mark is the optimistic target, whatever cache entry
  // the caller's row came from; settled it is the cache truth, which the
  // re-read keeps current.
  const starred =
    mutation.isPending && mutation.variables !== undefined ? !mutation.variables.is_favorite : target.cycle.is_favorite;
  return {
    starred,
    pending: mutation.isPending,
    toggle: () => {
      if (!mutation.isPending) mutation.mutate(target.cycle);
    },
  };
}
