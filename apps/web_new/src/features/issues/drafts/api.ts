// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-draft data layer (parity/specs/issues.md §12, ISS-137–141).
// The list is one cursor-paged infinite query at fifty rows per page;
// inline picks patch one draft and repaint every cached page, delete and
// promote drop the row (and its count) at once, and a create re-reads the
// list so the new block joins the top of it.

import { infiniteQueryOptions, useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  createDraftIssue,
  deleteDraftIssue,
  isApiError,
  listDraftIssues,
  promoteDraftIssue,
  updateDraftIssue,
  type ApiClient,
  type DraftIssue,
  type DraftIssueCreateInput,
  type DraftIssueListResponse,
  type PromotedIssue,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { rollbackSnapshots, snapshotQueries, type QuerySnapshot } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

/** Rows per page (spec §12: fifty, cursor-paged). */
export const DRAFT_PAGE_SIZE = 50;

/** Toast copy for the draft writes the page drives. */
export const DRAFT_DELETED_TITLE = "Success!";
export const DRAFT_DELETED_BODY = "Draft deleted";
export const DRAFT_DELETE_FAILED_TITLE = "Couldn't delete the work item";
export const DRAFT_DELETE_FAILED_BODY = "Check your connection and try again.";
export const DRAFT_DELETE_PERMISSION_TITLE = "You don't have permission to perform this action";
export const DRAFT_DELETE_PERMISSION_BODY = "Only a workspace admin or the creator can perform this action.";
export const DRAFT_UPDATE_FAILED_TITLE = "Couldn't save the change";
const DRAFT_UPDATE_FAILED_BODY = "The draft is unchanged — check your connection and try again.";
export const DRAFT_UPDATE_PERMISSION_BODY = "Only a workspace admin or this draft's creator can change it.";
export const DRAFT_PROMOTE_PERMISSION_TITLE = "You don't have permission to perform this action";
export const DRAFT_PROMOTE_PERMISSION_BODY = "Only a workspace member or admin can publish a draft.";
export const DRAFT_CREATED_TITLE = "Success!";
export const DRAFT_CREATED_BODY = "Draft created";
export const DRAFT_CREATE_FAILED_TITLE = "Error!";
export const DRAFT_CREATE_FAILED_BODY = "Work item could not be created. Please try again.";
export const DRAFT_PROMOTED_TITLE = "Success";
export const DRAFT_PROMOTED_BODY = "Draft published to project.";
export const DRAFT_PROMOTE_FAILED_TITLE = "Error!";
export const DRAFT_PROMOTE_FAILED_BODY = "Failed to move work item to project. Please try again.";

/** Server wording for the creator-or-admin refusal on delete. */
const SERVER_PERMISSION_REFUSAL = "Only admin or creator can delete the work item";

export const draftKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "issue-drafts"] as const,
  list: (workspaceSlug: string) => [...draftKeys.all(workspaceSlug), "list"] as const,
};

