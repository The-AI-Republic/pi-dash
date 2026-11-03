// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row and card inline writes (parity/specs/modules.md §4, MOD-013/014).
// The date control and the status pill persist through the partial module
// PATCH: dates write both sides at once (an empty side saves an empty
// date), a status pick writes the lifecycle value. Every write paints
// the project list cache first and rolls it back with an error notice
// when the server refuses, so the row never shows a value the server
// rejected; a settled write refreshes the module's reads and answers
// with the pinned module notice.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateModule, type ModulePatch, type ModuleRow, type ModuleStatus } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { rollbackSnapshots, snapshotQueries } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { invalidateAfterModuleWrite } from "../api/invalidate.js";
import { moduleKeys } from "../api/keys.js";
import { MODULE_FAILURE_TITLE, MODULE_SUCCESS_TITLE, moduleFailureBody, moduleUpdateNotice } from "./moduleForm.js";

/** The PATCH subset the inline controls may write. */
export type ModuleInlinePatch = Pick<ModulePatch, "start_date" | "target_date" | "status">;

/**
 * Next list cache with one row's inline fields applied; pure for tests.
 * Rows the server never sent stay untouched, and an absent cache stays
 * absent — the re-read after the write settles fills it in.
 */
export function applyModuleInlinePatch(
  rows: readonly ModuleRow[] | undefined,
  moduleId: string,
  patch: ModuleInlinePatch
): ModuleRow[] | undefined {
  if (rows === undefined) return undefined;
  return rows.map((row) => (row.id === moduleId ? { ...row, ...patch } : row));
}

export interface ModuleInlineSaveTarget {
  workspaceSlug: string;
  projectId: string;
}

export interface ModuleInlineSave {
  /** Persist a date-range pick: both sides, empty sides included. */
  saveDates: (moduleId: string, range: { startDate: string | null; targetDate: string | null }) => void;
  /** Persist a lifecycle status pick. */
  saveStatus: (moduleId: string, status: ModuleStatus) => void;
}

export function useModuleInlineSave({ workspaceSlug, projectId }: ModuleInlineSaveTarget): ModuleInlineSave {
  const queryClient = useQueryClient();
  const listKey = moduleKeys.list(workspaceSlug, projectId);

  const mutation = useMutation<void, unknown, { moduleId: string; patch: ModuleInlinePatch }>({
    mutationFn: async ({ moduleId, patch }) => {
      const snapshots = snapshotQueries(queryClient, listKey);
      queryClient.setQueryData(listKey, (previous: unknown) =>
        applyModuleInlinePatch(previous as readonly ModuleRow[] | undefined, moduleId, patch)
      );
      try {
        await updateModule(getAppClient(), workspaceSlug, projectId, moduleId, patch);
        notifySuccess(MODULE_SUCCESS_TITLE, moduleUpdateNotice());
      } catch (error) {
        rollbackSnapshots(queryClient, snapshots);
        notifyError(MODULE_FAILURE_TITLE, moduleFailureBody(error));
        throw error;
      }
    },
    onSettled: (_data, _error, variables) => {
      void invalidateAfterModuleWrite(queryClient, workspaceSlug, projectId, variables.moduleId);
    },
  });

  return {
    saveDates: (moduleId, range) =>
      mutation.mutate({ moduleId, patch: { start_date: range.startDate, target_date: range.targetDate } }),
    saveStatus: (moduleId, status) => mutation.mutate({ moduleId, patch: { status } }),
  };
}
