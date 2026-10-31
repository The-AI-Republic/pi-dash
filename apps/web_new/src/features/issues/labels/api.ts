// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Label reads and writes (parity/specs/issues.md §19, ISS-226–229).
// The settings list and the shared picker read one cached list per
// project; writes patch it optimistically and roll back on failure.
// Renames land on every carrying surface through this shared cache.
// Drag moves roll back silently (pinned bug NEWFRONT-408).

import { queryOptions, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  createLabel,
  deleteLabel,
  isApiError,
  listLabels,
  listWorkspaceLabels,
  updateLabel,
  type ApiClient,
  type Label,
  type LabelCreate,
  type LabelList,
  type LabelPatch,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { notifyError } from "../../../shared/shell/index.js";

/** Failure title for a refused label create; duplicates ride the description. */
export const LABEL_CREATE_FAILED_TITLE = "Couldn't create the label";
/** Duplicate-name detail for creates (the picker reuses it). */
export const LABEL_DUPLICATE_MESSAGE = "Label already exists";
/** Failure title for a refused label update. */
export const LABEL_UPDATE_FAILED_TITLE = "Error while updating the label";
/** Duplicate-name detail for renames. */
export const LABEL_RENAME_DUPLICATE_MESSAGE = "A label with the same name already exists";
/** Failure title for a refused label delete. */
export const LABEL_DELETE_FAILED_TITLE = "Label could not be deleted. Please try again.";
/** Fallback detail when a failure carries no message. */
export const LABEL_FAILURE_FALLBACK = "Check your connection and try again.";
/** Notice title when the label list itself fails to load. */
export const LABELS_LOAD_FAILED_TITLE = "Couldn't load the labels";

export const labelKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "labels"] as const,
  list: (workspaceSlug: string, projectId: string) => [...labelKeys.all(workspaceSlug), "list", projectId] as const,
  workspace: (workspaceSlug: string) => [...labelKeys.all(workspaceSlug), "workspace-list"] as const,
};

/** One project's label list. Reference data with the shared freshness. */
export function labelsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string) {
  return queryOptions({
    queryKey: labelKeys.list(workspaceSlug, projectId),
    queryFn: () => listLabels(client, workspaceSlug, projectId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** The workspace's label list for cross-project hosts. */
export function workspaceLabelsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: labelKeys.workspace(workspaceSlug),
    queryFn: () => listWorkspaceLabels(client, workspaceSlug),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/**
 * One project's label list; pickers disable it until they open. An
 * empty project id never fires: cross-project hosts read the
 * workspace list instead.
 */
export function useLabels(workspaceSlug: string, projectId: string, enabled = true) {
  return useQuery({
    ...labelsQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: enabled && projectId !== "",
  });
}

/** The workspace's label list; pickers disable it until they open. */
export function useWorkspaceLabels(workspaceSlug: string, enabled = true) {
  return useQuery({ ...workspaceLabelsQueryOptions(getAppClient(), workspaceSlug), enabled });
}

/** True when the server refused a label write as a duplicate name. */
export function isLabelDuplicateError(error: unknown): boolean {
  if (!isApiError(error) || error.status !== 400) return false;
  const names = error.fields?.name;
  return names !== undefined && names.length > 0;
}

/** Server detail for a failure notice, falling back when empty. */
export function labelFailureDetail(error: unknown): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return LABEL_FAILURE_FALLBACK;
}

/** Next label cache with one row patched. */
export function applyLabelPatch(
  previous: LabelList | undefined,
  labelId: string,
  patch: LabelPatch
): LabelList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) => (row.id === labelId ? { ...row, ...patch } : row));
}

/** Next label cache without one row. */
export function removeCachedLabel(previous: LabelList | undefined, labelId: string): LabelList | undefined {
  if (previous === undefined) return undefined;
  return previous.filter((row) => row.id !== labelId);
}

/** Next label cache with the created row appended. */
export function appendCachedLabel(previous: LabelList | undefined, created: Label): LabelList {
  return previous === undefined ? [created] : [...previous, created];
}

