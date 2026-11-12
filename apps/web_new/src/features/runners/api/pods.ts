// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod writers and the project picker read (NEWFRONT-542, RUN-013–015).
// Every write re-reads the runners subtree when it settles so the tiles,
// the table and the rail agree on the pod set; the callers keep their
// dialogs open on failure and toast the reason.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createPod, deletePod, listProjects, updatePod, type PodCreateInput, type PodPatch } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { projectKeys } from "../../projects/index.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { runnersKeys } from "./keys.js";

export interface PodUpdateInput {
  podId: string;
  patch: PodPatch;
}

/**
 * The workspace's projects for the create dialog. The caller renders the
 * form only while the dialog is open, so the read starts at that moment and
 * shares its cache entry with the rest of the projects surface.
 */
export function usePodProjects(workspaceSlug: string) {
  return useQuery({
    queryKey: projectKeys.list(workspaceSlug),
    queryFn: () => listProjects(getAppClient(), workspaceSlug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** Create one pod under a project and refresh the pod set. */
export function useCreatePod(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: PodCreateInput) => createPod(getAppClient(), input),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) }),
  });
}

/** Patch the changed subset of one pod. */
export function useUpdatePod(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ podId, patch }: PodUpdateInput) => updatePod(getAppClient(), podId, patch),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) }),
  });
}

/** Delete one pod; the caller keeps its confirmation open on failure. */
export function useDeletePod(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (podId: string) => deletePod(getAppClient(), podId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) }),
  });
}
