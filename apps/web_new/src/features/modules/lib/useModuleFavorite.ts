// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module favorite toggle (parity/specs/modules.md §4.4, MOD-016). The row
// and card star flips at once, the write lands through the module
// favorites endpoints (POST/DELETE user-favorite-modules/), and the
// module lists plus the sidebar favorites tree re-read after the toggle
// settles so the row, the Favorites filter and the sidebar agree. A
// failure rolls the star back with an error notice. The first add opens
// the sidebar favorites section live through the shell's flag — the same
// seam every other area's star uses, never a module-specific sidebar.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { favoriteModule, unfavoriteModule, type ModuleRow } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { isFavoritesSectionOpen, setFavoritesSectionOpen } from "../../../shared/shell/favorites.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { invalidateAfterModuleWrite } from "../api/invalidate.js";
import { moduleKeys } from "../api/keys.js";
import { MODULE_FAILURE_TITLE, MODULE_SUCCESS_TITLE, moduleFailureBody } from "./moduleForm.js";

/** Pinned outcome notices; the oracle matches them literally. */
export const MODULE_FAVORITE_ADDED = "Module added to favorites.";
export const MODULE_FAVORITE_REMOVED = "Module removed from favorites.";

export interface ModuleFavoriteTarget {
  workspaceSlug: string;
  projectId: string;
  module: Pick<ModuleRow, "id" | "name" | "is_favorite">;
}

/** Flip one cached row's mark; pure for tests. */
export function flipModuleFavorite<T extends { id: string; is_favorite: boolean }>(row: T | undefined): T | undefined {
  if (row === undefined) return undefined;
  return { ...row, is_favorite: !row.is_favorite };
}

/** Flip the matching row inside a cached list; pure for tests. */
export function flipModuleFavoriteInList<T extends { id: string; is_favorite: boolean }>(
  rows: readonly T[] | undefined,
  moduleId: string
): T[] | undefined {
  if (rows === undefined) return undefined;
  return rows.map((row) => (row.id === moduleId ? { ...row, is_favorite: !row.is_favorite } : row));
}

/**
 * Star/unstar a module. The display mark follows the cache (flipped
 * optimistically, settled by the re-read); `pending` pins the progress
 * state on the star while the write flies. Overlapping toggles are
 * refused: the second click waits for the first write to settle, so a
 * double press can never read the optimistic flip as the opposite
 * toggle.
 */
export function useModuleFavorite(target: ModuleFavoriteTarget): {
  starred: boolean;
  pending: boolean;
  toggle: () => void;
} {
  const queryClient = useQueryClient();
  const listKey = moduleKeys.list(target.workspaceSlug, target.projectId);

  const mutation = useMutation<void, Error, Pick<ModuleRow, "id" | "name" | "is_favorite">>({
    mutationFn: async (module) => {
      await queryClient.cancelQueries({ queryKey: listKey });
      const rollback = applyOptimisticUpdate(queryClient, listKey, (rows: ModuleRow[] | undefined) =>
        flipModuleFavoriteInList(rows, module.id)
      );
      try {
        if (module.is_favorite) {
          await unfavoriteModule(getAppClient(), target.workspaceSlug, target.projectId, module.id);
          notifySuccess(MODULE_SUCCESS_TITLE, MODULE_FAVORITE_REMOVED);
        } else {
          await favoriteModule(getAppClient(), target.workspaceSlug, target.projectId, module.id);
          notifySuccess(MODULE_SUCCESS_TITLE, MODULE_FAVORITE_ADDED);
          if (!isFavoritesSectionOpen()) setFavoritesSectionOpen(true);
        }
      } catch (error) {
        rollback();
        notifyError(MODULE_FAILURE_TITLE, moduleFailureBody(error));
        throw error;
      }
    },
    onSettled: (_data, _error, module) => {
      // The re-read carries is_favorite, so the star, the row and the
      // sidebar agree; the sidebar tree re-reads so the first add
      // surfaces without a reload.
      void invalidateAfterModuleWrite(queryClient, target.workspaceSlug, target.projectId, module.id);
      void queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(target.workspaceSlug) });
    },
  });

  // Mid-flight the mark is the optimistic target, whatever cache entry
  // the caller's row came from; settled it is the cache truth, which
  // the re-read keeps current.
  const flight = mutation.isPending ? mutation.variables : undefined;
  const starred = flight === undefined ? target.module.is_favorite : !flight.is_favorite;
  return {
    starred,
    pending: mutation.isPending,
    toggle: () => {
      if (!mutation.isPending) mutation.mutate(target.module);
    },
  };
}
