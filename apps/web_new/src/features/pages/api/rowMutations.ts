// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row state writes (parity/specs/pages.md §2.3, §2.4, §3.8, §11.1,
// PAGE-013–PAGE-017): visibility, lock, shelve/restore, deletion and the
// cloud-only cross-project move. Each capability has exactly one helper so
// the list row and the detail header (PAGE-057–059) drive the same write
// with different chrome. Optimistic mutations patch the one collection and
// the detail cache, roll back on refusal, notify with the pinned copy and
// publish the protocol event through the shared realtime hub — the
// collaboration channel the old app used to keep other viewers in step.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  archivePage,
  deletePage,
  lockPage,
  movePageToProject,
  PAGE_ACCESS_PRIVATE,
  PAGE_ACCESS_PUBLIC,
  restorePage,
  setPageAccess,
  unlockPage,
  type PageRow as PageRowData,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { getRealtimeHub } from "../../../core/query/realtime.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import {
  PAGE_ARCHIVED_TITLE,
  PAGE_ARCHIVE_FAILED_TITLE,
  PAGE_DELETED_TITLE,
  PAGE_DELETE_FAILED_TITLE,
  PAGE_LOCKED_TITLE,
  PAGE_LOCK_FAILED_TITLE,
  PAGE_MOVED_TITLE,
  PAGE_MOVE_FAILED_TITLE,
  PAGE_RESTORED_TITLE,
  PAGE_RESTORE_FAILED_TITLE,
  PAGE_UNLOCKED_TITLE,
  PAGE_VISIBILITY_FAILED_TITLE,
  PAGE_VISIBILITY_PUBLIC_SUFFIX,
  PAGE_VISIBILITY_SUFFIX,
} from "../lib/pageActions.js";
import { pageKeys } from "./keys.js";

/** Topic the pages area publishes state changes on. */
export const PAGES_REALTIME_TOPIC = "pages";

/** Protocol event names the old collaboration channel carried. */
export const PAGE_EVENT_MADE_PRIVATE = "made-private";
export const PAGE_EVENT_MADE_PUBLIC = "made-public";
export const PAGE_EVENT_LOCKED = "locked";
export const PAGE_EVENT_UNLOCKED = "unlocked";
export const PAGE_EVENT_ARCHIVED = "archived";
export const PAGE_EVENT_UNARCHIVED = "unarchived";
export const PAGE_EVENT_DELETED = "deleted";
export const PAGE_EVENT_MOVED = "moved";

export interface PageEventPayload {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  event: string;
}

/** Publish one page state change for other viewers. */
export function publishPageEvent(payload: PageEventPayload): void {
  getRealtimeHub().publish({ topic: PAGES_REALTIME_TOPIC, payload });
}

/** Replace one row inside a cached collection; other rows keep identity. */
export function patchPageRow<T extends { id: string }>(
  rows: readonly T[] | undefined,
  pageId: string,
  patch: Partial<T>
): T[] | undefined {
  return rows?.map((row) => (row.id === pageId ? { ...row, ...patch } : row));
}

/** Drop one row from a cached collection. */
export function removePageRow<T extends { id: string }>(
  rows: readonly T[] | undefined,
  pageId: string
): T[] | undefined {
  return rows?.filter((row) => row.id !== pageId);
}

/** One page's row shaped for the shared cache patches. */
export interface PageRowTarget {
  workspaceSlug: string;
  projectId: string;
  page: Pick<PageRowData, "id" | "name">;
}

interface RowCache {
  queryClient: ReturnType<typeof useQueryClient>;
  listKey: readonly unknown[];
  detailKey: readonly unknown[];
  patch: (patch: Partial<PageRowData>) => Promise<{ rollback: () => void }>;
  /** Drop the row once the server has confirmed it is gone. */
  drop: () => void;
  invalidate: () => void;
}

