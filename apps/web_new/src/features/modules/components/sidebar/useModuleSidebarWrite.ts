// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module field writes for the analytics sidebar (parity/specs/modules.md
// §4, §9.1, MOD-033). Each edit sends only the changed field(s) through
// the partial module PATCH, paints the detail cache immediately, rolls it
// back on refusal, and refreshes the module's reads and the project's
// lists so the page title, breadcrumb, row and switcher agree. The
// success notice is the pinned module wording; the failure notice carries
// the server sentence when there is one.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { isApiError, updateModule, type ApiClient, type ModulePatch } from "@pidash/api-client";

import { rollbackSnapshots, snapshotQueries } from "../../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { invalidateAfterModuleWrite } from "../../api/invalidate.js";
import { moduleKeys } from "../../api/keys.js";
import { MODULE_SAVE_FAILED_BODY, MODULE_SAVE_SUCCESS } from "./moduleSidebarModel.js";

export const MODULE_SAVE_FAILED_TITLE = "Couldn't update the module";

/** The server's refusal sentence, or the shared fallback when it has none. */
export function moduleSaveFailureBody(error: unknown): string {
  return isApiError(error) && error.message !== "" ? error.message : MODULE_SAVE_FAILED_BODY;
}

export interface ModuleSidebarWriteArgs {
  client: ApiClient;
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
}

/** Patch one field and keep the visible detail behind it. */
export function useModuleSidebarWrite({ client, workspaceSlug, projectId, moduleId }: ModuleSidebarWriteArgs) {
  const queryClient = useQueryClient();
  return useMutation<void, unknown, ModulePatch, { snapshots: ReturnType<typeof snapshotQueries> }>({
    mutationFn: async (patch) => {
      await updateModule(client, workspaceSlug, projectId, moduleId, patch);
    },
    onMutate: (patch) => {
      const key = moduleKeys.detail(workspaceSlug, projectId, moduleId);
      const snapshots = snapshotQueries(queryClient, key);
      queryClient.setQueryData<Record<string, unknown> | undefined>(key, (previous) =>
        previous === undefined ? previous : { ...previous, ...patch }
      );
      return { snapshots };
    },
    onSuccess: () => {
      notifySuccess(MODULE_SAVE_SUCCESS);
    },
    onError: (error, _patch, context) => {
      if (context !== undefined) rollbackSnapshots(queryClient, context.snapshots);
      notifyError(MODULE_SAVE_FAILED_TITLE, moduleSaveFailureBody(error));
    },
    onSettled: () => {
      void invalidateAfterModuleWrite(queryClient, workspaceSlug, projectId, moduleId);
    },
  });
}
