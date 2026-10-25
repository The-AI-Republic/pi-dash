// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Nested tree writes (parity/specs/pages.md §11.7, PAGE-084/087/088). Three
// capabilities live here so the list, the detail header and the tree share
// one implementation each: create a child from a source page, reparent a
// page (with its subtree) inside the project, and archive/restore while
// saying what the server did to the subtree. The cross-project move stays
// in rowMutations — the two moves never share a dialog.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  archivePage,
  createPage,
  isApiError,
  restorePage,
  updatePage,
  type PageDetail,
  type PageRow as PageRowData,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import {
  PAGE_ARCHIVE_FAILED_TITLE,
  PAGE_ARCHIVED_TITLE,
  PAGE_MOVE_FAILED_TITLE,
  PAGE_RESTORED_TITLE,
  PAGE_RESTORE_FAILED_TITLE,
} from "../lib/pageActions.js";
import { PAGE_CREATE_FAILED_TITLE } from "./mutations.js";
import { pageCreateInput } from "../lib/pageCreate.js";
import { PAGE_NESTED_MOVED_TITLE, PAGE_RESTORE_TO_TOP_TITLE, restoreDetaches } from "../lib/pageNested.js";
import { pageKeys } from "./keys.js";
import { PAGE_EVENT_ARCHIVED, PAGE_EVENT_MOVED, PAGE_EVENT_UNARCHIVED, publishPageEvent } from "./rowMutations.js";

/** Body a sub-page receives when created from a source page's menu. */
export const PAGE_SUB_PAGE_NAME = "Untitled";

/** The reason text shown when a move is refused by the server. */
export function moveFailureMessage(error: unknown): string {
  if (isApiError(error) && typeof error.message === "string" && error.message.trim() !== "") return error.message;
  return PAGE_MOVE_FAILED_TITLE;
}

/**
 * Create a child page under `parentId` and open it (PAGE-084). The child
 * inherits only the parent link; the caller supplies PAGE-007's quick-create
 * visibility. A successful write refreshes the parent's cached children and
 * the collection, so the parent's count increases without a manual reload.
 */
export function useAddSubPage(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<PageDetail, Error, { parentId: string; access: number }>({
    mutationFn: ({ parentId, access }) =>
      createPage(getAppClient(), workspaceSlug, projectId, {
        ...pageCreateInput(PAGE_SUB_PAGE_NAME, access),
        parent: parentId,
      }),
    onError: () => {
      notifyError(PAGE_CREATE_FAILED_TITLE);
    },
    onSettled: (_data, _error, variables) => {
      void queryClient.invalidateQueries({
        queryKey: [...pageKeys.all(workspaceSlug), "children", projectId, variables.parentId],
      });
      void queryClient.invalidateQueries({ queryKey: pageKeys.all(workspaceSlug) });
    },
  });
}

export interface PageReparentInput {
  pageId: string;
  /** Target container, or null to return the page to the top level. */
  parentId: string | null;
}

/**
 * Reparent one page inside its project (PAGE-087). The server moves the
 * whole subtree with it; the client refreshes both ends of the move. A
 * refusal keeps the tree where it was and reports the server's reason.
 */
export function usePageReparent(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, PageReparentInput>({
    mutationFn: async ({ pageId, parentId }) => {
      await updatePage(getAppClient(), workspaceSlug, projectId, pageId, { parent: parentId });
    },
    onSuccess: (_data, variables) => {
      notifySuccess(PAGE_NESTED_MOVED_TITLE);
      publishPageEvent({ workspaceSlug, projectId, pageId: variables.pageId, event: PAGE_EVENT_MOVED });
    },
    onError: (error) => {
      notifyError(moveFailureMessage(error));
    },
    onSettled: () => {
      // Parenthood changes every ancestor's child list and count, so the
      // whole nested subtree is stale, not just the two ends.
      void queryClient.invalidateQueries({ queryKey: pageKeys.all(workspaceSlug) });
    },
  });
}

export interface PageSubtreeArchiveInput {
  pageId: string;
  /** The row as the list holds it; its parent decides the restore notice. */
  row: Pick<PageRowData, "parent" | "parent_id">;
  archived: boolean;
}

/**
 * Archive or restore a page and its subtree (PAGE-088). Archiving cascades
 * server-side; restoring a page whose parent is still archived detaches it
 * to the top level, and the notice says so. The notice is chosen from the
 * collection the caller already holds, so no extra read is needed.
 */
export function usePageSubtreeArchive(
  workspaceSlug: string,
  projectId: string,
  rows: readonly PageRowData[],
  onChange?: (archived: boolean) => void
) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, PageSubtreeArchiveInput>({
    mutationFn: async ({ pageId, archived }) => {
      if (archived) {
        await archivePage(getAppClient(), workspaceSlug, projectId, pageId);
        return;
      }
      await restorePage(getAppClient(), workspaceSlug, projectId, pageId);
    },
    onSuccess: (_data, variables) => {
      onChange?.(variables.archived);
      if (variables.archived) {
        notifySuccess(PAGE_ARCHIVED_TITLE);
      } else if (restoreDetaches(rows, variables.row)) {
        notifySuccess(PAGE_RESTORE_TO_TOP_TITLE);
      } else {
        notifySuccess(PAGE_RESTORED_TITLE);
      }
      publishPageEvent({
        workspaceSlug,
        projectId,
        pageId: variables.pageId,
        event: variables.archived ? PAGE_EVENT_ARCHIVED : PAGE_EVENT_UNARCHIVED,
      });
    },
    onError: (_error, variables) => {
      notifyError(variables.archived ? PAGE_ARCHIVE_FAILED_TITLE : PAGE_RESTORE_FAILED_TITLE);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: pageKeys.all(workspaceSlug) });
    },
  });
}
