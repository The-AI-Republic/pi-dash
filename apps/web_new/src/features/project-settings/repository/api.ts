// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository sync data access (parity/specs/project-settings-part-f.md
// §Repository tab, PSET-050–052). The tab reads the binding, flips the
// sync flag and unbinds; it never binds — binding belongs to the General
// tab's repository row (PSET-008).

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  getProjectRepository,
  setProjectRepositorySync,
  unbindProjectRepository,
  type ApiClient,
  type ProjectRepositoryStatus,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../../core/query/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";

export const repositoryKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "project-repository"] as const,
  project: (workspaceSlug: string, projectId: string) => [...repositoryKeys.all(workspaceSlug), projectId] as const,
};

export function repositoryQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions<ProjectRepositoryStatus>({
    queryKey: repositoryKeys.project(workspaceSlug, projectId),
    queryFn: () => getProjectRepository(client, workspaceSlug, projectId),
    staleTime: LIST_STALE_TIME_MS,
    refetchOnWindowFocus: false,
  });
}

/** The project's binding state; `isPending` drives the skeleton (PSET-050). */
export function useRepositoryStatus(workspaceSlug: string, projectId: string) {
  return useQuery(repositoryQueryOptions(getAppClient(), workspaceSlug, projectId));
}

export const SYNC_UPDATED_TITLE = "Sync setting saved";
export const SYNC_UPDATED_BODY = "The repository sync setting now matches the switch.";
export const SYNC_FAILED_TITLE = "Couldn’t change sync";
export const SYNC_FAILED_BODY = "The repository sync setting is unchanged. Try again in a moment.";
export const UNBOUND_TITLE = "Repository unlinked";
/** The notice the oracle pins after a confirm-free unbind (PSET-051). */
export const UNBOUND_BODY = "Mirrored issues remain editable.";

/** Flip the sync flag; the binding itself is left intact (PSET-051). */
export function useToggleRepositorySync(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) => setProjectRepositorySync(getAppClient(), workspaceSlug, projectId, enabled),
    onSuccess: (binding) => {
      queryClient.setQueryData(repositoryKeys.project(workspaceSlug, projectId), binding);
      notifySuccess(SYNC_UPDATED_TITLE, SYNC_UPDATED_BODY);
    },
    onError: () => {
      notifyError(SYNC_FAILED_TITLE, SYNC_FAILED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: repositoryKeys.project(workspaceSlug, projectId) });
    },
  });
}

/**
 * Drop the binding with no confirmation (PSET-051). The server keeps the
 * mirrored issues and their synced comments intact; the notice says so.
 */
export function useUnbindRepository(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => unbindProjectRepository(getAppClient(), workspaceSlug, projectId),
    onSuccess: (status) => {
      queryClient.setQueryData(repositoryKeys.project(workspaceSlug, projectId), status);
      notifySuccess(UNBOUND_TITLE, UNBOUND_BODY);
    },
    onError: () => {
      notifyError(SYNC_FAILED_TITLE, SYNC_FAILED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: repositoryKeys.project(workspaceSlug, projectId) });
    },
  });
}
