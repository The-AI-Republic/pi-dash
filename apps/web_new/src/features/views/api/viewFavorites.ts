// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// View favorite writes (parity/specs/views.md §2/§6, VIEW-012). The
// row star toggles through the generic favorites collection: add is
// POST .../user-favorites/ with entity_type "view", remove is DELETE
// of the favorite row's own id. The toggle is silent — no notice on
// success or failure — flips the cached record at once, rolls back on
// failure, and opens the sidebar favorites section on the first add.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  addViewFavorite,
  listFavorites,
  removeViewFavorite,
  type SavedView,
  type SavedViewList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../../core/query/optimistic.js";
import { isFavoritesSectionOpen, setFavoritesSectionOpen } from "../../../shared/shell/index.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { viewKeys } from "./keys.js";
import { findViewFavoriteId, flipViewFavorite, isViewFavorited } from "../lib/projectViewActions.js";

export interface ToggleViewFavoriteInput {
  workspaceSlug: string;
  projectId: string;
  view: SavedView;
}

/**
 * Favorite row ids learned from add responses, keyed by view id. The
 * list record carries only `is_favorite`, but the remove call keys on
 * the favorite row — a star/unstarr cycle in one visit reuses the id
 * without a lookup; anything older falls back to the favorites list.
 */
const viewFavoriteIds = new Map<string, string>();

/** Forget learned favorite ids (tests only). */
export function resetViewFavoriteIds(): void {
  viewFavoriteIds.clear();
}

/** The row mid-toggle, or null when no toggle is flying. Pure for tests. */
export function selectPendingViewId(isPending: boolean, variables: ToggleViewFavoriteInput | undefined): string | null {
  if (!isPending) return null;
  return variables?.view.id ?? null;
}

/** The favorite row to remove: learned first, the favorites list after. */
async function resolveViewFavoriteId(workspaceSlug: string, viewId: string): Promise<string | null> {
  const learned = viewFavoriteIds.get(viewId);
  if (learned !== undefined) return learned;
  const favorites = await listFavorites(getAppClient(), workspaceSlug);
  return findViewFavoriteId(favorites, viewId);
}

/**
 * Star/unstar a saved view (VIEW-012): the row flips at once, the
 * write lands through the generic favorites pair, and the first add
 * opens the favorites section. A failed write rolls the row back.
 * Silent throughout: no success or failure notice ever fires here.
 */
export function useToggleViewFavorite() {
  const queryClient = useQueryClient();
  const mutation = useMutation<void, Error, ToggleViewFavoriteInput, { rollback: Rollback }>({
    mutationFn: async (input) => {
      const client = getAppClient();
      if (isViewFavorited(input.view.is_favorite)) {
        const favoriteId = await resolveViewFavoriteId(input.workspaceSlug, input.view.id);
        if (favoriteId === null) throw new Error(`No favorite row found for view ${input.view.id}.`);
        await removeViewFavorite(client, input.workspaceSlug, favoriteId);
        viewFavoriteIds.delete(input.view.id);
        return;
      }
      const row = await addViewFavorite(client, input.workspaceSlug, input.view);
      viewFavoriteIds.set(input.view.id, row.id);
      if (!isFavoritesSectionOpen()) setFavoritesSectionOpen(true);
    },
    onMutate: async (input) => {
      const key = viewKeys.projectList(input.workspaceSlug, input.projectId);
      await queryClient.cancelQueries({ queryKey: key });
      return {
        rollback: applyOptimisticUpdate<SavedViewList>(queryClient, key, (rows) =>
          flipViewFavorite(rows, input.view.id)
        ),
      };
    },
    onError: (_error, _input, context) => {
      context?.rollback();
    },
    onSettled: (_data, _error, input) => {
      // The sidebar section reads its own list: starring re-reads it
      // so the first add surfaces (and opens the section) without a
      // reload. The views list itself is never re-read here — the
      // write already landed on the cached record (VIEW-012).
      void queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(input.workspaceSlug) });
    },
  });
  return {
    ...mutation,
    /** The row mid-toggle, so the view pins the pending state on it. */
    pendingViewId: selectPendingViewId(mutation.isPending, mutation.variables),
  };
}
