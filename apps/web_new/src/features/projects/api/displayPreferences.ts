// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace display preference writes (parity/specs/shell.md §6,
// SHELL-071). The project-list rendering mode (accordion vs tabbed)
// and the listed-project cap ride the workspace user-properties
// endpoint; a zero (or absent) limit means uncapped. The cap read
// lives in tabPreferences beside its query; the project tab strip
// reads the mode (isTabbedMode). Writes apply optimistically, roll
// back on failure with an error toast, and re-read after settling.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateWorkspaceUserProperties, type WorkspaceUserProperties } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { notifyError } from "../../../shared/shell/index.js";
import { projectKeys } from "./keys.js";

/** Project-list rendering modes (SHELL-071). */
export type ProjectNavMode = "ACCORDION" | "TABBED";

const DISPLAY_PREFS_FAILED_TITLE = "Couldn't save display preferences";
const DISPLAY_PREFS_FAILED_BODY = "Your sidebar is unchanged — check your connection and try again.";

type DisplayPatch = Partial<
  Pick<WorkspaceUserProperties, "navigation_control_preference" | "navigation_project_limit">
>;

/** Persist one display field with optimistic apply and rollback. */
function useUpdateDisplayPreferences(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const key = projectKeys.workspaceProperties(workspaceSlug);
  return useMutation<WorkspaceUserProperties, Error, DisplayPatch>({
    mutationFn: async (patch) => {
      const rollback = applyOptimisticUpdate<WorkspaceUserProperties>(queryClient, key, (previous) =>
        previous === undefined ? undefined : { ...previous, ...patch }
      );
      try {
        return await updateWorkspaceUserProperties(getAppClient(), workspaceSlug, patch);
      } catch (error) {
        rollback();
        notifyError(DISPLAY_PREFS_FAILED_TITLE, DISPLAY_PREFS_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: key });
    },
  });
}

export interface ProjectNavModeHandle {
  isSaving: boolean;
  setMode: (mode: ProjectNavMode) => void;
}

/** Persist the project-list rendering mode (SHELL-071). */
export function useUpdateProjectNavMode(workspaceSlug: string): ProjectNavModeHandle {
  const mutation = useUpdateDisplayPreferences(workspaceSlug);
  return {
    isSaving: mutation.isPending,
    setMode: (mode) => mutation.mutate({ navigation_control_preference: mode }),
  };
}

export interface ProjectLimitHandle {
  isSaving: boolean;
  setLimit: (limit: number) => void;
}

/** Persist the listed-project cap; zero means uncapped (SHELL-071). */
export function useUpdateProjectLimit(workspaceSlug: string): ProjectLimitHandle {
  const mutation = useUpdateDisplayPreferences(workspaceSlug);
  return {
    isSaving: mutation.isPending,
    setLimit: (limit) => mutation.mutate({ navigation_project_limit: limit }),
  };
}
