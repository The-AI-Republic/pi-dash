// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project mutations (SHELL-032, SHELL-037 through SHELL-042). The
// favorite star toggles through here: the card flips optimistically,
// rolls back on failure, and the first successful add opens the
// sidebar favorites section. Archive, restore, delete and create
// land here too: each confirms through its dialog, toasts, and
// refreshes the workspace subtree so every list re-reads.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  addFavoriteProject,
  archiveProject,
  createProject,
  deleteProject,
  removeFavoriteProject,
  unarchiveProject,
  type ProjectCreateInput,
  type ProjectDetails,
  type ProjectDetailsList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate, invalidateWorkspace, type Rollback } from "../../../core/query/optimistic.js";
import {
  isFavoritesSectionOpen,
  notifyError,
  notifySuccess,
  setFavoritesSectionOpen,
} from "../../../shared/shell/index.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { projectKeys } from "./keys.js";

export const FAVORITE_ADDED_TITLE = "Added to favorites";
export const FAVORITE_REMOVED_TITLE = "Removed from favorites";
export const FAVORITE_FAILED_TITLE = "Couldn't update favorites";

export const ARCHIVED_TITLE = "Project archived";
export const ARCHIVE_FAILED_TITLE = "Couldn't archive the project";
export const RESTORED_TITLE = "Project restored";
export const RESTORE_FAILED_TITLE = "Couldn't restore the project";
export const DELETED_TITLE = "Project deleted";
export const DELETE_FAILED_TITLE = "Couldn't delete the project";
export const CREATED_TITLE = "Project created";

export interface ToggleFavoriteInput {
  workspaceSlug: string;
  project: ProjectDetails;
}

function flipStar(rows: ProjectDetailsList | undefined, projectId: string): ProjectDetailsList | undefined {
  return rows?.map((row) => (row.id === projectId ? { ...row, is_favorite: !row.is_favorite } : row));
}

/** The card mid-toggle, or null when no toggle is flying. Pure for tests. */
export function selectPendingProjectId(isPending: boolean, variables: ToggleFavoriteInput | undefined): string | null {
  if (!isPending) return null;
  return variables?.project.id ?? null;
}

/**
 * Star/unstar a project (SHELL-032): the card flips at once, the write
 * lands through the favorite endpoints, and the first add opens the
 * favorites section. A failed write toasts and rolls the card back.
 */
export function useToggleFavoriteProject() {
  const queryClient = useQueryClient();
  const mutation = useMutation<unknown, Error, ToggleFavoriteInput, { rollback: Rollback }>({
    mutationFn: async (input) => {
      const client = getAppClient();
      try {
        if (input.project.is_favorite) {
          const status = await removeFavoriteProject(client, input.workspaceSlug, input.project.id);
          notifySuccess(FAVORITE_REMOVED_TITLE, `${input.project.name} is no longer a favorite.`);
          return status;
        }
        const status = await addFavoriteProject(client, input.workspaceSlug, input.project.id);
        notifySuccess(FAVORITE_ADDED_TITLE, `${input.project.name} is now a favorite.`);
        if (!isFavoritesSectionOpen()) setFavoritesSectionOpen(true);
        return status;
      } catch (error) {
        notifyError(FAVORITE_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onMutate: async (input) => {
      const key = projectKeys.details(input.workspaceSlug);
      await queryClient.cancelQueries({ queryKey: key });
      return {
        rollback: applyOptimisticUpdate(queryClient, key, (rows: ProjectDetailsList | undefined) =>
          flipStar(rows, input.project.id)
        ),
      };
    },
    onError: (_error, _input, context) => {
      context?.rollback();
    },
    onSettled: (_data, _error, input) => {
      void queryClient.invalidateQueries({ queryKey: projectKeys.details(input.workspaceSlug) });
      // The sidebar section reads its own list: starring re-reads it
      // so the first add surfaces (and opens the section) without a
      // reload (SHELL-032 seam; the section is NEWFRONT-286's).
      void queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(input.workspaceSlug) });
    },
  });
  return {
    ...mutation,
    /** The card mid-toggle, so the view pins the pending state on it. */
    pendingProjectId: selectPendingProjectId(mutation.isPending, mutation.variables),
  };
}

export interface ProjectLifecycleInput {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
}

export interface CreateProjectInput {
  workspaceSlug: string;
  payload: ProjectCreateInput;
}

/**
 * Archive a project (SHELL-037): the write stamps `archived_at`,
 * the notice confirms, and the workspace subtree re-reads so the
 * project leaves the default list for the archive view. A failed
 * write toasts and the dialog stays open.
 */
export function useArchiveProject() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, ProjectLifecycleInput>({
    mutationFn: async (input) => {
      try {
        const result = await archiveProject(getAppClient(), input.workspaceSlug, input.projectId);
        notifySuccess(ARCHIVED_TITLE, `${input.projectName} has been archived successfully`);
        return result;
      } catch (error) {
        notifyError(ARCHIVE_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onSettled: (_data, _error, input) => {
      void invalidateWorkspace(queryClient, input.workspaceSlug);
    },
  });
}

/**
 * Restore an archived project (SHELL-037): the write clears
 * `archived_at`, the notice confirms, and the re-read returns the
 * project to the default list. Failures toast, dialog stays open.
 */
export function useRestoreProject() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, ProjectLifecycleInput>({
    mutationFn: async (input) => {
      try {
        const status = await unarchiveProject(getAppClient(), input.workspaceSlug, input.projectId);
        notifySuccess(RESTORED_TITLE, `You can find ${input.projectName} in your projects.`);
        return status;
      } catch (error) {
        notifyError(RESTORE_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onSettled: (_data, _error, input) => {
      void invalidateWorkspace(queryClient, input.workspaceSlug);
    },
  });
}

/**
 * Delete an archived project (SHELL-039). The dialog double-gates
 * the write; success toasts and the re-read drops the card.
 * Failures toast, dialog stays open.
 */
export function useDeleteProject() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, ProjectLifecycleInput>({
    mutationFn: async (input) => {
      try {
        const status = await deleteProject(getAppClient(), input.workspaceSlug, input.projectId);
        notifySuccess(DELETED_TITLE, "Project deleted successfully.");
        return status;
      } catch (error) {
        notifyError(DELETE_FAILED_TITLE, (error as Error).message);
        throw error;
      }
    },
    onSettled: (_data, _error, input) => {
      void invalidateWorkspace(queryClient, input.workspaceSlug);
    },
  });
}

/**
 * Create a project (SHELL-040 through SHELL-042): one POST, no
 * cover step. Success toasts and the re-read seats the new card.
 * Failures never toast here — the dialog maps duplicates onto
 * their fields and toasts the rest itself.
 */
export function useCreateProject() {
  const queryClient = useQueryClient();
  return useMutation<unknown, Error, CreateProjectInput>({
    mutationFn: async (input) => {
      const created = await createProject(getAppClient(), input.workspaceSlug, input.payload);
      notifySuccess(CREATED_TITLE, "Project created successfully");
      return created;
    },
    onSettled: (_data, _error, input) => {
      void invalidateWorkspace(queryClient, input.workspaceSlug);
    },
  });
}
