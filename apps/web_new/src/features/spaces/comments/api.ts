// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public comment writes (parity/specs/space.md §3.10, SPC-053–057). The
// composer posts through the public comment collection and links its
// confirmed image assets afterwards; edits and deletes repaint the
// thread cache first and reconcile from a fresh list when refused;
// reactions overlay one pending toggle on the server rows. Every write
// reports through slice D's notices, and anonymous reaction attempts
// leave for sign-in instead of writing.

import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  addSpaceCommentReaction,
  completeSpaceAssetUpload,
  createSpaceComment,
  deleteSpaceComment,
  linkSpaceAssets,
  removeSpaceCommentReaction,
  requestSpaceAssetUpload,
  updateSpaceComment,
  uploadSpaceAssetBytes,
  type ApiClient,
  type SpaceAssetByteUpload,
  type SpaceComment,
} from "@pidash/api-client";
import * as React from "react";

import { rollbackSnapshots, snapshotQueries, type QuerySnapshot } from "../../../core/query/optimistic.js";
import { spaceKeys } from "../api/keys.js";
import { spaceMembersQueryOptions, spaceViewerQueryOptions } from "../api/queries.js";
import { nextSpaceReaction, type SpaceReactionGroup, type SpaceReactionWrite } from "../engagement/model.js";
import {
  notifySpaceAttachmentFailure,
  notifySpaceCommentDeleteFailure,
  notifySpaceCommentDeleteSuccess,
  notifySpaceCommentEditFailure,
  notifySpaceCommentEditSuccess,
  notifySpaceCommentFailure,
  notifySpaceReactionFailure,
} from "../lib/spaceNotices.js";
import {
  groupSpaceCommentReactions,
  matchSpaceMentionMembers,
  overlaySpaceCommentReaction,
  spaceCommentImageSrc,
  viewerSpaceCommentReacted,
  type SpaceMentionPerson,
} from "./model.js";

/** Re-read one issue's stored thread after a comment write settles. */
export function invalidateSpaceComments(queryClient: QueryClient, anchor: string, issueId: string): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: spaceKeys.comments(anchor, issueId) }).then(() => undefined);
}

/**
 * People the live @-query matched, capped for the popup (SPC-054). The
 * board members are read once per visit; the query only filters them.
 */
export function useSpaceMentionPeople(
  client: ApiClient,
  anchor: string,
  query: string,
  enabled: boolean
): SpaceMentionPerson[] {
  const options = spaceMembersQueryOptions(client, anchor);
  const members = useQuery({ ...options, enabled: enabled && options.enabled !== false });
  return matchSpaceMentionMembers(members.data ?? [], query);
}

/** One image the composer embeds: the confirmed asset plus its shown address. */
export interface SpaceComposerImage {
  assetId: string;
  /** Address the editor embeds; relative to the API origin. */
  src: string;
}

export interface SpaceCommentUploadInput {
  file: File;
  /** Comment id when editing; empty for a new comment. */
  commentId?: string | undefined;
}

/**
 * Upload an inline image while a comment form is open (SPC-053): park
 * the metadata, post the bytes to the signed grant, then confirm (the
 * asset 404s until the confirm lands). A refused leg reports through
 * the attachment notice and the draft keeps no node for it.
 */
export function useUploadSpaceCommentImage(client: ApiClient, anchor: string, upload?: SpaceAssetByteUpload) {
  return useMutation<SpaceComposerImage, Error, SpaceCommentUploadInput>({
    mutationFn: async ({ file, commentId }: SpaceCommentUploadInput): Promise<SpaceComposerImage> => {
      const grant = await requestSpaceAssetUpload(client, anchor, {
        name: file.name,
        type: file.type,
        size: file.size,
        ...(commentId === undefined ? {} : { entityIdentifier: commentId }),
      });
      const form = new FormData();
      for (const [name, value] of Object.entries(grant.upload_data.fields)) {
        form.append(name, value);
      }
      form.append("file", file);
      const bytes =
        upload === undefined
          ? await uploadSpaceAssetBytes(grant, form)
          : await uploadSpaceAssetBytes(grant, form, upload);
      if (!bytes.ok) throw new Error(`space comment image upload failed with status ${bytes.status}`);
      await completeSpaceAssetUpload(client, anchor, grant.asset_id);
      return { assetId: grant.asset_id, src: spaceCommentImageSrc(client.baseUrl, anchor, grant.asset_id) };
    },
    onError: () => notifySpaceAttachmentFailure(),
  });
}

export interface CreateSpaceCommentInput {
  html: string;
  /** Images confirmed while composing; linked once the comment exists. */
  assetIds?: readonly string[];
}

/**
 * Post a comment and link its images (SPC-053). The new row joins the
 * thread cache on success with no notice — the row is its own
 * confirmation. A refused post reports and leaves the cached thread
 * untouched; the composer keeps the draft for a retry.
 */
export function useCreateSpaceComment(client: ApiClient, anchor: string, issueId: string) {
  const queryClient = useQueryClient();
  return useMutation<SpaceComment, Error, CreateSpaceCommentInput>({
    mutationFn: async ({ html, assetIds = [] }: CreateSpaceCommentInput): Promise<SpaceComment> => {
      const comment = await createSpaceComment(client, anchor, issueId, { commentHtml: html });
      if (assetIds.length > 0) {
        try {
          await linkSpaceAssets(client, anchor, comment.id, [...assetIds]);
        } catch {
          notifySpaceAttachmentFailure();
        }
      }
      return comment;
    },
    onSuccess: (comment) => {
      queryClient.setQueryData<SpaceComment[]>(spaceKeys.comments(anchor, issueId), (previous) => {
        if (previous === undefined || previous.some((row) => row.id === comment.id)) return previous;
        return [...previous, comment].sort((left, right) => left.created_at.localeCompare(right.created_at));
      });
    },
    onError: () => notifySpaceCommentFailure(),
    onSettled: () => invalidateSpaceComments(queryClient, anchor, issueId),
  });
}

