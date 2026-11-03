// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Writes behind the general page (parity/specs/workspace-settings.md
// §3–§6; WSET-003–006). Each mutation answers the row the server stored
// and re-seeds the shared details key so the form, the sidebar identity
// and the workspace list agree without a reload. Toast copy lives with
// the mutations because it is the same for every caller.

import { useMutation, useQueryClient } from "@tanstack/react-query";

import {
  deleteLegacyWorkspaceFileAsset,
  deleteWorkspace,
  deleteWorkspaceLogoAsset,
  updateWorkspace,
  uploadWorkspaceLogo,
  type ByteUpload,
  type WorkspaceDetails,
  type WorkspaceUpdateInput,
} from "@pidash/api-client";

import { getAppClient } from "../../../../core/api/client.js";
import { sessionKeys } from "../../../../core/session/index.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { assetKeyFromUrl } from "../rules.js";
import { workspaceDetailsKey } from "./details.js";

/** Save notification (§3, parity-pinned). */
export const DETAILS_SAVED_TITLE = "Success!";
export const DETAILS_SAVED_MESSAGE = "Workspace updated successfully";

/** Logo removal notification (§4, parity-pinned). */
export const LOGO_REMOVED_TITLE = "Success!";
export const LOGO_REMOVED_MESSAGE = "Workspace picture removed successfully.";

/** Logo upload failure (§4). */
export const LOGO_UPLOAD_FAILED_TITLE = "Error";
export const LOGO_UPLOAD_FAILED_MESSAGE = "The logo could not be uploaded. Try again.";

/** Logo removal failure (§4). */
export const LOGO_REMOVE_FAILED_TITLE = "Error!";
export const LOGO_REMOVE_FAILED_MESSAGE = "The workspace picture could not be removed. Please try again.";

/** Copy confirmation (§5, parity-pinned). */
export const URL_COPIED_TITLE = "Workspace URL copied to the clipboard.";

/** Delete notifications (§6). */
export const WORKSPACE_DELETED_TITLE = "Workspace deleted.";
export const WORKSPACE_DELETE_FAILED_TITLE = "That didn't work.";
export const WORKSPACE_DELETE_FAILED_MESSAGE = "The workspace could not be deleted. Please try again.";

/** The general form's write body: exactly the three editable fields (§3). */
export function detailsUpdateInput(form: {
  name: string;
  organizationSize: string;
  timezone: string;
}): WorkspaceUpdateInput {
  return {
    name: form.name,
    organization_size: form.organizationSize,
    timezone: form.timezone,
  };
}

/** Write the details form, then re-seed the shared row and the workspace list. */
export function useSaveWorkspaceDetails(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<WorkspaceDetails, Error, WorkspaceUpdateInput>({
    mutationFn: (input) => updateWorkspace(getAppClient(), workspaceSlug, input),
    onSuccess: (saved) => {
      queryClient.setQueryData(workspaceDetailsKey(workspaceSlug), saved);
      void queryClient.invalidateQueries({ queryKey: sessionKeys.workspaces });
      notifySuccess(DETAILS_SAVED_TITLE, DETAILS_SAVED_MESSAGE);
    },
  });
}

/** Mark the shared row with a new logo value (or clear it) without a re-read. */
export function applyLogoToDetails(
  details: WorkspaceDetails | undefined,
  logoUrl: string | null
): WorkspaceDetails | undefined {
  if (details === undefined) return undefined;
  return { ...details, logo: logoUrl, logo_asset: logoUrl === null ? null : details.logo_asset, logo_url: logoUrl };
}

export interface LogoUploadInput {
  workspaceId: string;
  file: File;
  /** Bytes leg; tests inject a fake. */
  uploadBytes?: ByteUpload | undefined;
}

/** Grant → bytes → confirm, then store the returned URL on the workspace row. */
export function useUploadWorkspaceLogo(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const key = workspaceDetailsKey(workspaceSlug);
  return useMutation<WorkspaceDetails, Error, LogoUploadInput>({
    mutationFn: async ({ workspaceId, file, uploadBytes }) => {
      const client = getAppClient();
      const assetUrl = await uploadWorkspaceLogo(client, workspaceSlug, workspaceId, file, uploadBytes);
      return updateWorkspace(client, workspaceSlug, { logo: assetUrl });
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(key, saved);
      void queryClient.invalidateQueries({ queryKey: sessionKeys.workspaces });
    },
    onError: () => {
      notifyError(LOGO_UPLOAD_FAILED_TITLE, LOGO_UPLOAD_FAILED_MESSAGE);
    },
  });
}

export interface LogoRemoveInput {
  workspaceId: string;
  /** The stored logo value the row currently carries. */
  logoUrl: string;
  legacy: boolean;
}

/**
 * Drop the stored asset (legacy file-asset path for full URLs, the v2
 * asset path otherwise), then clear the logo field. The cached row loses
 * its logo on success, so the avatar falls back to the initial at once
 * (NEWFRONT-416 target).
 */
export function useRemoveWorkspaceLogo(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const key = workspaceDetailsKey(workspaceSlug);
  return useMutation<WorkspaceDetails, Error, LogoRemoveInput>({
    mutationFn: async ({ workspaceId, logoUrl, legacy }) => {
      const client = getAppClient();
      const assetKey = assetKeyFromUrl(logoUrl);
      if (legacy) {
        await deleteLegacyWorkspaceFileAsset(client, workspaceId, assetKey);
      } else {
        await deleteWorkspaceLogoAsset(client, workspaceSlug, assetKey);
      }
      return updateWorkspace(client, workspaceSlug, { logo: "" });
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(key, { ...saved, logo: null, logo_asset: null, logo_url: null });
      void queryClient.invalidateQueries({ queryKey: sessionKeys.workspaces });
      notifySuccess(LOGO_REMOVED_TITLE, LOGO_REMOVED_MESSAGE);
    },
    onError: () => {
      notifyError(LOGO_REMOVE_FAILED_TITLE, LOGO_REMOVE_FAILED_MESSAGE);
    },
  });
}

/**
 * Delete the workspace, then drop its cached subtree and refresh the
 * user settings and workspace list the landing target reads (§6). A
 * failed delete leaves every cache untouched.
 */
export function useDeleteWorkspace(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, void>({
    mutationFn: async () => {
      await deleteWorkspace(getAppClient(), workspaceSlug);
    },
    onSuccess: () => {
      queryClient.removeQueries({ queryKey: ["ws", workspaceSlug] });
      void queryClient.invalidateQueries({ queryKey: sessionKeys.settings });
      void queryClient.invalidateQueries({ queryKey: sessionKeys.workspaces });
    },
    onError: () => {
      notifyError(WORKSPACE_DELETE_FAILED_TITLE, WORKSPACE_DELETE_FAILED_MESSAGE);
    },
  });
}
