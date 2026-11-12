// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar favorites (parity/specs/shell.md §4, SHELL-056). Top-level
// entries and folders over `.../user-favorites/`, per-folder children
// over the group endpoint, and folder create/rename, removal and
// sequence reorder writes. Ordering is descending sequence; reorder
// lands with one PATCH per drop. Writes re-read after settling and
// toast the oracle-pinned notices.

import { queryOptions, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  createFavorite,
  deleteFavorite,
  listFavoriteGroup,
  listFavorites,
  updateFavorite,
  type ApiClient,
  type Favorite,
  type FavoriteList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { workspaceScope } from "../../../core/query/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { sidebarKeys } from "./keys.js";

/** Step a reorder write jumps past its new end neighbor (SHELL-056). */
export const FAVORITE_ORDER_STEP = 10000;

/** Folder-form rejection the parity driver reads (SHELL-056). */
export const FOLDER_EMPTY_TITLE = "Folder name cannot be empty";
/** Duplicate-folder rejection the parity driver reads (SHELL-056). */
export const FOLDER_DUPLICATE_TITLE = "Folder already exists";
/** Removal confirmation the parity driver reads (SHELL-056). */
export const FAVORITE_REMOVED_TITLE = "Favorite removed successfully";
const FAVORITE_CREATED_TITLE = "Favorite created successfully";
const FAVORITE_UPDATED_TITLE = "Favorite updated successfully";
const FAVORITE_FAILED_TITLE = "Something went wrong";
const FAVORITE_FAILED_BODY = "The favorites change didn't save — the list is back to its previous state.";

/** Descending stored sequence, the server's display order. */
export function compareFavoriteSequence(a: Pick<Favorite, "sequence">, b: Pick<Favorite, "sequence">): number {
  return b.sequence - a.sequence;
}

/** The list in display order, without mutating the cache. */
export function selectOrderedFavorites(list: readonly Favorite[]): Favorite[] {
  return [...list].sort(compareFavoriteSequence);
}

/** Top-level rows (folders and loose entries) in display order. */
export function selectTopLevelFavorites(list: readonly Favorite[]): Favorite[] {
  return selectOrderedFavorites(list.filter((row) => row.parent === null));
}

/** Why a folder name is rejected, or null when it is usable. */
export type FolderNameIssue = "empty" | "duplicate";

/**
 * Validate a folder name against the sibling folder names (SHELL-056):
 * blank-after-trimming is empty, a trimmed exact hit is a duplicate.
 * The caller blocks a literally empty submit silently (the required
 * rule), so this only ever sees names with something in them.
 */
export function validateFolderName(name: string, existing: readonly string[]): FolderNameIssue | null {
  if (name.trim() === "") return "empty";
  if (existing.includes(name.trim())) return "duplicate";
  return null;
}

/**
 * Ids with the dragged row moved to a slot, shifting the rows after
 * it down. Slots clamp into the list; unknown dragged ids resolve
 * back to the current order.
 */
export function moveFavoriteToIndex(current: readonly string[], draggedId: string, index: number): string[] {
  if (!current.includes(draggedId)) return [...current];
  const next = current.filter((id) => id !== draggedId);
  next.splice(Math.max(0, Math.min(index, next.length)), 0, draggedId);
  return next;
}

/**
 * Stored sequence landing the dragged row at its new slot with one
 * write (SHELL-056): past the edge neighbor at either end, midway
 * between the new neighbors in the middle. The order runs
 * high-to-low, so the head jumps up and the tail jumps down.
 */
export function sequenceForMove(
  ordered: readonly Pick<Favorite, "id" | "sequence">[],
  draggedId: string,
  order: readonly string[]
): number | null {
  const slot = order.indexOf(draggedId);
  if (slot < 0 || order.length !== ordered.length) return null;
  const positions = new Map(ordered.map((row) => [row.id, row.sequence]));
  const rest = order.filter((id) => id !== draggedId);
  if (slot === 0) return (positions.get(rest[0] ?? "") ?? 0) + FAVORITE_ORDER_STEP;
  if (slot === order.length - 1) return (positions.get(rest[rest.length - 1] ?? "") ?? 0) - FAVORITE_ORDER_STEP;
  return ((positions.get(rest[slot - 1] ?? "") ?? 0) + (positions.get(rest[slot] ?? "") ?? 0)) / 2;
}

/**
 * The single reorder write for a drop, or null when the drop changes
 * nothing. The caller patches the dragged row's sequence and re-reads
 * the list; siblings keep their sequences.
 */
export function favoriteOrderPatch(
  ordered: readonly Pick<Favorite, "id" | "sequence">[],
  order: readonly string[],
  draggedId: string
): { favoriteId: string; sequence: number } | null {
  const current = ordered.map((row) => row.id);
  if (order.length !== current.length || order.every((id, index) => id === current[index])) return null;
  const sequence = sequenceForMove(ordered, draggedId, order);
  if (sequence === null) return null;
  return { favoriteId: draggedId, sequence };
}

/** Next list cache with one row's sequence applied. */
export function applyFavoriteSequence(
  previous: FavoriteList | undefined,
  favoriteId: string,
  sequence: number
): FavoriteList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) => (row.id === favoriteId ? { ...row, sequence } : row));
}