/** The one cache surface every row write touches. */
function useRowCache(target: PageRowTarget): RowCache {
  const queryClient = useQueryClient();
  const listKey = pageKeys.list(target.workspaceSlug, target.projectId);
  const detailKey = pageKeys.detail(target.workspaceSlug, target.projectId, target.page.id);

  const patch = async (change: Partial<PageRowData>): Promise<{ rollback: () => void }> => {
    await queryClient.cancelQueries({ queryKey: listKey });
    await queryClient.cancelQueries({ queryKey: detailKey });
    const rollbackList = applyOptimisticUpdate(queryClient, listKey, (rows: PageRowData[] | undefined) =>
      patchPageRow(rows, target.page.id, change)
    );
    const rollbackDetail = applyOptimisticUpdate(queryClient, detailKey, (row: PageRowData | undefined) =>
      row?.id === target.page.id ? { ...row, ...change } : row
    );
    return {
      rollback: () => {
        rollbackList();
        rollbackDetail();
      },
    };
  };

  return {
    queryClient,
    listKey,
    detailKey,
    patch,
    drop: () => {
      queryClient.setQueryData<PageRowData[] | undefined>(listKey, (rows) => removePageRow(rows, target.page.id));
      queryClient.removeQueries({ queryKey: detailKey, exact: true });
    },
    invalidate: () => {
      void queryClient.invalidateQueries({ queryKey: listKey });
      void queryClient.invalidateQueries({ queryKey: detailKey });
    },
  };
}

function publish(target: PageRowTarget, event: string): void {
  publishPageEvent({
    workspaceSlug: target.workspaceSlug,
    projectId: target.projectId,
    pageId: target.page.id,
    event,
  });
}

/** One page's visibility switch (PAGE-013, shared with PAGE-057). */
export function usePageVisibilityWrite(target: PageRowTarget): {
  pending: boolean;
  setVisibility: (restricted: boolean) => void;
} {
  const cache = useRowCache(target);
  const mutation = useMutation<void, Error, number, { rollback: () => void }>({
    mutationFn: (access) =>
      setPageAccess(getAppClient(), target.workspaceSlug, target.projectId, target.page.id, access),
    onMutate: (access) => cache.patch({ access }),
    onSuccess: (_data, access) => {
      const word = access === PAGE_ACCESS_PRIVATE ? PAGE_VISIBILITY_SUFFIX : PAGE_VISIBILITY_PUBLIC_SUFFIX;
      notifySuccess(`Page ${word}.`);
      publish(target, access === PAGE_ACCESS_PRIVATE ? PAGE_EVENT_MADE_PRIVATE : PAGE_EVENT_MADE_PUBLIC);
    },
    onError: (_error, _access, context) => {
      context?.rollback();
      notifyError(PAGE_VISIBILITY_FAILED_TITLE);
    },
    onSettled: cache.invalidate,
  });
  return {
    pending: mutation.isPending,
    setVisibility: (restricted) => {
      if (mutation.isPending) return;
      mutation.mutate(restricted ? PAGE_ACCESS_PRIVATE : PAGE_ACCESS_PUBLIC);
    },
  };
}

/** One page's lock switch (PAGE-014, shared with PAGE-058). */
export function usePageLockWrite(target: PageRowTarget): {
  pending: boolean;
  toggleLock: (locked: boolean) => void;
} {
  const cache = useRowCache(target);
  const mutation = useMutation<void, Error, boolean, { rollback: () => void }>({
    mutationFn: (locked) =>
      locked
        ? lockPage(getAppClient(), target.workspaceSlug, target.projectId, target.page.id)
        : unlockPage(getAppClient(), target.workspaceSlug, target.projectId, target.page.id),
    onMutate: (locked) => cache.patch({ is_locked: locked }),
    onSuccess: (_data, locked) => {
      notifySuccess(locked ? PAGE_LOCKED_TITLE : PAGE_UNLOCKED_TITLE);
      publish(target, locked ? PAGE_EVENT_LOCKED : PAGE_EVENT_UNLOCKED);
    },
    onError: (_error, _locked, context) => {
      context?.rollback();
      notifyError(PAGE_LOCK_FAILED_TITLE);
    },
    onSettled: cache.invalidate,
  });
  return {
    pending: mutation.isPending,
    toggleLock: (locked) => {
      if (mutation.isPending) return;
      mutation.mutate(locked);
    },
  };
}

