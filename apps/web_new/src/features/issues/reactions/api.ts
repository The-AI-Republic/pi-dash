// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reaction data (parity/specs/issues.md §16, ISS-203/204). One write path
// for both surfaces: clicking a chip the viewer already picked deletes
// their reaction, anything else posts one. Each surface re-reads its list
// on settle, so counts and reactor names always come from the server.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createCommentReaction,
  createIssueReaction,
  deleteCommentReaction,
  deleteIssueReaction,
  isApiError,
  listCommentReactions,
  listIssueReactions,
  type ApiClient,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { commentReactionRows, issueReactionRows } from "./model.js";

/** Notices the oracle scenarios assert after each toggle. */
export const REACTION_CREATED_TITLE = "Reaction created successfully";
export const REACTION_REMOVED_TITLE = "Reaction removed successfully";
export const REACTION_CREATE_FAILED_TITLE = "Reaction creation failed";
export const REACTION_REMOVE_FAILED_TITLE = "Reaction remove failed";
export const REACTION_FAILURE_FALLBACK = "Check your connection and try again.";

export function reactionFailureDetail(error: unknown): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return REACTION_FAILURE_FALLBACK;
}

/** One toggle: which reaction key and whether it is being added. */
export interface ToggleInput {
  code: string;
  action: "add" | "remove";
}

export interface ReactionScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

export function issueReactionsQueryOptions(client: ApiClient, scope: ReactionScope, enabled = true) {
  return queryOptions({
    queryKey: issueKeys.reactions(scope.workspaceSlug, scope.projectId, scope.issueId),
    enabled,
    queryFn: async () =>
      issueReactionRows(await listIssueReactions(client, scope.workspaceSlug, scope.projectId, scope.issueId)),
    staleTime: 15_000,
    refetchOnWindowFocus: false,
  });
}

export function useIssueReactions(scope: ReactionScope, enabled = true) {
  return useQuery(issueReactionsQueryOptions(getAppClient(), scope, enabled));
}

export function commentReactionsQueryOptions(client: ApiClient, scope: ReactionScope, commentId: string) {
  return queryOptions({
    queryKey: issueKeys.commentReactions(scope.workspaceSlug, scope.projectId, commentId),
    queryFn: async () =>
      commentReactionRows(await listCommentReactions(client, scope.workspaceSlug, scope.projectId, commentId)),
    staleTime: 15_000,
    refetchOnWindowFocus: false,
  });
}

export function useCommentReactions(scope: ReactionScope, commentId: string) {
  return useQuery(commentReactionsQueryOptions(getAppClient(), scope, commentId));
}

/** Post or withdraw the viewer's reaction on the issue itself (ISS-204). */
export function useToggleIssueReaction(scope: ReactionScope) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ code, action }: ToggleInput) => {
      if (action === "remove") {
        await deleteIssueReaction(client, scope.workspaceSlug, scope.projectId, scope.issueId, code);
      } else {
        await createIssueReaction(client, scope.workspaceSlug, scope.projectId, scope.issueId, code);
      }
    },
    onSuccess: (_value, { action }) => {
      notifySuccess(action === "remove" ? REACTION_REMOVED_TITLE : REACTION_CREATED_TITLE);
    },
    onError: (error: Error, { action }) => {
      notifyError(
        action === "remove" ? REACTION_REMOVE_FAILED_TITLE : REACTION_CREATE_FAILED_TITLE,
        reactionFailureDetail(error)
      );
    },
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: issueKeys.reactions(scope.workspaceSlug, scope.projectId, scope.issueId),
      }),
  });
}

/** Post or withdraw the viewer's reaction on one comment (ISS-203). */
export function useToggleCommentReaction(scope: ReactionScope, commentId: string) {
  const client = getAppClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ code, action }: ToggleInput) => {
      if (action === "remove") {
        await deleteCommentReaction(client, scope.workspaceSlug, scope.projectId, commentId, code);
      } else {
        await createCommentReaction(client, scope.workspaceSlug, scope.projectId, commentId, code);
      }
    },
    onSuccess: (_value, { action }) => {
      notifySuccess(action === "remove" ? REACTION_REMOVED_TITLE : REACTION_CREATED_TITLE);
    },
    onError: (error: Error, { action }) => {
      notifyError(
        action === "remove" ? REACTION_REMOVE_FAILED_TITLE : REACTION_CREATE_FAILED_TITLE,
        reactionFailureDetail(error)
      );
    },
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: issueKeys.commentReactions(scope.workspaceSlug, scope.projectId, commentId),
      }),
  });
}
