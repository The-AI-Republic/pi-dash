// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Widget visibility and order writes (SHELL-018, 023; written from
// parity/specs/shell.md §2). Toggles flip instantly and silently; reorders
// confirm with a notice. Both paint optimistically, roll back with a
// failure notice on error, and refresh the widget map once settled.

import { mutationOptions, useMutation, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { updateHomePreference, type ApiClient, type HomePreferenceList, type HomeWidgetKey } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { widgetPrefsKeys } from "./widgetPrefs.js";

export interface WidgetToggle {
  key: HomeWidgetKey;
  is_enabled: boolean;
}

export interface WidgetReorderInput {
  key: HomeWidgetKey;
  sort_order: number;
}

interface MutationContext {
  rollbacks: Rollback[];
}

function failureMessage(error: unknown): string | undefined {
  return error instanceof Error ? error.message : undefined;
}

function refreshPrefs(queryClient: QueryClient, workspaceSlug: string): void {
  void queryClient.invalidateQueries({ queryKey: widgetPrefsKeys.all(workspaceSlug) });
}

export function toggleWidgetMutationOptions(client: ApiClient, queryClient: QueryClient, workspaceSlug: string) {
  return mutationOptions({
    mutationFn: (toggle: WidgetToggle) =>
      updateHomePreference(client, workspaceSlug, toggle.key, { is_enabled: toggle.is_enabled }),
    onMutate: (toggle: WidgetToggle): MutationContext => ({
      rollbacks: [
        applyOptimisticUpdate<HomePreferenceList>(queryClient, widgetPrefsKeys.list(workspaceSlug), (previous) =>
          previous?.map((pref) => (pref.key === toggle.key ? { ...pref, is_enabled: toggle.is_enabled } : pref))
        ),
      ],
    }),
    onError: (error: unknown, _toggle: WidgetToggle, context: MutationContext | undefined) => {
      context?.rollbacks.forEach((rollback) => {
        rollback();
      });
      notifyError("Widget toggle failed", failureMessage(error));
    },
    onSettled: () => {
      refreshPrefs(queryClient, workspaceSlug);
    },
  });
}

/** Flips one widget's visibility with an optimistic paint. Silent on success. */
export function useToggleWidget(workspaceSlug: string) {
  return useMutation(toggleWidgetMutationOptions(getAppClient(), useQueryClient(), workspaceSlug));
}

export function reorderWidgetMutationOptions(client: ApiClient, queryClient: QueryClient, workspaceSlug: string) {
  return mutationOptions({
    mutationFn: (reorder: WidgetReorderInput) =>
      updateHomePreference(client, workspaceSlug, reorder.key, { sort_order: reorder.sort_order }),
    onMutate: (reorder: WidgetReorderInput): MutationContext => ({
      rollbacks: [
        applyOptimisticUpdate<HomePreferenceList>(queryClient, widgetPrefsKeys.list(workspaceSlug), (previous) =>
          previous?.map((pref) => (pref.key === reorder.key ? { ...pref, sort_order: reorder.sort_order } : pref))
        ),
      ],
    }),
    onError: (error: unknown, _reorder: WidgetReorderInput, context: MutationContext | undefined) => {
      context?.rollbacks.forEach((rollback) => {
        rollback();
      });
      notifyError("Widget reorder failed", failureMessage(error));
    },
    onSuccess: () => {
      notifySuccess("Widget order saved");
    },
    onSettled: () => {
      refreshPrefs(queryClient, workspaceSlug);
    },
  });
}

/** Moves one widget with an optimistic paint and a confirmation notice. */
export function useReorderWidget(workspaceSlug: string) {
  return useMutation(reorderWidgetMutationOptions(getAppClient(), useQueryClient(), workspaceSlug));
}
