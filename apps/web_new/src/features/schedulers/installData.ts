// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project install data for parity/specs/agents.md §2 (AGT-006–009).
// One workspace-scoped definition list plus one install list per project;
// any write to an install invalidates both, because the catalog's install
// count and the project's list must agree after a create, a flip, an edit
// or a removal. The install dialogs and the list share these keys.

import { queryOptions, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  createSchedulerBinding,
  deleteSchedulerBinding,
  getSchedulerBinding,
  isApiError,
  listBindingRuns,
  listSchedulerBindings,
  updateSchedulerBinding,
  type ApiClient,
  type SchedulerBinding,
  type SchedulerBindingCreateInput,
  type SchedulerBindingDetail,
  type SchedulerBindingList,
  type SchedulerBindingUpdateInput,
} from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../core/query/client.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";

export const bindingKeys = {
  all: (workspaceSlug: string, projectId: string) =>
    [...workspaceScope(workspaceSlug), "scheduler-bindings", projectId] as const,
  list: (workspaceSlug: string, projectId: string) => [...bindingKeys.all(workspaceSlug, projectId), "list"] as const,
  detail: (workspaceSlug: string, projectId: string, bindingId: string) =>
    [...bindingKeys.all(workspaceSlug, projectId), "detail", bindingId] as const,
  runs: (workspaceSlug: string, projectId: string, bindingId: string, page: number) =>
    [...bindingKeys.all(workspaceSlug, projectId), "runs", bindingId, page] as const,
};

/** How often the detail's run history re-reads while the page is open. */
export const RUNS_POLL_INTERVAL_MS = 15_000;

/** The install's own page, including the composed prompt the API builds. */
export function bindingDetailQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  bindingId: string
) {
  return queryOptions({
    queryKey: bindingKeys.detail(workspaceSlug, projectId, bindingId),
    queryFn: () => getSchedulerBinding(client, workspaceSlug, projectId, bindingId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

export function useSchedulerBindingDetail(workspaceSlug: string, projectId: string, bindingId: string) {
  return useQuery(bindingDetailQueryOptions(getAppClient(), workspaceSlug, projectId, bindingId));
}

/**
 * One page of the install's run history. The detail page holds this query
 * on a fifteen-second poll, so a firing appears without a reload; paging
 * is the page number, and out-of-range pages come back empty with the
 * real total.
 */
export function bindingRunsQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  bindingId: string,
  page: number
) {
  return queryOptions({
    queryKey: bindingKeys.runs(workspaceSlug, projectId, bindingId, page),
    queryFn: () => listBindingRuns(client, workspaceSlug, projectId, bindingId, { page }),
    staleTime: 0,
    refetchInterval: RUNS_POLL_INTERVAL_MS,
  });
}

export function useSchedulerBindingRuns(workspaceSlug: string, projectId: string, bindingId: string, page: number) {
  return useQuery(bindingRunsQueryOptions(getAppClient(), workspaceSlug, projectId, bindingId, page));
}

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

interface ToggleContext {
  previousList: SchedulerBindingList | undefined;
  previousDetail: SchedulerBindingDetail | undefined;
}

export function useToggleSchedulerBinding(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<SchedulerBindingDetail, Error, ToggleBindingRequest, ToggleContext>({
    mutationFn: ({ projectId, bindingId, enabled }) =>
      updateSchedulerBinding(getAppClient(), workspaceSlug, projectId, bindingId, { enabled }),
    onMutate: async ({ projectId, bindingId, enabled }) => {
      const listKey = bindingKeys.list(workspaceSlug, projectId);
      const detailKey = bindingKeys.detail(workspaceSlug, projectId, bindingId);
      await Promise.all([
        queryClient.cancelQueries({ queryKey: listKey }),
        queryClient.cancelQueries({ queryKey: detailKey }),
      ]);
      const previousList = queryClient.getQueryData<SchedulerBindingList>(listKey);
      const previousDetail = queryClient.getQueryData<SchedulerBindingDetail>(detailKey);
      queryClient.setQueryData<SchedulerBindingList>(listKey, (current) =>
        current?.map((binding) => (binding.id === bindingId ? { ...binding, enabled } : binding))
      );
      queryClient.setQueryData<SchedulerBindingDetail>(detailKey, (current) =>
        current === undefined ? current : { ...current, enabled }
      );
      return { previousList, previousDetail };
    },
    onError: (error, { projectId, bindingId }, context) => {
      queryClient.setQueryData<SchedulerBindingList>(bindingKeys.list(workspaceSlug, projectId), context?.previousList);
      queryClient.setQueryData<SchedulerBindingDetail>(
        bindingKeys.detail(workspaceSlug, projectId, bindingId),
        context?.previousDetail
      );
      notifyError("Something went wrong", error.message);
    },
    onSuccess: (data, { projectId, bindingId, enabled }) => {
      queryClient.setQueryData(bindingKeys.detail(workspaceSlug, projectId, bindingId), data);
      notifySuccess(
        "Install updated",
        enabled ? "It will fire on the next scheduled tick." : "It will not fire until re-enabled."
      );
    },
    onSettled: async (_data, _error, { projectId }) => {
      await invalidateInstalls(queryClient, workspaceSlug, projectId);
    },
  });
}

/**
 * Save an edited install. The mutation resolves with the detail-shape row
 * so a detail caller can hand the response straight to its cache; the
 * caller settles the calendar and the detail, this settles the list.
 */
export interface UpdateBindingRequest {
  projectId: string;
  bindingId: string;
  input: SchedulerBindingUpdateInput;
}

export function useUpdateSchedulerBinding(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<SchedulerBindingDetail, Error, UpdateBindingRequest>({
    mutationFn: ({ projectId, bindingId, input }) =>
      updateSchedulerBinding(getAppClient(), workspaceSlug, projectId, bindingId, input),
    onSuccess: async (data, { projectId }) => {
      notifySuccess("Install updated", "Subsequent runs use the new settings.");
      if (data !== undefined) {
        queryClient.setQueryData(bindingKeys.detail(workspaceSlug, projectId, data.id), data);
      }
      await invalidateInstalls(queryClient, workspaceSlug, projectId);
    },
    onError: (error) => notifyError("Something went wrong", schedulerBindingFailure(error)),
  });
}

/** Server detail worth showing for a refused binding write. */
export function schedulerBindingFailure(error: unknown): string {
  if (!isApiError(error)) return "Check your connection and try again.";
  const fields = error.fields ?? {};
  for (const field of ["rrule", "dtstart", "tzid", "extra_context", "pod", "outcome_mode", "enabled"]) {
    const messages = fields[field];
    if (messages !== undefined && messages.length > 0) return messages.join(" ");
  }
  return error.message.trim() === "" ? "The install was refused." : error.message;
}

/**
 * Uninstall one install. The workspace definition is untouched; only the
 * project binding goes, so the definition stays in the catalog and can be
 * installed again.
 */
export interface UninstallBindingRequest {
  projectId: string;
  bindingId: string;
}

export function useUninstallSchedulerBinding(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation<void, Error, UninstallBindingRequest>({
    mutationFn: async ({ projectId, bindingId }) => {
      await deleteSchedulerBinding(getAppClient(), workspaceSlug, projectId, bindingId);
    },
    onSuccess: async (_data, { projectId }) => {
      notifySuccess("Scheduler uninstalled", "It stops firing on this project until reinstalled.");
      await invalidateInstalls(queryClient, workspaceSlug, projectId);
    },
    onError: (error) => notifyError("Something went wrong", schedulerBindingFailure(error)),
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
