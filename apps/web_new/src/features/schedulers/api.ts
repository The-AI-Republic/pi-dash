// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scheduler definition data for parity/specs/agents.md §1 (AGT-001–004).
// One workspace-scoped list feeds the catalog and later install flows;
// successful writes invalidate it so live install counts stay authoritative.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createScheduler,
  deleteScheduler,
  isApiError,
  listSchedulers,
  updateScheduler,
  type ApiClient,
  type Scheduler,
  type SchedulerCreateInput,
  type SchedulerUpdateInput,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../core/query/client.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";

export const schedulerKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "schedulers"] as const,
  list: (workspaceSlug: string) => [...schedulerKeys.all(workspaceSlug), "list"] as const,
};

export function schedulersQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: schedulerKeys.list(workspaceSlug),
    queryFn: () => listSchedulers(client, workspaceSlug),
    staleTime: LIST_STALE_TIME_MS,
  });
}

/** The workspace catalog; `enabled: false` keeps a closed dialog quiet. */
export function useSchedulers(workspaceSlug: string, enabled = true) {
  return useQuery({ ...schedulersQueryOptions(getAppClient(), workspaceSlug), enabled });
}

export function schedulerFailureMessage(error: unknown): string {
  if (isApiError(error) && error.message.trim() !== "") return error.message;
  return "Check your connection and try again.";
}

export function schedulerFieldError(error: unknown, field: string): string | undefined {
  if (!isApiError(error)) return undefined;
  const messages = error.fields?.[field];
  return messages === undefined || messages.length === 0 ? undefined : messages.join(" ");
}

export function useCreateScheduler(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<Scheduler, Error, SchedulerCreateInput>({
    mutationFn: (input) => createScheduler(getAppClient(), workspaceSlug, input),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: schedulerKeys.list(workspaceSlug) });
      notifySuccess("Scheduler created", "The definition is ready to install.");
    },
    onError: (error) => notifyError("Could not create scheduler", schedulerFailureMessage(error)),
  });
}

export interface SchedulerUpdateRequest {
  schedulerId: string;
  input: SchedulerUpdateInput;
}

export function useUpdateScheduler(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<Scheduler, Error, SchedulerUpdateRequest>({
    mutationFn: ({ schedulerId, input }) => updateScheduler(getAppClient(), workspaceSlug, schedulerId, input),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: schedulerKeys.list(workspaceSlug) });
      notifySuccess("Scheduler updated");
    },
    onError: (error) => notifyError("Could not update scheduler", schedulerFailureMessage(error)),
  });
}

export function useDeleteScheduler(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<number, Error, string>({
    mutationFn: (schedulerId) => deleteScheduler(getAppClient(), workspaceSlug, schedulerId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: schedulerKeys.list(workspaceSlug) });
      notifySuccess("Scheduler deleted", "Its handle can now be used again.");
    },
    onError: (error) => notifyError("Could not delete scheduler", schedulerFailureMessage(error)),
  });
}
