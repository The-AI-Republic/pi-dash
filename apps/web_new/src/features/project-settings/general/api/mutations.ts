// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Writes behind the General tab (parity/specs/project-settings-general.md
// §PSET-006–009). Each answer updates the cached settings row and the
// shell's project read, so the page header, the rail and the collection
// follow a rename or a new emblem without a reload. Notices live here
// because every caller of these writes shows the same copy.

import { useMutation, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  bindProjectRepository,
  updateProjectSettings,
  uploadProjectAsset,
  type ByteUpload,
  type ProjectRepo,
  type ProjectRepositoryBound,
  type ProjectRepositoryStatus,
  type ProjectSettingsDetail,
  type ProjectUpdateInput,
} from "@pidash/api-client";

import { getAppClient, getProviderWriteClient } from "../../../../core/api/client.js";
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

/** Repository list notices (NEWPIDASH-413). */
export const REPOS_SAVED_TITLE = "Success!";
export const REPOS_SAVED_MESSAGE = "Repositories updated";
export const REPOS_SAVE_FAILED_TITLE = "Error!";

/**
 * The row a repos-list write re-seeds: the PATCH answer already carries
 * the reconciled list, so seeding matches the server and the invalidation
 * re-reads it.
 */
function seedReposRow(queryClient: QueryClient, slug: string, projectId: string, saved: ProjectSettingsDetail): void {
  seedProjectRow(queryClient, slug, projectId, saved);
  void queryClient.invalidateQueries({ queryKey: generalKeys.detail(slug, projectId) });
}

/**
 * Write the repository list (NEWPIDASH-413): a full-list replace through
 * the settings PATCH. The columns follow renames and clears on the
 * server (double delete), so removing the legacy-matching row needs no
 * special casing here. The write re-seeds the row and re-reads it, so
 * the list always matches the server after a write.
 */
export function useSaveProjectRepos(slug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectSettingsDetail, Error, { repos: ProjectRepo[] }>({
    mutationFn: ({ repos }) => updateProjectSettings(getAppClient(), slug, projectId, { settings: { repos } }),
    onSuccess: (saved) => {
      seedReposRow(queryClient, slug, projectId, saved);
      notifySuccess(REPOS_SAVED_TITLE, REPOS_SAVED_MESSAGE);
    },
    onError: (error) => {
      notifyError(REPOS_SAVE_FAILED_TITLE, error.message);
    },
  });
}

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
 * The bind POST goes through the provider-write client: a provider-less
 * stack answers 401, and that credential is not the signed-in session,
 * so the refusal must surface as a bind failure instead of expiring it
 * (NEWFRONT-395, same class as NEWFRONT-421).
 */
export function useBindRepository(slug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectRepositoryBound, Error, BindRepositoryInput>({
    mutationFn: ({ repoUrl }) =>
      bindProjectRepository(getProviderWriteClient(), slug, projectId, { repo_url: repoUrl }),
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
 * Grant → bytes → confirm one project-owned image (PSET-007). The upload
 * path is the only artwork the tab accepts besides the emoji/icon mark;
 * there is no cover, gallery or provider search. The asset is created
 * against the project, never against the signed-in account's profile.
 */
export function useUploadEmblem(slug: string, projectId: string) {
  return useMutation<string, Error, EmblemUploadInput>({
    mutationFn: ({ file, uploadBytes }) =>
      uploadProjectAsset(getAppClient(), slug, projectId, "PROJECT_COVER", file, uploadBytes),
    onError: () => {
      notifyError(EMBLEM_UPLOAD_FAILED_TITLE, EMBLEM_UPLOAD_FAILED_MESSAGE);
    },
  });
}
