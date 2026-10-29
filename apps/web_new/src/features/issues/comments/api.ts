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
  duplicateCommentImage,
  getIssueComment,
  gitSyncedCommentErrorFrom,
  isApiError,
  listIssueComments,
  searchMentionPeople,
  updateIssueComment,
  uploadCommentImage,
  type ApiClient,
  type CommentAccess,
  type IssueComment,
  type IssueCommentPatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { rollbackSnapshots, snapshotQueries, type QuerySnapshot } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { mergeDeltaRows, newestCreatedAt } from "../api/incremental.js";
import { issueKeys } from "../api/keys.js";
import {
  commentBodyImageAssetIds,
  patchCommentList,
  rewriteCommentImageSrcs,
  toggledAccess,
  withFold,
  type CommentPatch,
} from "./model.js";

/** Notice copy the comment flows raise; the oracle scenarios assert it. */
export const COMMENT_CREATED_TITLE = "Comment created successfully";
export const COMMENT_UPDATED_TITLE = "Comment updated successfully";
export const COMMENT_REMOVED_TITLE = "Comment removed successfully";
export const COMMENT_LINK_COPIED_TITLE = "Comment link copied to clipboard";
export const COMMENT_LINK_COPY_FAILED_TITLE = "Couldn't copy the comment link";
export const COMMENT_CREATE_FAILED_TITLE = "Couldn't add the comment";
export const COMMENT_UPDATE_FAILED_TITLE = "Couldn't update the comment";
export const COMMENT_REMOVE_FAILED_TITLE = "Couldn't delete the comment";
export const COMMENT_SYNCED_REMOVE_FAILED_TITLE = "Failed to delete the synced comment";
export const COMMENT_ATTACH_FAILED_TITLE = "Couldn't finish the comment upload";
export const COMMENT_IMAGE_UPLOAD_FAILED_TITLE = "Couldn't upload the image";
export const COMMENT_FAILURE_FALLBACK = "Check your connection and try again.";

/**
 * Drop every read that shows the issue's comments. The activity feed
 * keeps its own list of comment rows (ISS-194), so a comment write has to
 * refetch both or the timeline shows a stale set.
 */
function invalidateCommentViews(queryClient: QueryClient, scope: CommentScope): Promise<void> {
  return Promise.all([
    queryClient.invalidateQueries({
      queryKey: issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId),
    }),
    queryClient.invalidateQueries({
      queryKey: issueKeys.activity(scope.workspaceSlug, scope.projectId, scope.issueId),
    }),
  ]).then(() => undefined);
}

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

/**
 * The issue's comments in creation order. With the query client the
 * refetch is incremental (parity/specs/comments.md §4.5, CMT-017): it
 * reads the newest cached timestamp as its `created_at__gt` cursor and
 * merges the delta. Without it (or on an empty cache) the read fetches
 * the whole list.
 */
export function commentsQueryOptions(client: ApiClient, scope: CommentScope, queryClient?: QueryClient) {
  const key = issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId);
  return queryOptions({
    queryKey: key,
    queryFn: async () => {
      const cached = queryClient?.getQueryData<IssueComment[]>(key);
      const cursor = cached === undefined ? undefined : newestCreatedAt(cached);
      if (cached === undefined || cursor === undefined) {
        const rows = await listIssueComments(client, scope.workspaceSlug, scope.projectId, scope.issueId);
        return [...rows].sort((left, right) => left.created_at.localeCompare(right.created_at));
      }
      const delta = await listIssueComments(client, scope.workspaceSlug, scope.projectId, scope.issueId, {
        createdAfter: cursor,
      });
      return mergeDeltaRows(cached, delta).sort((left, right) => left.created_at.localeCompare(right.created_at));
    },
    staleTime: 15_000,
    refetchOnWindowFocus: false,
  });
}

export function useIssueComments(scope: CommentScope) {
  const queryClient = useQueryClient();
  return useQuery(commentsQueryOptions(getAppClient(), scope, queryClient));
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

/**
 * The patch a card paints while its write is still out. Fold and access
 * are presentation flags whose flip reads as instant feedback, and a
 * refusal rolls them back.
 *
 * A body change is not painted here (ISS-198). Painting it would show
 * the new words and the edited marker before the PATCH resolves, so
 * every reader of the stored comment — a reload, another tab, a fresh
 * API read — still sees the pre-edit body while the card claims the
 * edit landed. The body and the marker therefore wait for the confirmed
 * row in `onSuccess`; the editor stays open under its own saving state
 * until then, so nothing flickers.
 */
export function pendingCommentPatch(patch: IssueCommentPatch): CommentPatch {
  return {
    ...(patch.labels === undefined ? {} : { labels: patch.labels }),
    ...(patch.access === undefined ? {} : { access: patch.access }),
  };
}

/** Write one comment change (body, access or labels) with its notice. */
export function useUpdateComment(scope: CommentScope, failureTitle: string, successTitle: string) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ commentId, patch }: { commentId: string; patch: IssueCommentPatch }) =>
      updateIssueComment(client, scope.workspaceSlug, scope.projectId, scope.issueId, commentId, patch),
    onMutate: async ({ commentId, patch }) => paintComment(queryClient, scope, commentId, pendingCommentPatch(patch)),
    onSuccess: (comment) => {
      notifySuccess(successTitle);
      const key = issueKeys.comments(scope.workspaceSlug, scope.projectId, scope.issueId);
      // An unread thread picks the confirmed row up on its own first read.
      queryClient.setQueryData<IssueComment[]>(key, (previous) =>
        previous === undefined
          ? previous
          : patchCommentList(previous, comment.id, {
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
    onSettled: () => invalidateCommentViews(queryClient, scope),
  });
}

/**
 * Delete a comment (ISS-199): no confirmation, one notice. A comment
 * mirrored from a git provider is refused with 409 instead: the row rolls
 * back and one notice carries the server's unbind instruction (CMT-005).
 */
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
      const synced = gitSyncedCommentErrorFrom(error);
      if (synced === null) {
        notifyError(COMMENT_REMOVE_FAILED_TITLE, commentFailureDetail(error));
      } else {
        notifyError(COMMENT_SYNCED_REMOVE_FAILED_TITLE, synced.message);
      }
    },
    onSettled: () => invalidateCommentViews(queryClient, scope),
  });
}

