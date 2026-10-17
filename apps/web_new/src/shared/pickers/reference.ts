// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reference reads behind the shared property pickers
// (parity/specs/issues.md §17, ISS-208/209/211/212/213/216/217/218).
// Picker-owned keys under the workspace scope: the palette keeps its
// own parallel reads, and the cycles/modules/estimates areas own the
// full lists — the pickers only borrow the thin projections. Every
// read resolves lazily, enabled when the picker's popup opens.
import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  getIntakeState,
  getProject,
  isApiError,
  listCycles,
  listMembers,
  listModules,
  listPods,
  listProjectEstimates,
  listProjects,
  listStates,
  listWorkspaceMembers,
  type ApiClient,
  type State,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../core/query/client.js";

export const pickerKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "pickers"] as const,
  states: (workspaceSlug: string, projectId: string) =>
    [...pickerKeys.all(workspaceSlug), "states", projectId] as const,
  projectMembers: (workspaceSlug: string, projectId: string) =>
    [...pickerKeys.all(workspaceSlug), "project-members", projectId] as const,
  workspaceMembers: (workspaceSlug: string) => [...pickerKeys.all(workspaceSlug), "workspace-members"] as const,
  projects: (workspaceSlug: string) => [...pickerKeys.all(workspaceSlug), "projects"] as const,
  cycles: (workspaceSlug: string, projectId: string) =>
    [...pickerKeys.all(workspaceSlug), "cycles", projectId] as const,
  modules: (workspaceSlug: string, projectId: string) =>
    [...pickerKeys.all(workspaceSlug), "modules", projectId] as const,
  estimates: (workspaceSlug: string, projectId: string) =>
    [...pickerKeys.all(workspaceSlug), "estimates", projectId] as const,
  intakeStates: (workspaceSlug: string, projectId: string) =>
    [...pickerKeys.all(workspaceSlug), "intake-states", projectId] as const,
  pods: (workspaceSlug: string, projectId: string) => [...pickerKeys.all(workspaceSlug), "pods", projectId] as const,
  project: (workspaceSlug: string, projectId: string) =>
    [...pickerKeys.all(workspaceSlug), "project", projectId] as const,
};

function referenceOptions<T>(key: readonly unknown[], fetch: () => Promise<T>) {
  return queryOptions({
    queryKey: key,
    queryFn: fetch,
    staleTime: REFERENCE_STALE_TIME_MS,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

export function pickerStatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return referenceOptions(pickerKeys.states(workspaceSlug, projectId), () =>
    listStates(client, workspaceSlug, projectId)
  );
}

export function pickerProjectMembersQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return referenceOptions(pickerKeys.projectMembers(workspaceSlug, projectId), () =>
    listMembers(client, workspaceSlug, projectId)
  );
}

export function pickerWorkspaceMembersQueryOptions(client: ApiClient, workspaceSlug: string) {
  return referenceOptions(pickerKeys.workspaceMembers(workspaceSlug), () =>
    listWorkspaceMembers(client, workspaceSlug)
  );
}

export function pickerProjectsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return referenceOptions(pickerKeys.projects(workspaceSlug), () => listProjects(client, workspaceSlug));
}

export function pickerCyclesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return referenceOptions(pickerKeys.cycles(workspaceSlug, projectId), () =>
    listCycles(client, workspaceSlug, projectId)
  );
}

export function pickerModulesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return referenceOptions(pickerKeys.modules(workspaceSlug, projectId), () =>
    listModules(client, workspaceSlug, projectId)
  );
}

/** Project workflow states, fetched once the popup opens. */
export function usePickerStates(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...pickerStatesQueryOptions(getAppClient(), workspaceSlug, projectId), enabled });
}

/** Bare project memberships (ids plus roles); names join from the directory. */
export function usePickerProjectMembers(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...pickerProjectMembersQueryOptions(getAppClient(), workspaceSlug, projectId), enabled });
}

/** Workspace member directory: names, avatars and suspended flags. */
export function usePickerWorkspaceMembers(workspaceSlug: string, enabled: boolean) {
  return useQuery({ ...pickerWorkspaceMembersQueryOptions(getAppClient(), workspaceSlug), enabled });
}

/** Joined workspace projects for the project picker. */
export function usePickerProjects(workspaceSlug: string, enabled: boolean) {
  return useQuery({ ...pickerProjectsQueryOptions(getAppClient(), workspaceSlug), enabled });
}

/** Thin project cycle rows for the cycle picker. */
export function usePickerCycles(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...pickerCyclesQueryOptions(getAppClient(), workspaceSlug, projectId), enabled });
}

/** Thin project module rows for the module picker. */
export function usePickerModules(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...pickerModulesQueryOptions(getAppClient(), workspaceSlug, projectId), enabled });
}

export function pickerEstimatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return referenceOptions(pickerKeys.estimates(workspaceSlug, projectId), () =>
    listProjectEstimates(client, workspaceSlug, projectId)
  );
}

export function pickerIntakeStatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return referenceOptions(pickerKeys.intakeStates(workspaceSlug, projectId), async (): Promise<State[]> => {
    try {
      return [await getIntakeState(client, workspaceSlug, projectId)];
    } catch (error) {
      if (isApiError(error) && error.status === 404) return [];
      throw error;
    }
  });
}

export function pickerPodsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return referenceOptions(pickerKeys.pods(workspaceSlug, projectId), () => listPods(client, { projectId }));
}

export function pickerProjectQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return referenceOptions(pickerKeys.project(workspaceSlug, projectId), () =>
    getProject(client, workspaceSlug, projectId)
  );
}

/** Project estimate systems for the estimate picker. */
export function usePickerEstimates(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...pickerEstimatesQueryOptions(getAppClient(), workspaceSlug, projectId), enabled });
}

/** Project intake states for the intake-state picker: the single triage state, or empty. */
export function usePickerIntakeStates(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...pickerIntakeStatesQueryOptions(getAppClient(), workspaceSlug, projectId), enabled });
}

/** Project pods for the execution-target picker, default first. */
export function usePickerPods(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...pickerPodsQueryOptions(getAppClient(), workspaceSlug, projectId), enabled });
}

/** Project row carrying the executor options for the execution-target picker. */
export function usePickerProject(workspaceSlug: string, projectId: string, enabled: boolean) {
  return useQuery({ ...pickerProjectQueryOptions(getAppClient(), workspaceSlug, projectId), enabled });
}