/** Next list cache with one row's parent applied. */
export function applyFavoriteParent(
  previous: FavoriteList | undefined,
  favoriteId: string,
  parent: string | null
): FavoriteList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) => (row.id === favoriteId ? { ...row, parent } : row));
}

/**
 * Stored sequence landing a row inserted from another list at its new
 * slot with one write (SHELL-109): past the edge neighbor at either
 * end, midway between the new neighbors in the middle. Slots clamp
 * into the list; an empty list starts a step above zero.
 */
export function sequenceForInsert(ordered: readonly Pick<Favorite, "id" | "sequence">[], index: number): number {
  const slot = Math.max(0, Math.min(index, ordered.length));
  const prev = ordered[slot - 1]?.sequence;
  const next = ordered[slot]?.sequence;
  if (prev === undefined) return (next ?? 0) + FAVORITE_ORDER_STEP;
  if (next === undefined) return prev - FAVORITE_ORDER_STEP;
  return (prev + next) / 2;
}

/** One measured favorites row for drop resolution (SHELL-109). */
export interface FavoriteDropSpan {
  id: string;
  top: number;
  bottom: number;
  isFolder: boolean;
  /** Folder header bounds; the row bounds for plain entries. */
  headerTop: number;
  headerBottom: number;
  headerLeft: number;
  headerRight: number;
}

/**
 * Folder id whose header middle band holds the pointer, or null
 * (SHELL-109). The band is the header quarter-inset on every side, so
 * edge releases keep their sibling-reorder meaning; the dragged row
 * itself never matches. Degenerate (zero-area) headers never match.
 */
export function resolveIntoFolder(
  spans: readonly FavoriteDropSpan[],
  x: number,
  y: number,
  sourceId: string
): string | null {
  for (const span of spans) {
    if (!span.isFolder || span.id === sourceId) continue;
    const left = span.headerLeft;
    const right = span.headerRight;
    const top = span.headerTop;
    const bottom = span.headerBottom;
    const width = right - left;
    const height = bottom - top;
    if (
      width > 0 &&
      height > 0 &&
      x >= left + width / 4 &&
      x <= right - width / 4 &&
      y >= top + height / 4 &&
      y <= bottom - height / 4
    ) {
      return span.id;
    }
  }
  return null;
}

/** The caller's top-level favorites for one workspace. */
export function favoritesQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: sidebarKeys.favorites(workspaceSlug),
    queryFn: () => listFavorites(client, workspaceSlug),
  });
}

/** The caller's top-level favorites for one workspace. */
export function useFavorites(workspaceSlug: string) {
  return useQuery(favoritesQueryOptions(getAppClient(), workspaceSlug));
}

