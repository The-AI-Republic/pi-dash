// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Code-review reads and writes (parity/specs/issues.md §16, ISS-205).
// The collection is per issue, newest first; attaching takes only the
// pasted PR/MR URL and detaching removes the link by its id. Both writes
// invalidate the collection and the item so activity notices follow.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  attachCodeReview,
  detachCodeReview,
  listCodeReviews,
  type ApiClient,
  type CodeReviewLink,
} from "@pidash/api-client";

import { getAppClient } from "../../../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../../../core/query/client.js";
import { notifyError, notifySuccess } from "../../../../../shared/shell/index.js";
import { issueKeys } from "../../../api/keys.js";
import {
  CODE_REVIEW_ATTACHED_TITLE,
  CODE_REVIEW_ATTACH_FAILED_TITLE,
  CODE_REVIEW_DETACHED_TITLE,
  CODE_REVIEW_DETACH_FAILED_TITLE,
} from "./format.js";

export const codeReviewKeys = {
  all: (workspaceSlug: string, issueId: string) =>
    [...issueKeys.detail(workspaceSlug, issueId), "code-reviews"] as const,
  list: (workspaceSlug: string, projectId: string, issueId: string) =>
    [...codeReviewKeys.all(workspaceSlug, issueId), projectId] as const,
};

export function codeReviewsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, issueId: string) {
  return queryOptions({
    queryKey: codeReviewKeys.list(workspaceSlug, projectId, issueId),
    queryFn: () => listCodeReviews(client, workspaceSlug, projectId, issueId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** The issue's linked reviews, newest first. */
export function useCodeReviews(workspaceSlug: string, projectId: string, issueId: string) {
  return useQuery(codeReviewsQueryOptions(getAppClient(), workspaceSlug, projectId, issueId));
}

/** Attach one pasted review URL. */
export function useAttachCodeReview(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  return useMutation<CodeReviewLink, Error, string>({
    mutationFn: (url) => attachCodeReview(getAppClient(), workspaceSlug, projectId, issueId, url),
    onSuccess: () => {
      notifySuccess(CODE_REVIEW_ATTACHED_TITLE);
    },
    onError: () => {
      notifyError(CODE_REVIEW_ATTACH_FAILED_TITLE);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: codeReviewKeys.all(workspaceSlug, issueId) });
      void queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) });
    },
  });
}

/** Detach one linked review by id. */
export function useDetachCodeReview(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (linkId) => detachCodeReview(getAppClient(), workspaceSlug, projectId, issueId, linkId),
    onSuccess: () => {
      notifySuccess(CODE_REVIEW_DETACHED_TITLE);
    },
    onError: () => {
      notifyError(CODE_REVIEW_DETACH_FAILED_TITLE);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: codeReviewKeys.all(workspaceSlug, issueId) });
      void queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) });
    },
  });
}
