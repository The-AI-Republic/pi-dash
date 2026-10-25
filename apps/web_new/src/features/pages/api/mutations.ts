// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page creation, duplication and the shared favourite write
// (parity/specs/pages.md §2.4, §3.6, §3.8, §7.2 and §13.1, PAGE-007–012
// and PAGE-056). One favourite helper serves the list row and the detail
// header; the two surfaces draw different controls around it but never a
// second implementation. Every write reports its outcome through the
// shared notice surface, and a failed favourite write rolls the visible
// star back.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  addPageFavorite,
  createPage,
  duplicatePage,
  removePageFavorite,
  type PageCreateInput,
  type PageDuplicateResult,
  type PageRow as PageRowData,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../../core/query/optimistic.js";
import { isFavoritesSectionOpen, setFavoritesSectionOpen } from "../../../shared/shell/favorites.js";
import { notifyError, notifyPending, notifySuccess } from "../../../shared/shell/toaster.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { PAGE_DUPLICATE_FAILED_TITLE, PAGE_DUPLICATED_TITLE } from "../lib/pageActions.js";
import { pageCreateInput, PAGE_QUICK_CREATE_NAME } from "../lib/pageCreate.js";
import { pageKeys } from "./keys.js";

/** Create failure; the oracle matches /could not be created/ on it. */
export const PAGE_CREATE_FAILED_TITLE = "Page could not be created.";

/** Favourite progress and outcomes; the oracle matches /added/ and /removed/. */
export const PAGE_FAVORITE_ADDING_TITLE = "Adding to favorites…";
export const PAGE_FAVORITE_REMOVING_TITLE = "Removing from favorites…";
export const PAGE_FAVORITE_ADDED_TITLE = "Added to favorites";
export const PAGE_FAVORITE_REMOVED_TITLE = "Removed from favorites";
export const PAGE_FAVORITE_FAILED_TITLE = "Couldn't update favorites";

/** Flip one cached row's star; undefined stays undefined. Pure for tests. */
export function flipPageFavorite<T extends { is_favorite: boolean }>(row: T | undefined): T | undefined {
  return row === undefined ? undefined : { ...row, is_favorite: !row.is_favorite };
}

/** Flip the matching row inside a cached collection. Pure for tests. */
export function flipPageFavoriteInList<T extends { id: string; is_favorite: boolean }>(
  rows: readonly T[] | undefined,
  pageId: string
): T[] | undefined {
  return rows?.map((row) => (row.id === pageId ? { ...row, is_favorite: !row.is_favorite } : row));
}

export interface CreatePageVariables {
  access: number;
  input: PageCreateInput;
}

/**
 * Create a page and re-read the collection so the new row is present for
 * whoever stays on the list. Callers decide whether to navigate; a failed
 * write only reports, leaving the caller where it is.
 */
export function useCreatePage(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<PageRowData, Error, CreatePageVariables>({
    mutationFn: ({ input }) => createPage(getAppClient(), workspaceSlug, projectId, input),
    onError: () => {
      notifyError(PAGE_CREATE_FAILED_TITLE);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: pageKeys.list(workspaceSlug, projectId) }),
  });
}

/** Quick-create state: one press posts an untitled page into the tab's visibility. */
export function useQuickCreatePage(workspaceSlug: string, projectId: string, access: number) {
  const create = useCreatePage(workspaceSlug, projectId);
  return {
    pending: create.isPending,
    create: (onCreated: (page: PageRowData) => void): void => {
      create.mutate({ access, input: pageCreateInput(PAGE_QUICK_CREATE_NAME, access) }, { onSuccess: onCreated });
    },
  };
}

export interface DuplicatePageVariables {
  pageId: string;
}

/**
 * Copy a page into a standalone duplicate. The copy arrives through the
 * re-read, never through a client-side insertion, so the list keeps one
 * source of truth.
 */
export function useDuplicatePage(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<PageDuplicateResult, Error, DuplicatePageVariables>({
    mutationFn: ({ pageId }) => duplicatePage(getAppClient(), workspaceSlug, projectId, pageId),
    onSuccess: () => {
      notifySuccess(PAGE_DUPLICATED_TITLE);
    },
    onError: () => {
      notifyError(PAGE_DUPLICATE_FAILED_TITLE);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: pageKeys.list(workspaceSlug, projectId) }),
  });
}

export interface PageFavoriteTarget {
  workspaceSlug: string;
  projectId: string;
  page: Pick<PageRowData, "id" | "name" | "is_favorite">;
}

/**
 * Star or unstar one page for the signed-in user (§13.1). The mark flips at
 * once in the collection and the detail cache and rolls back if the write
 * fails; the first successful add also opens the sidebar's favourites
 * section, which is the shell seam this area registers into rather than
 * rendering itself. Both callers wait for an in-flight write before the
 * next one, so a double press cannot race two writes for one page.
 */
export function usePageFavorite(target: PageFavoriteTarget): {
  starred: boolean;
  pending: boolean;
  toggle: () => void;
} {
  const queryClient = useQueryClient();
  const { workspaceSlug, projectId, page } = target;
  const listKey = pageKeys.list(workspaceSlug, projectId);
  const detailKey = pageKeys.detail(workspaceSlug, projectId, page.id);

  const mutation = useMutation<void, Error, Pick<PageRowData, "id" | "name" | "is_favorite">, { rollback: Rollback }>({
    mutationFn: async (row) => {
      const client = getAppClient();
      const notice = notifyPending(row.is_favorite ? PAGE_FAVORITE_REMOVING_TITLE : PAGE_FAVORITE_ADDING_TITLE);
      try {
        if (row.is_favorite) {
          await removePageFavorite(client, workspaceSlug, projectId, row.id);
          notice.settle({ title: PAGE_FAVORITE_REMOVED_TITLE, description: `${row.name} is no longer a favorite.` });
          return;
        }
        await addPageFavorite(client, workspaceSlug, projectId, row.id);
        notice.settle({ title: PAGE_FAVORITE_ADDED_TITLE, description: `${row.name} is now a favorite.` });
        if (!isFavoritesSectionOpen()) setFavoritesSectionOpen(true);
      } catch (error) {
        notice.settle({
          title: PAGE_FAVORITE_FAILED_TITLE,
          ...(error instanceof Error ? { description: error.message } : {}),
          failed: true,
        });
        throw error;
      }
    },
    onMutate: async (row) => {
      await queryClient.cancelQueries({ queryKey: listKey });
      await queryClient.cancelQueries({ queryKey: detailKey });
      const rollbackList = applyOptimisticUpdate(queryClient, listKey, (rows: PageRowData[] | undefined) =>
        flipPageFavoriteInList(rows, row.id)
      );
      const rollbackDetail = applyOptimisticUpdate(queryClient, detailKey, flipPageFavorite);
      return {
        rollback: () => {
          rollbackList();
          rollbackDetail();
        },
      };
    },
    onError: (_error, _row, context) => {
      context?.rollback();
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: listKey });
      void queryClient.invalidateQueries({ queryKey: detailKey });
      // The sidebar tree owns the favourites list; re-reading it lets the
      // section the first add just opened show the page.
      void queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(workspaceSlug) });
    },
  });

  return {
    starred: page.is_favorite,
    pending: mutation.isPending,
    toggle: () => {
      if (mutation.isPending) return;
      mutation.mutate({ id: page.id, name: page.name, is_favorite: page.is_favorite });
    },
  };
}