export interface CreateCommentInput {
  html: string;
  access: CommentAccess;
  /** Images uploaded while composing; attached once the comment exists. */
  assetIds?: readonly string[];
}

/** How long a create waits for attached pictures to reach the stored body. */
export interface CommentImageWait {
  /** Pause between re-reads. Defaults to 500ms. */
  intervalMs?: number;
  /** Give up after this long. Defaults to 30s. */
  timeoutMs?: number;
}

/**
 * Re-read a comment until its stored body carries the attached pictures
 * (CMT-009). The bulk link answers 204 before the rewritten body is
 * readable, so the first re-read usually still shows the pre-attach
 * HTML; caching that row would hide the pictures until a reload (the
 * feed's delta refresh only adds newer rows). Resolves with the first
 * row whose body embeds an image, or the last row read when the wait
 * runs out; rejects when no re-read succeeds.
 */
export async function readCommentWithImages(
  client: ApiClient,
  scope: CommentScope,
  commentId: string,
  wait: CommentImageWait = {}
): Promise<IssueComment> {
  const intervalMs = wait.intervalMs ?? 500;
  const timeoutMs = wait.timeoutMs ?? 30_000;
  const started = Date.now();
  for (;;) {
    const row = await getIssueComment(client, scope.workspaceSlug, scope.projectId, scope.issueId, commentId);
    if (row.comment_html.includes("image-component")) return row;
    if (Date.now() - started >= timeoutMs) return row;
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
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
      if (assetIds.length === 0) return comment;
      try {
        await attachCommentImages(client, scope.workspaceSlug, scope.projectId, comment.id, assetIds);
      } catch (error) {
        notifyError(COMMENT_ATTACH_FAILED_TITLE, commentFailureDetail(error));
        return comment;
      }
      // The bulk link answers 204 before the rewritten body is
      // readable: wait for the pictures, and keep the create response
      // when no re-read succeeds rather than failing the post (CMT-009).
      try {
        return await readCommentWithImages(client, scope, comment.id);
      } catch {
        return comment;
      }
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
    onSettled: () => invalidateCommentViews(queryClient, scope),
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

/**
 * Duplicate the images an edit keeps so the new revision owns its
 * pictures (CMT-009). Ids in both bodies are duplicated and rebound;
 * added uploads and dropped pictures need no call. Rejects when a
 * duplicate is refused, so the caller fails the save instead of
 * storing a body that points at the old revision's assets.
 */
export async function duplicateKeptCommentImages(
  client: ApiClient,
  scope: { workspaceSlug: string; projectId: string; commentId: string },
  originalHtml: string,
  nextHtml: string
): Promise<string> {
  const original = new Set(commentBodyImageAssetIds(originalHtml));
  const kept = commentBodyImageAssetIds(nextHtml).filter((id) => original.has(id));
  if (kept.length === 0) return nextHtml;
  const mapping = new Map<string, string>();
  for (const id of kept) {
    const duplicate = await duplicateCommentImage(client, scope.workspaceSlug, id, {
      commentId: scope.commentId,
      projectId: scope.projectId,
    });
    mapping.set(id, duplicate);
  }
  return rewriteCommentImageSrcs(nextHtml, mapping);
}

/** How many people one comment mention search may offer (CMT-019). */
export const COMMENT_MENTION_LIMIT = 5;

/** One mention suggestion row the comment @ popup renders. */
export interface CommentMentionPerson {
  id: string;
  name: string;
  avatarUrl: string | null;
}

/** Mention lookup key, nested under the workspace like every read. */
export function commentMentionSearchKey(workspaceSlug: string, projectId: string, text: string) {
  return [...workspaceScope(workspaceSlug), "issues", "comments", "mention-search", projectId, text] as const;
}

/** People matching the text typed after an @; none without a project. */
export async function requestCommentMentions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  query: string
): Promise<CommentMentionPerson[]> {
  if (projectId === "") return [];
  const people = await searchMentionPeople(client, workspaceSlug, {
    query,
    projectId,
    count: COMMENT_MENTION_LIMIT,
  });
  return people.map((person) => ({
    id: person.member__id,
    name: person.member__display_name,
    avatarUrl: person.member__avatar_url,
  }));
}

/** Mention lookup for the comment composer and edit form (CMT-019). */
export function useCommentMentionSearch(workspaceSlug: string, projectId: string, query: string, enabled: boolean) {
  return useQuery({
    queryKey: commentMentionSearchKey(workspaceSlug, projectId, query),
    queryFn: () => requestCommentMentions(getAppClient(), workspaceSlug, projectId, query),
    staleTime: REFERENCE_STALE_TIME_MS,
    enabled: enabled && projectId !== "",
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
