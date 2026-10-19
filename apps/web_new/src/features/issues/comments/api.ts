// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment data (parity/specs/issues.md §16, ISS-197–202). Reads go
// through the issue history with the comment activity type; writes run
// against the comment collection. Every write repaints the comment cache
// first (fold and access flip while the request is out), rolls back with
// a notice when refused, and re-reads on settle. Images upload before the
// comment exists and attach to it right after creation.

import { queryOptions, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  attachCommentImages,
  createIssueComment,
  deleteIssueComment,
  isApiError,
  listIssueComments,
  updateIssueComment,
  uploadCommentImage,
  type ApiClient,
  type CommentAccess,
  type IssueComment,
  type IssueCommentPatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries, type QuerySnapshot } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { patchCommentList, toggledAccess, withFold, type CommentPatch } from "./model.js";

/** Notice copy the comment flows raise; the oracle scenarios assert it. */
export const COMMENT_CREATED_TITLE = "Comment created successfully";
export const COMMENT_UPDATED_TITLE = "Comment updated successfully";
export const COMMENT_REMOVED_TITLE = "Comment removed successfully";
export const COMMENT_LINK_COPIED_TITLE = "Comment link copied to clipboard";
export const COMMENT_LINK_COPY_FAILED_TITLE = "Couldn't copy the comment link";
export const COMMENT_CREATE_FAILED_TITLE = "Couldn't add the comment";
export const COMMENT_UPDATE_FAILED_TITLE = "Couldn't update the comment";
export const COMMENT_REMOVE_FAILED_TITLE = "Couldn't delete the comment";
export const COMMENT_ATTACH_FAILED_TITLE = "Couldn't finish the comment upload";
export const COMMENT_IMAGE_UPLOAD_FAILED_TITLE = "Couldn't upload the image";
export const COMMENT_FAILURE_FALLBACK = "Check your connection and try again.";

/** Server detail for a comment failure notice, falling back when empty. */
export function commentFailureDetail(error: unknown): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return COMMENT_FAILURE_FALLBACK;
}

export interface CommentScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/** The issue's comments in creation order. */
export function commentsQueryOptions(client: ApiClient, scope: CommentScope) {
  return queryOptions({
    queryKey: issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId),
    queryFn: () =>
      listIssueComments(client, scope.workspaceSlug, scope.projectId, scope.issueId).then((rows) =>
        [...rows].sort((left, right) => left.created_at.localeCompare(right.created_at))
      ),
    staleTime: 15_000,
    refetchOnWindowFocus: false,
  });
}

export function useIssueComments(scope: CommentScope) {
  return useQuery(commentsQueryOptions(getAppClient(), scope));
}

/** Repaint one row in the comment cache, returning the snapshots. */
export async function paintComment(
  queryClient: QueryClient,
  scope: CommentScope,
  commentId: string,
  patch: CommentPatch
): Promise<QuerySnapshot[]> {
  const key = issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId);
  await queryClient.cancelQueries({ queryKey: key });
  const snapshots = snapshotQueries(queryClient, key);
  queryClient.setQueryData<IssueComment[]>(key, (previous) =>
    previous === undefined ? previous : patchCommentList(previous, commentId, patch)
  );
  return snapshots;
}

/** Drop one row from the comment cache, returning the snapshots. */
export async function removeCommentRow(
  queryClient: QueryClient,
  scope: CommentScope,
  commentId: string
): Promise<QuerySnapshot[]> {
  const key = issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId);
  await queryClient.cancelQueries({ queryKey: key });
  const snapshots = snapshotQueries(queryClient, key);
  queryClient.setQueryData<IssueComment[]>(key, (previous) =>
    previous === undefined ? previous : previous.filter((comment) => comment.id !== commentId)
  );
  return snapshots;
}

function authorPatch(comment: IssueComment): CommentPatch {
  return { comment_html: comment.comment_html, edited_at: comment.edited_at };
}

