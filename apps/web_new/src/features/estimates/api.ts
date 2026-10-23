// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates settings data access (parity/specs/project-settings-part-f.md
// §Estimates entry, PSET-049). One list read, one create with its points,
// one delete, and the project write that links or clears the active
// system. Writes refresh the list and the project row so the switch and
// the rows agree with the server.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createEstimate,
  deleteEstimate,
  listProjectEstimates,
  updateProject,
  type ApiClient,
  type Estimate,
  type EstimateList,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { workspaceScope } from "../../core/query/index.js";
import { authKeys } from "../auth/index.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";

export const estimatesKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "estimates"] as const,
  project: (workspaceSlug: string, projectId: string) => [...estimatesKeys.all(workspaceSlug), projectId] as const,
};

export function projectEstimatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions<EstimateList>({
    queryKey: estimatesKeys.project(workspaceSlug, projectId),
    queryFn: () => listProjectEstimates(client, workspaceSlug, projectId),
    // EST-027: the old client revalidates this read on mount, stale data
    // and window focus, with no staleness window — a revisit fetches
    // rather than replaying a cache. The shared client opts out of focus
    // refetching (ISS-221), so the read opts back in explicitly. There is
    // still no live channel: an open, focused screen stays stale until a
    // fetch actually runs.
    staleTime: 0,
    refetchOnWindowFocus: true,
  });
}

/** The project's estimate systems; `isPending` drives the loader (EST-002). */
export function useProjectEstimates(workspaceSlug: string, projectId: string) {
  return useQuery(projectEstimatesQueryOptions(getAppClient(), workspaceSlug, projectId));
}

export const ESTIMATE_CREATED_TITLE = "Estimate created";
export const ESTIMATE_CREATED_BODY = "The estimate system is now the project’s active one.";
export const ESTIMATE_CREATE_FAILED_TITLE = "Estimate creation failed — nothing was saved";
export const ESTIMATE_CREATE_FAILED_BODY = "Nothing was stored. Adjust the name or values and try again.";
export const ESTIMATE_DELETED_TITLE = "Estimate deleted";
export const ESTIMATE_DELETED_BODY = "The system and its values were removed.";
export const ESTIMATE_DELETE_FAILED_TITLE = "Couldn’t delete the estimate";
// An active system's delete is two writes: a failure can land after the
// system is already gone, so the notice never promises the system is
// still there.
export const ESTIMATE_DELETE_FAILED_BODY =
  "The delete did not finish. Reload the page to see whether the system is still listed.";
export const ESTIMATE_ENABLED_TITLE = "Estimates enabled";
export const ESTIMATE_ENABLED_BODY = "The project now offers its estimate values on work items.";
export const ESTIMATE_DISABLED_TITLE = "Estimates disabled";
export const ESTIMATE_DISABLED_BODY = "Work items no longer offer estimate values.";
export const ESTIMATE_TOGGLE_FAILED_TITLE = "Couldn’t change estimates";
export const ESTIMATE_TOGGLE_FAILED_BODY = "The project’s estimate setting is unchanged.";

export interface CreateEstimateValues {
  name: string;
  type: string;
  values: readonly string[];
}

/** Create the system as the project's active one, with its points. */
export function useCreateEstimate(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<Estimate, Error, CreateEstimateValues>({
    mutationFn: (input) =>
      createEstimate(getAppClient(), workspaceSlug, projectId, {
        name: input.name,
        type: input.type,
        last_used: true,
        points: input.values.map((value, index) => ({ key: index + 1, value })),
      }),
    onSuccess: () => {
      notifySuccess(ESTIMATE_CREATED_TITLE, ESTIMATE_CREATED_BODY);
    },
    onError: () => {
      notifyError(ESTIMATE_CREATE_FAILED_TITLE, ESTIMATE_CREATE_FAILED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: estimatesKeys.project(workspaceSlug, projectId) });
    },
  });
}

/**
 * Delete one system. Deleting the active system also clears the project's
 * reference, so the switch cannot point at a system that no longer exists
 * (EST-016); a superseded system needs only the delete.
 */
export function useDeleteEstimate(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, { estimate: Estimate; active: boolean }>({
    mutationFn: async ({ estimate, active }) => {
      await deleteEstimate(getAppClient(), workspaceSlug, projectId, estimate.id);
      if (active) {
        await updateProject(getAppClient(), workspaceSlug, projectId, { estimate: null });
        queryClient.setQueryData<{ estimate?: string | null } | undefined>(
          authKeys.project(workspaceSlug, projectId),
          (current) => (current === undefined ? current : { ...current, estimate: null })
        );
      }
    },
    onSuccess: () => {
      notifySuccess(ESTIMATE_DELETED_TITLE, ESTIMATE_DELETED_BODY);
    },
    onError: () => {
      notifyError(ESTIMATE_DELETE_FAILED_TITLE, ESTIMATE_DELETE_FAILED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: estimatesKeys.project(workspaceSlug, projectId) });
    },
  });
}

/**
 * The enable switch (PSET-049): enabling links the active system's id,
 * disabling writes null. The switch reads the project's stored reference,
 * never list position, so an archived system never becomes active.
 */
export function useToggleEstimates(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<{ estimate: string | null }, Error, { enabled: boolean; activeId: string }>({
    mutationFn: async ({ enabled, activeId }) => {
      await updateProject(getAppClient(), workspaceSlug, projectId, {
        estimate: enabled ? activeId : null,
      });
      return { estimate: enabled ? activeId : null };
    },
    onSuccess: (result) => {
      queryClient.setQueryData<{ estimate?: string | null } | undefined>(
        authKeys.project(workspaceSlug, projectId),
        (current) => (current === undefined ? current : { ...current, estimate: result.estimate })
      );
      if (result.estimate === null) notifySuccess(ESTIMATE_DISABLED_TITLE, ESTIMATE_DISABLED_BODY);
      else notifySuccess(ESTIMATE_ENABLED_TITLE, ESTIMATE_ENABLED_BODY);
    },
    onError: () => {
      notifyError(ESTIMATE_TOGGLE_FAILED_TITLE, ESTIMATE_TOGGLE_FAILED_BODY);
    },
  });
}