export interface UpdateSpaceCommentInput {
  commentId: string;
  html: string;
}

/**
 * Save a comment edit (SPC-056): the row swaps immediately, the
 * confirmed body lands on success with a notice, and a refusal rolls
 * the optimistic patch back and re-reads the stored thread so the view
 * never disagrees with the server (NEWFRONT-467).
 */
export function useUpdateSpaceComment(client: ApiClient, anchor: string, issueId: string) {
  const queryClient = useQueryClient();
  return useMutation<SpaceComment, Error, UpdateSpaceCommentInput, QuerySnapshot[]>({
    mutationFn: ({ commentId, html }: UpdateSpaceCommentInput) =>
      updateSpaceComment(client, anchor, issueId, commentId, { commentHtml: html }),
    onMutate: async ({ commentId, html }: UpdateSpaceCommentInput) => {
      const key = spaceKeys.comments(anchor, issueId);
      await queryClient.cancelQueries({ queryKey: key });
      const snapshots = snapshotQueries(queryClient, key);
      queryClient.setQueryData<SpaceComment[]>(key, (previous) =>
        previous === undefined
          ? previous
          : previous.map((row) => (row.id === commentId ? { ...row, comment_html: html } : row))
      );
      return snapshots;
    },
    onSuccess: (comment) => {
      notifySpaceCommentEditSuccess();
      queryClient.setQueryData<SpaceComment[]>(spaceKeys.comments(anchor, issueId), (previous) =>
        previous === undefined
          ? previous
          : previous.map((row) =>
              row.id === comment.id
                ? { ...row, comment_html: comment.comment_html, updated_at: comment.updated_at }
                : row
            )
      );
    },
    onError: (_error: Error, _variables: unknown, snapshots: QuerySnapshot[] | undefined) => {
      if (snapshots !== undefined) rollbackSnapshots(queryClient, snapshots);
      notifySpaceCommentEditFailure();
    },
    onSettled: () => invalidateSpaceComments(queryClient, anchor, issueId),
  });
}

/**
 * Delete the visitor's own comment (SPC-055): no confirmation, one
 * notice. The row leaves immediately and a refusal restores it with an
 * error notice.
 */
export function useDeleteSpaceComment(client: ApiClient, anchor: string, issueId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, string, QuerySnapshot[]>({
    mutationFn: (commentId: string) => deleteSpaceComment(client, anchor, issueId, commentId),
    onMutate: async (commentId: string) => {
      const key = spaceKeys.comments(anchor, issueId);
      await queryClient.cancelQueries({ queryKey: key });
      const snapshots = snapshotQueries(queryClient, key);
      queryClient.setQueryData<SpaceComment[]>(key, (previous) =>
        previous === undefined ? previous : previous.filter((row) => row.id !== commentId)
      );
      return snapshots;
    },
    onSuccess: () => notifySpaceCommentDeleteSuccess(),
    onError: (_error: Error, _commentId: unknown, snapshots: QuerySnapshot[] | undefined) => {
      if (snapshots !== undefined) rollbackSnapshots(queryClient, snapshots);
      notifySpaceCommentDeleteFailure();
    },
    onSettled: () => invalidateSpaceComments(queryClient, anchor, issueId),
  });
}

/**
 * Toggle one comment's reactions (SPC-057). One write at a time: the
 * pending toggle overlays the server rows until it settles, then the
 * thread re-reads. A refusal clears the overlay, re-reads and reports.
 * Anonymous activations leave for sign-in instead of writing.
 */
export function useSpaceCommentReactions(
  client: ApiClient,
  anchor: string,
  issueId: string,
  comment: Pick<SpaceComment, "id" | "comment_reactions">,
  onRequireSignIn: () => void
): { groups: SpaceReactionGroup[]; react: (code: string) => Promise<void> } {
  const viewer = useQuery(spaceViewerQueryOptions(client));
  const queryClient = useQueryClient();
  const [pending, setPending] = React.useState<SpaceReactionWrite | null>(null);
  const actor =
    viewer.data === null || viewer.data === undefined ? null : { id: viewer.data.id, name: viewer.data.display_name };
  const rows = overlaySpaceCommentReaction(comment.comment_reactions, actor, pending);
  const groups = groupSpaceCommentReactions(rows, actor === null ? null : actor.id);

  const react = React.useCallback(
    async (code: string): Promise<void> => {
      if (pending !== null) return;
      const signedIn = viewer.data;
      if (signedIn === null || signedIn === undefined) {
        onRequireSignIn();
        return;
      }
      const write = nextSpaceReaction(viewerSpaceCommentReacted(rows, signedIn.id, code), code);
      setPending(write);
      try {
        if (write.action === "remove") {
          await removeSpaceCommentReaction(client, anchor, comment.id, code);
        } else {
          await addSpaceCommentReaction(client, anchor, comment.id, code);
        }
        await invalidateSpaceComments(queryClient, anchor, issueId);
      } catch {
        await invalidateSpaceComments(queryClient, anchor, issueId);
        notifySpaceReactionFailure();
      } finally {
        setPending(null);
      }
    },
    [anchor, client, comment.id, issueId, onRequireSignIn, pending, queryClient, rows, viewer.data]
  );

  return { groups, react };
}
