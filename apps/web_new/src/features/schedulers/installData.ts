// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project install data for parity/specs/agents.md §2 (AGT-006–009).
// One workspace-scoped definition list plus one install list per project;
// any write to an install invalidates both, because the catalog's install
// count and the project's list must agree after a create, a flip or a
// removal. The enable switch is the only write this slice ships; the edit
// and uninstall dialogs live in NEWFRONT-587 and reuse these keys.

import { queryOptions, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  createSchedulerBinding,
  listSchedulerBindings,
  updateSchedulerBinding,
  type ApiClient,
  type SchedulerBinding,
  type SchedulerBindingCreateInput,
  type SchedulerBindingList,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../core/query/client.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";

export const bindingKeys = {
  all: (workspaceSlug: string, projectId: string) =>
    [...workspaceScope(workspaceSlug), "scheduler-bindings", projectId] as const,
  list: (workspaceSlug: string, projectId: string) => [...bindingKeys.all(workspaceSlug, projectId), "list"] as const,
};

/** The project's installs. The list order is the API's (newest first). */
export function bindingsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: bindingKeys.list(workspaceSlug, projectId),
    queryFn: () => listSchedulerBindings(client, workspaceSlug, projectId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

export function useSchedulerBindings(workspaceSlug: string, projectId: string) {
  return useQuery(bindingsQueryOptions(getAppClient(), workspaceSlug, projectId));
}

/**
 * The scheduler key the catalog slice owns. Invalidation of the catalog
 * list goes through this literal so the install flows never import the
 * catalog module's internals; the shape matches schedulerKeys.list.
 */
export function schedulerCatalogKey(workspaceSlug: string) {
  return [...workspaceScope(workspaceSlug), "schedulers", "list"] as const;
}

/** Install a definition on one project. */
export interface InstallBindingRequest {
  projectId: string;
  input: SchedulerBindingCreateInput;
}

export function useInstallSchedulerBinding(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<SchedulerBinding, Error, InstallBindingRequest>({
    mutationFn: ({ projectId, input }) => createSchedulerBinding(getAppClient(), workspaceSlug, projectId, input),
    onSuccess: async (_binding, { projectId }) => {
      await invalidateInstalls(queryClient, workspaceSlug, projectId);
    },
  });
}

/**
 * The enable switch. Optimistic: the row flips immediately and rolls back
 * with a notice when the write is refused. The caller holds the in-flight
 * gate so a double click cannot race two writes.
 */
export interface ToggleBindingRequest {
  projectId: string;
  bindingId: string;
  enabled: boolean;
}

export function useToggleSchedulerBinding(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, ToggleBindingRequest, { previous: SchedulerBindingList | undefined }>({
    mutationFn: async ({ projectId, bindingId, enabled }) => {
      await updateSchedulerBinding(getAppClient(), workspaceSlug, projectId, bindingId, { enabled });
    },
    onMutate: async ({ projectId, bindingId, enabled }) => {
      const key = bindingKeys.list(workspaceSlug, projectId);
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<SchedulerBindingList>(key);
      queryClient.setQueryData<SchedulerBindingList>(key, (current) =>
        current?.map((binding) => (binding.id === bindingId ? { ...binding, enabled } : binding))
      );
      return { previous };
    },
    onError: (error, { projectId }, context) => {
      queryClient.setQueryData<SchedulerBindingList>(bindingKeys.list(workspaceSlug, projectId), context?.previous);
      notifyError("Could not update the install", error.message);
    },
    onSuccess: () => notifySuccess("Install updated"),
    onSettled: async (_data, _error, { projectId }) => {
      await invalidateInstalls(queryClient, workspaceSlug, projectId);
    },
  });
}

/** Project installs and the catalog counts refresh together. */
export async function invalidateInstalls(
  queryClient: QueryClient,
  workspaceSlug: string,
  projectId: string
): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: bindingKeys.list(workspaceSlug, projectId) }),
    queryClient.invalidateQueries({ queryKey: schedulerCatalogKey(workspaceSlug) }),
  ]);
}
