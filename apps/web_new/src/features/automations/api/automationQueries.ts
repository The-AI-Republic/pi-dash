// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Automations reads and writes (parity/specs/agents.md §8, AGT-034–037).
// The page reads the workspace's project-details collection (the only
// read that carries the archive delay, close delay and cancelled-state
// target) plus one project's state list. The project update answers that
// same row shape, so a successful write folds straight back into the
// shared cache. Both reads are reference data with the shared freshness.

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query";
import { listStates, updateProject, type ApiClient, type ProjectDetails, type StateList } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { projectKeys } from "../../projects/index.js";

/** Notice title shared by both rows' failed updates (AGT-037). */
export const AUTOMATION_UPDATE_FAILED_TITLE = "Error!";
/** Notice detail for a failed automation update. */
export const AUTOMATION_UPDATE_FAILED_MESSAGE = "Something went wrong. Please try again.";
/** Heading of the refusal view non-admins see (AGT-037). */
export const AUTOMATIONS_NOT_AUTHORIZED_TITLE = "Oops! You are not authorized to view this page";

/** Fields the automations page writes through the project update. */
export interface AutomationPatch {
  archive_in?: number;
  close_in?: number;
  default_state?: string | null;
}

export const automationKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "automations"] as const,
  states: (workspaceSlug: string, projectId: string) =>
    [...automationKeys.all(workspaceSlug), "states", projectId] as const,
};

/** One project's states; the close row filters the cancelled group. */
export function automationStatesQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: automationKeys.states(workspaceSlug, projectId),
    queryFn: () => listStates(client, workspaceSlug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

interface UpdateAutomationInput {
  projectId: string;
  patch: AutomationPatch;
}

/**
 * Patch the project with an automation field and fold the answered row
 * back into the projects collection so every reader of it shows the
 * server's stored values.
 */
export function useUpdateAutomation(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectDetails, Error, UpdateAutomationInput>({
    mutationFn: ({ projectId, patch }) => updateProject(getAppClient(), workspaceSlug, projectId, patch),
    onSuccess: (project) => {
      queryClient.setQueryData(projectKeys.details(workspaceSlug), (rows: ProjectDetails[] | undefined) =>
        rows?.map((row) => (row.id === project.id ? project : row))
      );
    },
  });
}

export type { ProjectDetails, StateList };
