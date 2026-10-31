// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-link reads and writes (SHELL-011–013, 023; written from
// parity/specs/shell.md §2). The list fetches once per workspace and never
// refetches on focus or reconnect; a failed fetch is logged and keeps
// prior state with no retry affordance. Creates, edits and deletes paint
// optimistically, roll back with a failure notice on error, and refresh
// the list once settled.

import { mutationOptions, queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import {
  createQuickLink,
  deleteQuickLink,
  listQuickLinks,
  updateQuickLink,
  type ApiClient,
  type QuickLink,
  type QuickLinkList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { logHomeFetch } from "./fetch.js";

export const quicklinksKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "home", "quick-links"] as const,
  list: (workspaceSlug: string) => [...quicklinksKeys.all(workspaceSlug), "list"] as const,
};

export function quicklinksQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: quicklinksKeys.list(workspaceSlug),
    queryFn: () => logHomeFetch(`quick-links/${workspaceSlug}`, () => listQuickLinks(client, workspaceSlug)),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/** The workspace's saved links, fetched once and never silently refreshed. */
export function useQuickLinks(workspaceSlug: string) {
  return useQuery(quicklinksQueryOptions(getAppClient(), workspaceSlug));
}

export interface QuickLinkDraft {
  url: string;
  title: string;
}

export interface QuickLinkEdit extends QuickLinkDraft {
  id: string;
}

interface MutationContext {
  rollbacks: Rollback[];
}

function failureMessage(error: unknown): string | undefined {
  return error instanceof Error ? error.message : undefined;
}

function refreshLinks(queryClient: QueryClient, workspaceSlug: string): void {
  void queryClient.invalidateQueries({ queryKey: quicklinksKeys.all(workspaceSlug) });
}

let optimisticSeq = 0;

function optimisticLink(draft: QuickLinkDraft): QuickLink {
  optimisticSeq += 1;
  const now = new Date().toISOString();
  return {
    id: `quicklink-pending-${optimisticSeq}`,
    workspace: "",
    owner: "",
    title: draft.title,
    url: draft.url,
    metadata: {},
    created_by: null,
    updated_by: null,
    created_at: now,
    updated_at: now,
  };
}

export function createQuickLinkMutationOptions(client: ApiClient, queryClient: QueryClient, workspaceSlug: string) {
  return mutationOptions({
    mutationFn: (draft: QuickLinkDraft) =>
      createQuickLink(client, workspaceSlug, draft.title === "" ? { url: draft.url } : draft),
    onMutate: (draft: QuickLinkDraft): MutationContext => ({
      rollbacks: [
        applyOptimisticUpdate<QuickLinkList>(queryClient, quicklinksKeys.list(workspaceSlug), (previous) => [
          ...(previous ?? []),
          optimisticLink(draft),
        ]),
      ],
    }),
    onError: (error: unknown, _draft: QuickLinkDraft, context: MutationContext | undefined) => {
      context?.rollbacks.forEach((rollback) => {
        rollback();
      });
      notifyError("Link save failed", failureMessage(error));
    },
    onSuccess: () => {
      notifySuccess("Link saved");
    },
    onSettled: () => {
      refreshLinks(queryClient, workspaceSlug);
    },
  });
}

/** Saves a new link with an optimistic row. */
export function useCreateQuickLink(workspaceSlug: string) {
  return useMutation(createQuickLinkMutationOptions(getAppClient(), useQueryClient(), workspaceSlug));
}

export function updateQuickLinkMutationOptions(client: ApiClient, queryClient: QueryClient, workspaceSlug: string) {
  return mutationOptions({
    mutationFn: (edit: QuickLinkEdit) =>
      updateQuickLink(client, workspaceSlug, edit.id, { url: edit.url, title: edit.title }),
    onMutate: (edit: QuickLinkEdit): MutationContext => ({
      rollbacks: [
        applyOptimisticUpdate<QuickLinkList>(queryClient, quicklinksKeys.list(workspaceSlug), (previous) =>
          previous?.map((link) =>
            link.id === edit.id
              ? { ...link, url: edit.url, title: edit.title, updated_at: new Date().toISOString() }
              : link
          )
        ),
      ],
    }),
    onError: (error: unknown, _edit: QuickLinkEdit, context: MutationContext | undefined) => {
      context?.rollbacks.forEach((rollback) => {
        rollback();
      });
      notifyError("Link update failed", failureMessage(error));
    },
    onSuccess: () => {
      notifySuccess("Link updated");
    },
    onSettled: () => {
      refreshLinks(queryClient, workspaceSlug);
    },
  });
}

/** Renames and retargets a link with an optimistic paint. */
export function useUpdateQuickLink(workspaceSlug: string) {
  return useMutation(updateQuickLinkMutationOptions(getAppClient(), useQueryClient(), workspaceSlug));
}

export function deleteQuickLinkMutationOptions(client: ApiClient, queryClient: QueryClient, workspaceSlug: string) {
  return mutationOptions({
    mutationFn: (id: string) => deleteQuickLink(client, workspaceSlug, id),
    onMutate: (id: string): MutationContext => ({
      rollbacks: [
        applyOptimisticUpdate<QuickLinkList>(queryClient, quicklinksKeys.list(workspaceSlug), (previous) =>
          previous?.filter((link) => link.id !== id)
        ),
      ],
    }),
    onError: (error: unknown, _id: string, context: MutationContext | undefined) => {
      context?.rollbacks.forEach((rollback) => {
        rollback();
      });
      notifyError("Link delete failed", failureMessage(error));
    },
    onSuccess: () => {
      notifySuccess("Link deleted");
    },
    onSettled: () => {
      refreshLinks(queryClient, workspaceSlug);
    },
  });
}

/** Removes a link with an optimistic row drop. */
export function useDeleteQuickLink(workspaceSlug: string) {
  return useMutation(deleteQuickLinkMutationOptions(getAppClient(), useQueryClient(), workspaceSlug));
}
