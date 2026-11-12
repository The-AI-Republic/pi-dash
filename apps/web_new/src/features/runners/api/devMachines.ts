// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machine reads and writes (NEWFRONT-555, RUN-037–040). The machines
// table polls on its own short cadence; every write re-reads the subtree so
// the row, its counts and the runner list agree once the request settles.
// Built-in engine design §4.10, §8: rename, engine controls, engine
// sign-out and the merge of a replaced computer ride the same writer.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  deleteDevMachine,
  listDevMachines,
  listRunners,
  mergeDevMachineInto,
  revokeDevMachine,
  rotateDevMachine,
  signOutDevMachineEngine,
  updateDevMachine,
  type ApiClient,
  type DevMachinePatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { runnersKeys } from "./keys.js";

/** The list moves fast enough that status changes land without navigation. */
export const DEV_MACHINES_POLL_INTERVAL_MS = 5_000;

/** Machines attached to one workspace, newest-seen first. */
export function devMachinesQueryOptions(client: ApiClient, workspaceSlug: string, workspaceId: string) {
  return queryOptions({
    queryKey: runnersKeys.devMachines(workspaceSlug),
    queryFn: () => listDevMachines(client, workspaceId),
    staleTime: 0,
    refetchInterval: DEV_MACHINES_POLL_INTERVAL_MS,
    // 500s are app-retryable; retrying here would hide the asserted error
    // states and the recovery poll is the user-visible retry path.
    retry: false,
  });
}

export function useDevMachines(workspaceSlug: string, workspaceId: string | undefined, enabled = true) {
  const client = getAppClient();
  return useQuery({
    ...devMachinesQueryOptions(client, workspaceSlug, workspaceId ?? ""),
    enabled: enabled && workspaceId !== undefined,
  });
}

function useMachineWrite<TInput>(workspaceSlug: string, write: (client: ApiClient, input: TInput) => Promise<unknown>) {
  const queryClient = useQueryClient();
  const client = getAppClient();
  return useMutation({
    mutationFn: (input: TInput) => write(client, input),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) }),
  });
}

/** Rotate one machine's token; the row survives with fresh credentials. */
export function useRotateDevMachine(workspaceSlug: string, workspaceId: string) {
  return useMachineWrite(workspaceSlug, (client, machineId: string) =>
    rotateDevMachine(client, machineId, workspaceId)
  );
}

/** Permanently revoke one machine; hosted runners are revoked too. */
export function useRevokeDevMachine(workspaceSlug: string, workspaceId: string) {
  return useMachineWrite(workspaceSlug, (client, machineId: string) =>
    revokeDevMachine(client, machineId, workspaceId)
  );
}

/** Drop one machine, cascading local teardown by default (RUN-040). */
export function useDeleteDevMachine(workspaceSlug: string, workspaceId: string) {
  return useMachineWrite(workspaceSlug, (client, machineId: string) =>
    deleteDevMachine(client, machineId, workspaceId)
  );
}

/** One machine settings write: a rename or an engine control change. */
export interface DevMachineSettingsInput {
  machineId: string;
  patch: DevMachinePatch;
}

/** Rename a machine or change its engine controls; owner only. */
export function useUpdateDevMachine(workspaceSlug: string, workspaceId: string) {
  return useMachineWrite(workspaceSlug, (client, input: DevMachineSettingsInput) =>
    updateDevMachine(client, input.machineId, workspaceId, input.patch)
  );
}

/** Revoke the machine's engine credential only. */
export function useSignOutDevMachineEngine(workspaceSlug: string, workspaceId: string) {
  return useMachineWrite(workspaceSlug, (client, machineId: string) =>
    signOutDevMachineEngine(client, machineId, workspaceId)
  );
}

/** Which offline machine merges into which live one. */
export interface DevMachineMergeInput {
  sourceId: string;
  targetId: string;
}

/** Merge a replaced computer into the one that replaced it. */
export function useMergeDevMachine(workspaceSlug: string) {
  return useMachineWrite(workspaceSlug, (client, input: DevMachineMergeInput) =>
    mergeDevMachineInto(client, input.sourceId, input.targetId)
  );
}

/** Cache key for the runners the machines page expands into. */
export function devMachineRunnersKey(workspaceSlug: string) {
  return [...runnersKeys.devMachines(workspaceSlug), "runners"] as const;
}

/**
 * Every runner of the workspace the caller can see, built-in engines
 * included, for the expanded machine rows. Older servers hide engines
 * unless asked; newer ones list them always.
 */
export function devMachineRunnersQueryOptions(client: ApiClient, workspaceSlug: string, workspaceId: string) {
  return queryOptions({
    queryKey: devMachineRunnersKey(workspaceSlug),
    queryFn: () => listRunners(client, workspaceId, { includeBundled: true }),
    staleTime: 0,
    refetchInterval: DEV_MACHINES_POLL_INTERVAL_MS,
  });
}

/** The expanded rows' runners; waits for the workspace id. */
export function useDevMachineRunners(workspaceSlug: string, workspaceId: string | undefined, enabled = true) {
  const client = getAppClient();
  return useQuery({
    ...devMachineRunnersQueryOptions(client, workspaceSlug, workspaceId ?? ""),
    enabled: enabled && workspaceId !== undefined,
  });
}