/** Write one comment change (body, access or labels) with its notice. */
export function useUpdateComment(scope: CommentScope, failureTitle: string, successTitle: string) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ commentId, patch }: { commentId: string; patch: IssueCommentPatch }) =>
      updateIssueComment(client, scope.workspaceSlug, scope.projectId, scope.issueId, commentId, patch),
    onMutate: async ({ commentId, patch }) => {
      const optimistic: CommentPatch = {
        ...(patch.labels === undefined ? {} : { labels: patch.labels }),
        ...(patch.access === undefined ? {} : { access: patch.access }),
        ...(patch.comment_html === undefined ? {} : { comment_html: patch.comment_html }),
        ...(patch.comment_html === undefined ? {} : { edited_at: new Date().toISOString() }),
      };
      return paintComment(queryClient, scope, commentId, optimistic);
    },
    onSuccess: (comment) => {
      notifySuccess(successTitle);
      const key = issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId);
      // An unread thread puts the row in place for its own first read,
      // which then merges the server list over it.
      queryClient.setQueryData<IssueComment[]>(key, (previous) =>
        patchCommentList(previous ?? [], comment.id, {
          ...authorPatch(comment),
          labels: comment.labels,
          access: comment.access,
        })
      );
    },
    onError: (error: Error, _variables, snapshots: QuerySnapshot[] | undefined) => {
      if (snapshots !== undefined) rollbackSnapshots(queryClient, snapshots);
      notifyError(failureTitle, commentFailureDetail(error));
    },
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId),
      }),
  });
}

/** Delete a comment (ISS-199): no confirmation, one notice. */
export function useDeleteComment(scope: CommentScope) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (commentId: string) =>
      deleteIssueComment(client, scope.workspaceSlug, scope.projectId, scope.issueId, commentId),
    onMutate: (commentId: string) => removeCommentRow(queryClient, scope, commentId),
    onSuccess: () => notifySuccess(COMMENT_REMOVED_TITLE),
    onError: (error: Error, _commentId, snapshots: QuerySnapshot[] | undefined) => {
      if (snapshots !== undefined) rollbackSnapshots(queryClient, snapshots);
      notifyError(COMMENT_REMOVE_FAILED_TITLE, commentFailureDetail(error));
    },
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId),
      }),
  });
}

export interface CreateCommentInput {
  html: string;
  access: CommentAccess;
  /** Images uploaded while composing; attached once the comment exists. */
  assetIds?: readonly string[];
}

/** Post a comment, attach its images, and refresh the list (ISS-197). */
export function useCreateComment(scope: CommentScope) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ html, access, assetIds = [] }: CreateCommentInput) => {
      const comment = await createIssueComment(client, scope.workspaceSlug, scope.projectId, scope.issueId, {
        comment_html: html,
        access,
      });
      if (assetIds.length > 0) {
        try {
          await attachCommentImages(client, scope.workspaceSlug, scope.projectId, comment.id, assetIds);
        } catch (error) {
          notifyError(COMMENT_ATTACH_FAILED_TITLE, commentFailureDetail(error));
        }
      }
      return comment;
    },
    onSuccess: (comment) => {
      notifySuccess(COMMENT_CREATED_TITLE);
      queryClient.setQueryData<IssueComment[]>(
        issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId),
        (previous) => {
          if (previous === undefined || previous.some((row) => row.id === comment.id)) return previous;
          return [...previous, comment].sort((left, right) => left.created_at.localeCompare(right.created_at));
        }
      );
    },
    onError: (error: Error) => notifyError(COMMENT_CREATE_FAILED_TITLE, commentFailureDetail(error)),
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId),
      }),
  });
}

/** One image the composer embeds: the parked asset plus its shown address. */
export interface ComposerImage {
  assetId: string;
  /** Address the editor embeds; relative to the API origin. */
  src: string;
}

/**
 * Upload an inline image while the composer is open (ISS-197). The
 * post-composer upload stays parked until the comment is created; an
 * edit-composer upload attaches to the existing comment right away.
 */
export function useUploadCommentImage(scope: Pick<CommentScope, "workspaceSlug" | "projectId">) {
  const client = getAppClient();
  return useMutation({
    mutationFn: async (input: { file: File; commentId?: string }): Promise<ComposerImage> => {
      const uploaded = await uploadCommentImage(client, scope.workspaceSlug, scope.projectId, input.file);
      if (input.commentId !== undefined) {
        try {
          await attachCommentImages(client, scope.workspaceSlug, scope.projectId, input.commentId, [uploaded.assetId]);
        } catch (error) {
          notifyError(COMMENT_ATTACH_FAILED_TITLE, commentFailureDetail(error));
        }
      }
      return { assetId: uploaded.assetId, src: uploaded.assetUrl };
    },
    onError: (error: Error) => notifyError(COMMENT_IMAGE_UPLOAD_FAILED_TITLE, commentFailureDetail(error)),
  });
}

/** Patch that writes the fold marker through the shared update mutation. */
export function foldPatch(comment: Pick<IssueComment, "labels">, folded: boolean): IssueCommentPatch {
  return { labels: withFold(comment, folded) };
}

/** Patch that flips the access specifier through the shared update mutation. */
export function accessPatch(comment: Pick<IssueComment, "access">): IssueCommentPatch {
  return { access: toggledAccess(comment) };
}