/** Every loaded page of the workspace drafts, newest first. */
export function draftListQueryOptions(client: ApiClient, workspaceSlug: string) {
  return infiniteQueryOptions({
    queryKey: draftKeys.list(workspaceSlug),
    queryFn: ({ pageParam }) =>
      listDraftIssues(client, workspaceSlug, {
        perPage: DRAFT_PAGE_SIZE,
        ...(pageParam === "" ? {} : { cursor: pageParam }),
      }),
    initialPageParam: "",
    getNextPageParam: (last: DraftIssueListResponse) =>
      last.next_page_results && last.next_cursor !== "" ? last.next_cursor : undefined,
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** Flatten the loaded pages into the rows the page renders, in server order. */
export function flattenDraftPages(data: { pages: DraftIssueListResponse[] } | undefined): DraftIssue[] {
  if (data === undefined) return [];
  const seen = new Set<string>();
  const rows: DraftIssue[] = [];
  for (const page of data.pages) {
    for (const draft of page.results) {
      if (seen.has(draft.id)) continue;
      seen.add(draft.id);
      rows.push(draft);
    }
  }
  return rows;
}

/** The server total from the newest page, or the loaded count. */
export function draftTotalCount(data: { pages?: DraftIssueListResponse[] } | undefined): number {
  const pages = data?.pages ?? [];
  return pages[pages.length - 1]?.total_count ?? 0;
}

type DraftPageCache = { pages: DraftIssueListResponse[]; pageParams?: unknown[] };

/**
 * Rewrite every page that holds a draft, leaving other pages as they were.
 * An absent cache (no page has landed yet) stays absent.
 */
export function mapPagesHolding(
  current: DraftPageCache | undefined,
  draftId: string,
  rewrite: (page: DraftIssueListResponse) => DraftIssueListResponse
): DraftPageCache | undefined {
  if (current === undefined) return current;
  return {
    ...current,
    pages: current.pages.map((page) => (page.results.some((draft) => draft.id === draftId) ? rewrite(page) : page)),
  };
}

/** Apply a field write to one draft inside every cached page. */
export function paintDraftPatch(
  queryClient: QueryClient,
  workspaceSlug: string,
  draftId: string,
  patch: DraftIssueCreateInput
): void {
  queryClient.setQueriesData<DraftPageCache>({ queryKey: draftKeys.list(workspaceSlug) }, (current) =>
    mapPagesHolding(current, draftId, (page) => ({
      ...page,
      results: page.results.map((draft) => (draft.id === draftId ? { ...draft, ...patch } : draft)),
    }))
  );
}

/** Drop one draft from every cached page and lower the stored total. */
export function removeDraftFromCache(queryClient: QueryClient, workspaceSlug: string, draftId: string): void {
  queryClient.setQueriesData<DraftPageCache>({ queryKey: draftKeys.list(workspaceSlug) }, (current) =>
    mapPagesHolding(current, draftId, (page) => ({
      ...page,
      total_count: Math.max(0, page.total_count - 1),
      results: page.results.filter((draft) => draft.id !== draftId),
    }))
  );
}

function invalidateDrafts(queryClient: QueryClient, workspaceSlug: string): void {
  void queryClient.invalidateQueries({ queryKey: draftKeys.all(workspaceSlug) });
}

/** True when the server refused a delete on creator-or-admin grounds. */
export function isDraftPermissionRefusal(error: unknown): boolean {
  if (isApiError(error)) {
    if (error.status === 403) return true;
    return error.message.includes(SERVER_PERMISSION_REFUSAL);
  }
  return error instanceof Error && error.message.includes(SERVER_PERMISSION_REFUSAL);
}

/** An inline property pick: one draft, one patch, optimistic paint. */
export function useUpdateDraftField(workspaceSlug: string) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation<void, Error, { draftId: string; patch: DraftIssueCreateInput }, QuerySnapshot[]>({
    mutationFn: ({ draftId, patch }) => updateDraftIssue(client, workspaceSlug, draftId, patch),
    onMutate: ({ draftId, patch }) => {
      const snapshots = snapshotQueries(queryClient, draftKeys.list(workspaceSlug));
      paintDraftPatch(queryClient, workspaceSlug, draftId, patch);
      return snapshots;
    },
    onError: (error, _input, snapshots) => {
      if (snapshots !== undefined) rollbackSnapshots(queryClient, snapshots);
      if (isDraftPermissionRefusal(error)) {
        notifyError(DRAFT_DELETE_PERMISSION_TITLE, DRAFT_UPDATE_PERMISSION_BODY);
      } else {
        notifyError(DRAFT_UPDATE_FAILED_TITLE, DRAFT_UPDATE_FAILED_BODY);
      }
    },
  });
}

/** Delete one draft; the row leaves the page the moment the server agrees. */
export function useDeleteDraft(workspaceSlug: string) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: async (draftId) => {
      await deleteDraftIssue(client, workspaceSlug, draftId);
    },
    onSuccess: (_result, draftId) => {
      removeDraftFromCache(queryClient, workspaceSlug, draftId);
      notifySuccess(DRAFT_DELETED_TITLE, DRAFT_DELETED_BODY);
      invalidateDrafts(queryClient, workspaceSlug);
    },
  });
}

/** Promote a draft into its project; the draft is deleted by the server. */
export function usePromoteDraft(workspaceSlug: string) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation<PromotedIssue, Error, { draftId: string; input: DraftIssueCreateInput }>({
    mutationFn: ({ draftId, input }) => promoteDraftIssue(client, workspaceSlug, draftId, input),
    onSuccess: (_promoted, { draftId }) => {
      removeDraftFromCache(queryClient, workspaceSlug, draftId);
      notifySuccess(DRAFT_PROMOTED_TITLE, DRAFT_PROMOTED_BODY);
      invalidateDrafts(queryClient, workspaceSlug);
    },
  });
}

/** Create a draft (the page's create entry and the modal's copy save). */
export function useCreateDraft(workspaceSlug: string) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation<DraftIssue, Error, DraftIssueCreateInput>({
    mutationFn: (input) => createDraftIssue(client, workspaceSlug, input),
    onSuccess: () => {
      notifySuccess(DRAFT_CREATED_TITLE, DRAFT_CREATED_BODY);
      invalidateDrafts(queryClient, workspaceSlug);
    },
    onError: () => {
      notifyError(DRAFT_CREATE_FAILED_TITLE, DRAFT_CREATE_FAILED_BODY);
    },
  });
}
