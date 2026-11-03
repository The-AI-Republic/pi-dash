// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attach existing work items to the open module (parity/specs/modules.md
// §8.3, MOD-031). The picker only collects ids; this hook writes them to
// the module-issues endpoint, answers with the pinned module notices, and
// refreshes both reads the page proves membership with — the module's
// work-item lists and its detail record (the header badge and sidebar
// counts). A failed write attaches nothing and keeps the topic list
// untouched.

import { useQueryClient } from "@tanstack/react-query";
import { addIssuesToModule } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { entityListsKeyPart, issueKeys, type IssuesEntity } from "../../issues/index.js";
import { moduleKeys } from "../api/keys.js";
import { MODULE_ADD_EXISTING_FAILURE, MODULE_ADD_EXISTING_SUCCESS } from "./moduleAddWorkItem.js";

export interface UseModuleAddExistingInput {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
}

export interface ModuleAddExisting {
  /** Write one pick set; resolves true when the module now holds them. */
  addExisting: (issueIds: readonly string[]) => Promise<boolean>;
  isAdding: boolean;
}

/**
 * The picker's write. An empty pick is not a write and never notifies:
 * the picker itself keeps its submit control disabled until one is made.
 */
export function useModuleAddExisting({
  workspaceSlug,
  projectId,
  moduleId,
}: UseModuleAddExistingInput): ModuleAddExisting {
  const queryClient = useQueryClient();
  const [isAdding, setAdding] = React.useState(false);
  const addExisting = React.useCallback(
    async (issueIds: readonly string[]): Promise<boolean> => {
      if (issueIds.length === 0) return false;
      setAdding(true);
      try {
        await addIssuesToModule(getAppClient(), workspaceSlug, projectId, moduleId, [...issueIds]);
        notifySuccess(MODULE_ADD_EXISTING_SUCCESS);
        const entity: IssuesEntity = { scope: "module", projectId, moduleId };
        await Promise.all([
          queryClient.invalidateQueries({
            queryKey: issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity)),
          }),
          queryClient.invalidateQueries({
            queryKey: moduleKeys.detail(workspaceSlug, projectId, moduleId),
          }),
        ]);
        setAdding(false);
        return true;
      } catch {
        notifyError(MODULE_ADD_EXISTING_FAILURE);
        setAdding(false);
        return false;
      }
    },
    [moduleId, projectId, queryClient, workspaceSlug]
  );
  return { addExisting, isAdding };
}
