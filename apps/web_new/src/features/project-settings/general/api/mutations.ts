// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Writes behind the General tab (parity/specs/project-settings-general.md
// §PSET-006–009). Each answer updates the cached settings row and the
// shell's project read, so the page header, the rail and the collection
// follow a rename or a new emblem without a reload. Notices live here
// because every caller of these writes shows the same copy.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  bindProjectRepository,
  updateProjectSettings,
  uploadUserAsset,
  type ByteUpload,
  type ProjectRepositoryBound,
  type ProjectRepositoryStatus,
  type ProjectSettingsDetail,
  type ProjectUpdateInput,
} from "@pidash/api-client";

import { getAppClient } from "../../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { generalKeys, seedProjectRow } from "./queries.js";

/** Save confirmation (parity-pinned by the General oracle). */
export const PROJECT_SAVED_TITLE = "Success!";
export const PROJECT_SAVED_MESSAGE = "Project updated successfully";
export const PROJECT_SAVE_FAILED_TITLE = "Error!";

/** Repository binding notices (PSET-008). */
export const REPO_BIND_FAILED_TITLE = "Bind failed";
export const REPO_BOUND_TITLE = "Repository bound";
export const REPO_BOUND_MESSAGE = "Enable sync in the repository settings to start importing work.";

/** Emblem upload failure (PSET-007): the whole save aborts. */
export const EMBLEM_UPLOAD_FAILED_TITLE = "Error!";
export const EMBLEM_UPLOAD_FAILED_MESSAGE = "The emblem could not be uploaded. Try again.";

/** Project update that re-seeds the row and the shell's project read. */
export function useSaveProjectGeneral(slug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectSettingsDetail, Error, ProjectUpdateInput>({
    mutationFn: (input) => updateProjectSettings(getAppClient(), slug, projectId, input),
    onSuccess: (saved) => {
      seedProjectRow(queryClient, slug, projectId, saved);
      notifySuccess(PROJECT_SAVED_TITLE, PROJECT_SAVED_MESSAGE);
    },
  });
}

export interface BindRepositoryInput {
  repoUrl: string;
}

/**
 * Bind the repository (PSET-008): the bind answer carries the canonical
 * URL, which is then written onto the project row and re-read as status.
 * A refusal surfaces its own message; a provider-less stack answers 401,
 * which the global session middleware handles exactly as the oracle pins
 * it (NEWFRONT-395).
 */
export function useBindRepository(slug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectRepositoryBound, Error, BindRepositoryInput>({
    mutationFn: ({ repoUrl }) => bindProjectRepository(getAppClient(), slug, projectId, { repo_url: repoUrl }),
    onSuccess: async (bound) => {
      const client = getAppClient();
      const detail = queryClient.getQueryData<ProjectSettingsDetail>(generalKeys.detail(slug, projectId));
      if (detail !== undefined) {
        const saved = await updateProjectSettings(client, slug, projectId, { repo_url: bound.repo_url });
        seedProjectRow(queryClient, slug, projectId, saved);
      }
      queryClient.setQueryData<ProjectRepositoryStatus>(generalKeys.repository(slug, projectId), bound);
      void queryClient.invalidateQueries({ queryKey: generalKeys.repository(slug, projectId) });
      notifySuccess(REPO_BOUND_TITLE, REPO_BOUND_MESSAGE);
    },
    onError: (error) => {
      notifyError(REPO_BIND_FAILED_TITLE, error.message);
    },
  });
}

export interface EmblemUploadInput {
  file: File;
  /** Bytes leg; tests inject a fake. */
  uploadBytes?: ByteUpload | undefined;
}

/**
 * Grant → bytes → confirm one user asset (PSET-007). The upload path is the
 * only artwork the tab accepts besides the emoji/icon mark; there is no
 * cover, gallery or provider search.
 */
export function useUploadEmblem() {
  return useMutation<string, Error, EmblemUploadInput>({
    mutationFn: ({ file, uploadBytes }) => uploadUserAsset(getAppClient(), "USER_COVER", file, uploadBytes),
    onError: () => {
      notifyError(EMBLEM_UPLOAD_FAILED_TITLE, EMBLEM_UPLOAD_FAILED_MESSAGE);
    },
  });
}