/** One page's shelve/restore switch (PAGE-015, shared with PAGE-059). */
export function usePageArchiveWrite(target: PageRowTarget): {
  pending: boolean;
  toggleArchive: (archived: boolean) => void;
} {
  const cache = useRowCache(target);
  const mutation = useMutation<void, Error, boolean, { rollback: () => void }>({
    mutationFn: async (archived) => {
      if (archived) {
        await archivePage(getAppClient(), target.workspaceSlug, target.projectId, target.page.id);
        return;
      }
      await restorePage(getAppClient(), target.workspaceSlug, target.projectId, target.page.id);
    },
    onMutate: (archived) =>
      cache.patch({
        archived_at: archived ? new Date().toISOString() : null,
        ...(archived ? { is_favorite: false } : {}),
      }),
    onSuccess: (_data, archived) => {
      notifySuccess(archived ? PAGE_ARCHIVED_TITLE : PAGE_RESTORED_TITLE);
      publish(target, archived ? PAGE_EVENT_ARCHIVED : PAGE_EVENT_UNARCHIVED);
    },
    onError: (_error, archived, context) => {
      context?.rollback();
      notifyError(archived ? PAGE_ARCHIVE_FAILED_TITLE : PAGE_RESTORE_FAILED_TITLE);
    },
    onSettled: cache.invalidate,
  });
  return {
    pending: mutation.isPending,
    toggleArchive: (archived) => {
      if (mutation.isPending) return;
      mutation.mutate(archived);
    },
  };
}

/** Delete one already-shelved page (PAGE-016, shared with the detail). */
export function usePageDeleteWrite(target: PageRowTarget): {
  pending: boolean;
  remove: (options?: { onDeleted?: () => void }) => void;
} {
  const cache = useRowCache(target);
  const mutation = useMutation<void, Error, void>({
    mutationFn: () => deletePage(getAppClient(), target.workspaceSlug, target.projectId, target.page.id),
    // The row stays whole while the dialog holds its busy state; it leaves
    // only once the server confirms the page is gone.
    onSuccess: () => {
      cache.drop();
      notifySuccess(PAGE_DELETED_TITLE);
      publish(target, PAGE_EVENT_DELETED);
    },
    onError: () => {
      notifyError(PAGE_DELETE_FAILED_TITLE);
    },
    onSettled: cache.invalidate,
  });
  return {
    pending: mutation.isPending,
    remove: (options) => {
      if (mutation.isPending) return;
      mutation.mutate(undefined, {
        onSuccess: () => {
          options?.onDeleted?.();
        },
      });
    },
  };
}

/** Move one page into another project (PAGE-017, cloud only). */
export function usePageMoveWrite(target: PageRowTarget): {
  pending: boolean;
  move: (newProjectId: string) => void;
} {
  const cache = useRowCache(target);
  const mutation = useMutation<void, Error, string>({
    mutationFn: (newProjectId) =>
      movePageToProject(getAppClient(), target.workspaceSlug, target.projectId, target.page.id, {
        new_project_id: newProjectId,
      }),
    onSuccess: () => {
      cache.drop();
      notifySuccess(PAGE_MOVED_TITLE);
      publish(target, PAGE_EVENT_MOVED);
    },
    onError: () => {
      notifyError(PAGE_MOVE_FAILED_TITLE);
    },
    onSettled: cache.invalidate,
  });
  return {
    pending: mutation.isPending,
    move: (newProjectId) => {
      if (mutation.isPending) return;
      mutation.mutate(newProjectId);
    },
  };
}
