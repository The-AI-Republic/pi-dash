// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attachment reads and writes (parity/specs/issues.md §15, ISS-187–190).
// The list is the uploaded-only collection; uploads run the contract's
// three signed legs, and deletes are soft server-side. Every write
// invalidates the list and the issue detail so the activity feed picks
// the change up.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  deleteIssueAttachment,
  listIssueAttachments,
  uploadIssueAttachment,
  type ApiClient,
  type AttachmentByteUpload,
  type IssueAttachment,
} from "@pidash/api-client";

import { getAppClient } from "../../../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../../../core/query/client.js";
import { notifyError, notifySuccess } from "../../../../../shared/shell/index.js";
import { issueKeys } from "../../../api/keys.js";
import {
  ATTACHMENT_DELETE_FAILED_TITLE,
  ATTACHMENT_REMOVED_TITLE,
  ATTACHMENT_UPLOAD_FAILED_TITLE,
  ATTACHMENT_UPLOADED_TITLE,
} from "./format.js";

export const attachmentKeys = {
  all: (workspaceSlug: string, issueId: string) =>
    [...issueKeys.detail(workspaceSlug, issueId), "attachments"] as const,
  list: (workspaceSlug: string, projectId: string, issueId: string) =>
    [...attachmentKeys.all(workspaceSlug, issueId), projectId] as const,
};

export function attachmentsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, issueId: string) {
  return queryOptions({
    queryKey: attachmentKeys.list(workspaceSlug, projectId, issueId),
    queryFn: () => listIssueAttachments(client, workspaceSlug, projectId, issueId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** The issue's uploaded attachments. */
export function useIssueAttachments(workspaceSlug: string, projectId: string, issueId: string) {
  return useQuery(attachmentsQueryOptions(getAppClient(), workspaceSlug, projectId, issueId));
}

export interface UploadAttachmentInput {
  file: File;
  /** Injectable byte leg; the app uses the contract default. */
  uploadBytes?: AttachmentByteUpload;
}

/** Upload one file end to end, then refresh the list and the item. */
export function useUploadAttachment(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  return useMutation<IssueAttachment, Error, UploadAttachmentInput>({
    mutationFn: (input) =>
      uploadIssueAttachment(
        getAppClient(),
        workspaceSlug,
        projectId,
        issueId,
        input.file,
        ...(input.uploadBytes === undefined ? [] : [input.uploadBytes])
      ),
    onSuccess: () => {
      notifySuccess(ATTACHMENT_UPLOADED_TITLE);
    },
    onError: () => {
      notifyError(ATTACHMENT_UPLOAD_FAILED_TITLE);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: attachmentKeys.all(workspaceSlug, issueId) });
      void queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) });
    },
  });
}

/** Remove one attachment, then refresh the list and the item. */
export function useDeleteAttachment(workspaceSlug: string, projectId: string, issueId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (assetId) => deleteIssueAttachment(getAppClient(), workspaceSlug, projectId, issueId, assetId),
    onSuccess: () => {
      notifySuccess(ATTACHMENT_REMOVED_TITLE);
    },
    onError: () => {
      notifyError(ATTACHMENT_DELETE_FAILED_TITLE);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: attachmentKeys.all(workspaceSlug, issueId) });
      void queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) });
    },
  });
}
