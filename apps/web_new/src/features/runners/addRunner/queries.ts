// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-runner reads (RUN-006/RUN-007): the connected machines, the project
// list the picker offers, and the scope's pods. The machine list refreshes
// while the modal is open so a daemon coming online becomes usable without a
// reopen; a machine leaving the set is handled by the form rules.

import { queryOptions, useQuery } from "@tanstack/react-query";
import {
  createRunnerOnMachine,
  getCreateRunnerStatus,
  listDevMachines,
  listPods,
  listProjects,
  type ApiClient,
  type CreateRunnerStatus,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { pickerKeys } from "../../../shared/pickers/reference.js";
import { runnersKeys, type RunnersScope } from "../api/keys.js";
import { MACHINE_REFRESH_MS } from "./model.js";

export const addRunnerKeys = {
  machines: (workspaceSlug: string, workspaceId: string) =>
    [...runnersKeys.all(workspaceSlug), "dev-machines", workspaceId] as const,
};

/** Connected-capable machines for the workspace; polls while enabled. */
export function machinesQueryOptions(client: ApiClient, workspaceSlug: string, workspaceId: string) {
  return queryOptions({
    queryKey: addRunnerKeys.machines(workspaceSlug, workspaceId),
    queryFn: () => listDevMachines(client, workspaceId),
    refetchInterval: MACHINE_REFRESH_MS,
    staleTime: 0,
  });
}

/** Projects the project picker offers (the same reference read other pickers share). */
export function projectsForRunnerQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: pickerKeys.projects(workspaceSlug),
    queryFn: () => listProjects(client, workspaceSlug),
    staleTime: 5 * 60_000,
  });
}

/** Pods the pod picker offers for the scope. */
export function podsForRunnerQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  workspaceId: string,
  projectId: RunnersScope
) {
  return queryOptions({
    queryKey: runnersKeys.pods(workspaceSlug, projectId),
    queryFn: () => listPods(client, projectId === null ? { workspaceId } : { projectId }),
    staleTime: 30_000,
  });
}

export function useAddRunnerMachines(workspaceSlug: string, workspaceId: string, enabled: boolean) {
  return useQuery({ ...machinesQueryOptions(getAppClient(), workspaceSlug, workspaceId), enabled });
}

export function useAddRunnerProjects(workspaceSlug: string, enabled: boolean) {
  return useQuery({ ...projectsForRunnerQueryOptions(getAppClient(), workspaceSlug), enabled });
}

export function useAddRunnerPods(
  workspaceSlug: string,
  workspaceId: string,
  projectId: RunnersScope,
  enabled: boolean
) {
  return useQuery({ ...podsForRunnerQueryOptions(getAppClient(), workspaceSlug, workspaceId, projectId), enabled });
}

/**
 * Push a create command down a machine and read its status. The modal loop
 * owns the timing; this only wires the contracts.
 */
export function createRunnerTransport(): {
  createRunner: (machineId: string, input: CreateRunnerOnMachineArgs) => Promise<{ request_id: string }>;
  readStatus: (machineId: string, requestId: string, workspaceId: string) => Promise<CreateRunnerStatus>;
} {
  return {
    createRunner: (machineId, input) => createRunnerOnMachine(getAppClient(), machineId, input),
    readStatus: (machineId, requestId, workspaceId) =>
      getCreateRunnerStatus(getAppClient(), machineId, requestId, workspaceId),
  };
}

export interface CreateRunnerOnMachineArgs {
  workspace: string;
  project: string;
  pod?: string;
  name?: string;
  working_dir?: string;
  agent?: string;
  model?: string;
  reasoning_effort?: string;
}
