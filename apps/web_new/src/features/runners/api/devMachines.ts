// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machine reads and writes (NEWFRONT-555, RUN-037–040). The machines
// table polls on its own short cadence; every write re-reads the subtree so
// the row, its counts and the runner list agree once the request settles.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  deleteDevMachine,
  listDevMachines,
  revokeDevMachine,
  rotateDevMachine,
  type ApiClient,
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
