// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook reads and writes (parity/specs/workspace-settings.md §18-§24,
// WSET-019-024 + WSET-041). The list and detail reads follow the
// settings mount-only policy (§38); every write refreshes the affected
// caches so the list and the detail form agree with the server without a
// page reload. The secret never enters a cache: only the create and
// regenerate answers carry it, and the owning component keeps it in
// memory until the reveal closes (§20).

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createWorkspaceWebhook,
  deleteWorkspaceWebhook,
  getWorkspaceWebhook,
  listWorkspaceWebhooks,
  regenerateWorkspaceWebhookSecret,
  updateWorkspaceWebhook,
  type ApiClient,
  type CreateWorkspaceWebhookInput,
  type CreatedWorkspaceWebhook,
  type UpdateWorkspaceWebhookInput,
  type WorkspaceWebhook,
  type WorkspaceWebhookList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { workspaceScope } from "../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "../lib/settingsQuery.js";

export const webhookKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "workspace-settings", "webhooks"] as const,
  list: (workspaceSlug: string) => [...webhookKeys.all(workspaceSlug), "list"] as const,
  detail: (workspaceSlug: string, webhookId: string) =>
    [...webhookKeys.all(workspaceSlug), "detail", webhookId] as const,
};

/** A detail read that answered 404 stays missing on the cache too. */
export function isMissingWebhookFailure(error: unknown): boolean {
  return typeof error === "object" && error !== null && (error as { status?: unknown }).status === 404;
}

/**
 * The admin list. No `secret_key` reaches it: the contract's strict parse
 * drops the backend's leftover echo (§44 field note).
 */
export function webhooksListQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions<WorkspaceWebhookList>({
    queryKey: webhookKeys.list(workspaceSlug),
    queryFn: () => listWorkspaceWebhooks(client, workspaceSlug),
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
  });
}

/** One webhook for the detail form (§21). A 404 is not retried. */
export function webhookDetailQueryOptions(client: ApiClient, workspaceSlug: string, webhookId: string) {
  return queryOptions<WorkspaceWebhook>({
    queryKey: webhookKeys.detail(workspaceSlug, webhookId),
    queryFn: () => getWorkspaceWebhook(client, workspaceSlug, webhookId),
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
    retry: (failureCount, error) => !isMissingWebhookFailure(error) && failureCount < 1,
  });
}

export function useWebhooksList(workspaceSlug: string) {
  return useQuery(webhooksListQueryOptions(getAppClient(), workspaceSlug));
}

export function useWebhookDetail(workspaceSlug: string, webhookId: string) {
  return useQuery(webhookDetailQueryOptions(getAppClient(), workspaceSlug, webhookId));
}

/** Replace or prepend a row in the list cache; returns nothing to change when absent. */
export function upsertListRow(rows: WorkspaceWebhookList, row: WorkspaceWebhook): WorkspaceWebhookList {
  const index = rows.findIndex((existing) => existing.id === row.id);
  if (index === -1) return [row, ...rows];
  const next = rows.slice();
  next[index] = row;
  return next;
}

/**
 * Keep a write's row in the list cache only when that list already holds
 * every row. Seeding an unread list with the one row a detail read knows
 * about would hide every other webhook until the query refetches, so an
 * uncached list stays uncached and the next visit fetches it whole.
 */
function keepListRow(
  queryClient: ReturnType<typeof useQueryClient>,
  workspaceSlug: string,
  row: WorkspaceWebhook
): void {
  queryClient.setQueryData<WorkspaceWebhookList>(webhookKeys.list(workspaceSlug), (rows) =>
    rows === undefined ? rows : upsertListRow(rows, row)
  );
}

/** The stored half of a create/regenerate answer: everything but the key. */
export function webhookRowOf(issued: CreatedWorkspaceWebhook): WorkspaceWebhook {
  return {
    id: issued.id,
    url: issued.url,
    is_active: issued.is_active,
    created_at: issued.created_at,
    updated_at: issued.updated_at,
    project: issued.project,
    issue: issued.issue,
    cycle: issued.cycle,
    module: issued.module,
    issue_comment: issued.issue_comment,
  };
}

/**
 * A create or regenerate answer as the hooks hand it back. The one-time
 * key deliberately does not ride on this value: TanStack keeps a
 * mutation's result in its cache for the gc window, so a key there would
 * outlive the reveal. `mutationFn` passes the whole answer to the
 * caller's callback first, then resolves with the stored row only
 * (§20/§22).
 */
export interface IssuedWebhook {
  row: WorkspaceWebhook;
}

/**
 * Hand the full answer (key included) to the caller's callback, then
 * resolve with the stored row. The callback runs before the mutation
 * result exists, so the key is never part of that result.
 */
function issueWebhook(
  issued: CreatedWorkspaceWebhook,
  onIssued: (issued: CreatedWorkspaceWebhook) => void
): IssuedWebhook {
  onIssued(issued);
  return { row: webhookRowOf(issued) };
}

export function useCreateWebhook(workspaceSlug: string, onIssued: (issued: CreatedWorkspaceWebhook) => void) {
  const queryClient = useQueryClient();
  const client = getAppClient();
  return useMutation<IssuedWebhook, unknown, CreateWorkspaceWebhookInput>({
    mutationFn: async (input) => issueWebhook(await createWorkspaceWebhook(client, workspaceSlug, input), onIssued),
    onSuccess: (issued) => {
      keepListRow(queryClient, workspaceSlug, issued.row);
    },
  });
}

export interface UpdateWebhookVariables {
  webhookId: string;
  input: UpdateWorkspaceWebhookInput;
}

export function useUpdateWebhook(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const client = getAppClient();
  return useMutation<WorkspaceWebhook, unknown, UpdateWebhookVariables>({
    mutationFn: ({ webhookId, input }) => updateWorkspaceWebhook(client, workspaceSlug, webhookId, input),
    onSuccess: (saved, { webhookId }) => {
      queryClient.setQueryData(webhookKeys.detail(workspaceSlug, webhookId), saved);
      keepListRow(queryClient, workspaceSlug, saved);
    },
  });
}

/** The list row's active switch writes `is_active` alone (§24). */
export function useSetWebhookActive(workspaceSlug: string) {
  return useUpdateWebhook(workspaceSlug);
}

export function useDeleteWebhook(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const client = getAppClient();
  return useMutation<number, unknown, string>({
    mutationFn: (webhookId) => deleteWorkspaceWebhook(client, workspaceSlug, webhookId),
    onSuccess: (_status, webhookId) => {
      queryClient.removeQueries({ queryKey: webhookKeys.detail(workspaceSlug, webhookId) });
      queryClient.setQueryData<WorkspaceWebhookList>(webhookKeys.list(workspaceSlug), (rows) =>
        rows === undefined ? rows : rows.filter((row) => row.id !== webhookId)
      );
    },
  });
}

export function useRegenerateWebhookSecret(workspaceSlug: string, onIssued: (issued: CreatedWorkspaceWebhook) => void) {
  const queryClient = useQueryClient();
  const client = getAppClient();
  return useMutation<IssuedWebhook, unknown, string>({
    mutationFn: async (webhookId) =>
      issueWebhook(await regenerateWorkspaceWebhookSecret(client, workspaceSlug, webhookId), onIssued),
    onSuccess: (issued, webhookId) => {
      queryClient.setQueryData(webhookKeys.detail(workspaceSlug, webhookId), issued.row);
      keepListRow(queryClient, workspaceSlug, issued.row);
    },
  });
}
