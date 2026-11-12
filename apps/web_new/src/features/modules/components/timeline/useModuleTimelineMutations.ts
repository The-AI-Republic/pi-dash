// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules timeline writes (parity/specs/modules.md §6, MOD-021). The
// chart's gestures persist through the partial module PATCH: a bar move or
// resize writes the new start/target dates, a manual row reorder writes
// normalized sort ranks for every row whose rank moves. Every write paints
// the project list cache first and rolls it back with an error notice when
// the server refuses; a no-op drop (a row released onto itself) writes
// nothing and stays silent.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateModule, type ApiClient, type ModuleRow } from "@pidash/api-client";

import { rollbackSnapshots, snapshotQueries } from "../../../../core/query/optimistic.js";
import { notifyError } from "../../../../shared/shell/index.js";
import { deriveReorder, type DropPlacement } from "../../../issues/index.js";
import { invalidateAfterModuleWrite, invalidateModuleLists } from "../../api/invalidate.js";
import { moduleKeys } from "../../api/keys.js";
import { MODULE_SAVE_FAILED_TITLE, moduleSaveFailureBody } from "../sidebar/useModuleSidebarWrite.js";
import {
  changedModuleTimelineRanks,
  MODULE_TIMELINE_REORDER_FAILED_BODY,
  MODULE_TIMELINE_REORDER_FAILED_TITLE,
  normalizeModuleTimelineRanks,
  withModuleTimelineDates,
  withModuleTimelineRanks,
} from "./moduleTimelineModel.js";

export interface ModuleTimelineMutations {
  /** Persist a moved or resized bar's dates through the module PATCH. */
  moveBar: (moduleId: string, dates: { startDate: string | null; endDate: string | null }) => void;
  /**
   * Persist a manual row reorder as normalized ranks. A no-op drop writes
   * nothing and raises no notice.
   */
  reorder: (modules: readonly ModuleRow[], sourceId: string, targetId: string, placement: DropPlacement) => void;
}

export interface ModuleTimelineMutationsArgs {
  client: ApiClient;
  workspaceSlug: string;
  projectId: string;
}

export function useModuleTimelineMutations({
  client,
  workspaceSlug,
  projectId,
}: ModuleTimelineMutationsArgs): ModuleTimelineMutations {
  const queryClient = useQueryClient();
  const listKey = moduleKeys.list(workspaceSlug, projectId);

  const dates = useMutation<void, unknown, { moduleId: string; startDate: string | null; endDate: string | null }>({
    mutationFn: async ({ moduleId, startDate, endDate }) => {
      const snapshots = snapshotQueries(queryClient, listKey);
      queryClient.setQueryData(listKey, (previous: unknown) =>
        withModuleTimelineDates(previous, moduleId, { startDate, targetDate: endDate })
      );
      try {
        await updateModule(client, workspaceSlug, projectId, moduleId, {
          start_date: startDate,
          target_date: endDate,
        });
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(MODULE_SAVE_FAILED_TITLE, moduleSaveFailureBody(error));
        throw error;
      }
    },
    onSettled: (_data, _error, variables) => {
      void invalidateAfterModuleWrite(queryClient, workspaceSlug, projectId, variables.moduleId);
    },
  });

  const order = useMutation<void, unknown, readonly { id: string; sort_order: number }[]>({
    mutationFn: async (writes) => {
      const snapshots = snapshotQueries(queryClient, listKey);
      queryClient.setQueryData(listKey, (previous: unknown) => withModuleTimelineRanks(previous, writes));
      try {
        await Promise.all(
          writes.map((write) =>
            updateModule(client, workspaceSlug, projectId, write.id, { sort_order: write.sort_order })
          )
        );
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(MODULE_TIMELINE_REORDER_FAILED_TITLE, MODULE_TIMELINE_REORDER_FAILED_BODY);
        throw error;
      }
    },
    onSettled: () => {
      void invalidateModuleLists(queryClient, workspaceSlug, projectId);
    },
  });

  return {
    moveBar: (moduleId, input) => dates.mutate({ moduleId, ...input }),
    reorder: (modules, sourceId, targetId, placement) => {
      const result = deriveReorder(
        modules.map((module) => ({ id: module.id, sortOrder: module.sort_order })),
        sourceId,
        targetId,
        placement
      );
      if (result === null) return;
      order.mutate(changedModuleTimelineRanks(modules, normalizeModuleTimelineRanks(result.order)));
    },
  };
}