/** Paint the optimistic patch, run the write, and roll back on failure. */
async function runLabelWrite<T>(
  queryClient: QueryClient,
  key: readonly unknown[],
  labelId: string,
  patch: LabelPatch,
  write: () => Promise<T>,
  after: (value: T) => void
): Promise<T> {
  const previous = queryClient.getQueryData<LabelList>(key);
  queryClient.setQueryData<LabelList>(key, (cached) => applyLabelPatch(cached, labelId, patch));
  try {
    const value = await write();
    after(value);
    return value;
  } catch (error) {
    queryClient.setQueryData<LabelList>(key, () => previous);
    throw error;
  }
}

/** Create a label, appending the server row to the shared list. */
export function useCreateLabel(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  const key = labelKeys.list(workspaceSlug, projectId);
  return useMutation<Label, Error, LabelCreate>({
    mutationFn: (input) => createLabel(getAppClient(), workspaceSlug, projectId, input),
    onSuccess: (created) => {
      queryClient.setQueryData<LabelList>(key, (cached) => appendCachedLabel(cached, created));
    },
    onError: (error) => {
      if (isLabelDuplicateError(error)) {
        notifyError(LABEL_CREATE_FAILED_TITLE, LABEL_DUPLICATE_MESSAGE);
      } else {
        notifyError(LABEL_CREATE_FAILED_TITLE, labelFailureDetail(error));
      }
    },
  });
}

export interface LabelUpdateInput {
  labelId: string;
  patch: LabelPatch;
}

/** Rename/recolor a label with an optimistic patch plus rollback. */
export function useUpdateLabel(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  const key = labelKeys.list(workspaceSlug, projectId);
  return useMutation<Label, Error, LabelUpdateInput>({
    mutationFn: ({ labelId, patch }) =>
      runLabelWrite(
        queryClient,
        key,
        labelId,
        patch,
        () => updateLabel(getAppClient(), workspaceSlug, projectId, labelId, patch),
        (updated) => {
          queryClient.setQueryData<LabelList>(key, (cached) => applyLabelPatch(cached, updated.id, updated));
        }
      ),
    onError: (error) => {
      if (isLabelDuplicateError(error)) {
        notifyError(LABEL_UPDATE_FAILED_TITLE, LABEL_RENAME_DUPLICATE_MESSAGE);
      } else {
        notifyError(LABEL_UPDATE_FAILED_TITLE, labelFailureDetail(error));
      }
    },
  });
}

/** Delete a label with an optimistic removal plus rollback. */
export function useDeleteLabel(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  const key = labelKeys.list(workspaceSlug, projectId);
  return useMutation<void, Error, string>({
    mutationFn: async (labelId) => {
      const previous = queryClient.getQueryData<LabelList>(key);
      queryClient.setQueryData<LabelList>(key, (cached) => removeCachedLabel(cached, labelId));
      try {
        await deleteLabel(getAppClient(), workspaceSlug, projectId, labelId);
      } catch (error) {
        queryClient.setQueryData<LabelList>(key, () => previous);
        throw error;
      }
    },
    onError: () => {
      notifyError(LABEL_DELETE_FAILED_TITLE);
    },
  });
}

export interface LabelMoveInput {
  labelId: string;
  parent: string | null;
  sortOrder?: number | undefined;
}

/**
 * Move a label (drag group/reorder) with an optimistic placement. A
 * refused save rolls back with a notice (Intended per PSET-039; the
 * oracle's silent stuck drop is pinned bug NEWFRONT-408).
 */
export function useMoveLabel(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  const key = labelKeys.list(workspaceSlug, projectId);
  return useMutation<Label, Error, LabelMoveInput>({
    mutationFn: ({ labelId, parent, sortOrder }) => {
      const patch: LabelPatch = sortOrder === undefined ? { parent } : { parent, sort_order: sortOrder };
      return runLabelWrite(
        queryClient,
        key,
        labelId,
        patch,
        () => updateLabel(getAppClient(), workspaceSlug, projectId, labelId, patch),
        (updated) => {
          queryClient.setQueryData<LabelList>(key, (cached) => applyLabelPatch(cached, updated.id, updated));
        }
      );
    },
    onError: (error) => {
      notifyError(LABEL_UPDATE_FAILED_TITLE, labelFailureDetail(error));
    },
  });
}