/** One folder's children for one workspace. */
export function favoriteGroupQueryOptions(client: ApiClient, workspaceSlug: string, folderId: string) {
  return queryOptions({
    queryKey: sidebarKeys.favoriteGroup(workspaceSlug, folderId),
    queryFn: () => listFavoriteGroup(client, workspaceSlug, folderId),
  });
}

/** One folder's children for one workspace. */
export function useFavoriteGroup(workspaceSlug: string, folderId: string) {
  return useQuery(favoriteGroupQueryOptions(getAppClient(), workspaceSlug, folderId));
}

/** Re-read the top-level list plus every open folder's children. */
function invalidateFavoriteLists(queryClient: QueryClient, workspaceSlug: string) {
  void queryClient.invalidateQueries({ queryKey: [...workspaceScope(workspaceSlug), "favorites"] });
}

/** Create a top-level folder, then re-read the list. */
export function useCreateFavoriteFolder(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<Favorite, Error, { name: string }>({
    mutationFn: async (input) => {
      try {
        const row = await createFavorite(getAppClient(), workspaceSlug, {
          entity_type: "folder",
          name: input.name.trim(),
          is_folder: true,
          parent: null,
          project_id: null,
        });
        notifySuccess(FAVORITE_CREATED_TITLE);
        return row;
      } catch (error) {
        notifyError(FAVORITE_FAILED_TITLE, FAVORITE_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => invalidateFavoriteLists(queryClient, workspaceSlug),
  });
}

/** Rename a folder, then re-read the list. */
export function useRenameFavoriteFolder(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<Favorite, Error, { favoriteId: string; name: string }>({
    mutationFn: async (input) => {
      try {
        const row = await updateFavorite(getAppClient(), workspaceSlug, input.favoriteId, {
          name: input.name.trim(),
        });
        notifySuccess(FAVORITE_UPDATED_TITLE);
        return row;
      } catch (error) {
        notifyError(FAVORITE_FAILED_TITLE, FAVORITE_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => invalidateFavoriteLists(queryClient, workspaceSlug),
  });
}

/** Remove one row (entry or folder), then re-read the lists. */
export function useDeleteFavorite(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, { favoriteId: string }>({
    mutationFn: async (input) => {
      try {
        await deleteFavorite(getAppClient(), workspaceSlug, input.favoriteId);
        notifySuccess(FAVORITE_REMOVED_TITLE);
      } catch (error) {
        notifyError(FAVORITE_FAILED_TITLE, FAVORITE_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => invalidateFavoriteLists(queryClient, workspaceSlug),
  });
}

export interface FavoriteMoveInput {
  favoriteId: string;
  /** Folder holding the dragged list, or null for the top level. */
  groupId: string | null;
  /** New stored sequence for sibling-reorder drops. */
  sequence?: number;
  /** New parent for folder drops: a folder id to nest, null to un-nest. */
  parent?: string | null;
}

/** Persist one dragged row's sequence and/or parent with rollback on failure. */
export function useMoveFavorite(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, FavoriteMoveInput>({
    mutationFn: async (input) => {
      const { favoriteId, groupId, sequence, parent } = input;
      if (sequence === undefined && parent === undefined) return;
      const key =
        groupId === null ? sidebarKeys.favorites(workspaceSlug) : sidebarKeys.favoriteGroup(workspaceSlug, groupId);
      const patch = {
        ...(sequence !== undefined && { sequence }),
        ...(parent !== undefined && { parent }),
      };
      const rollback = applyOptimisticUpdate<FavoriteList>(queryClient, key, (previous) =>
        previous?.map((row) => (row.id === favoriteId ? { ...row, ...patch } : row))
      );
      try {
        await updateFavorite(getAppClient(), workspaceSlug, favoriteId, patch);
      } catch (error) {
        rollback();
        notifyError(FAVORITE_FAILED_TITLE, FAVORITE_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => invalidateFavoriteLists(queryClient, workspaceSlug),
  });
}
