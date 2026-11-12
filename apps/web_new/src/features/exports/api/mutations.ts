// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace export writes (parity/specs/exports.md §2, EXP-005/EXP-006). The
// submit queues a file build, toasts the outcome, and re-reads the history
// so the fresh request joins without a reload. Toast copy is pinned by the
// oracle scenario, so it is behavior rather than old-app wording.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createWorkspaceExport } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { exportRequestBody, formatLabel, type ExportFormValue } from "../exportForm.js";
import { exportsKeys } from "./keys.js";

export const EXPORT_SUCCESS_TITLE = "Export successful";
export const EXPORT_FAILED_TITLE = "Error";
export const EXPORT_FAILED_MESSAGE = "Export was unsuccessful. Please try again.";

/** Success note naming the chosen format, per EXP-005. */
export function selectExportSuccessMessage(provider: string): string {
  return `Your ${formatLabel(provider)} export has started.`;
}

export interface CreateWorkspaceExportVariables {
  workspaceSlug: string;
  value: ExportFormValue;
}

/**
 * Queue a workspace issue export (EXP-005): the provider, the picked ids
 * (empty means every project the caller belongs to) and the separate-files
 * flag travel together; the create contract derives the flag itself, so the
 * body passed through carries the model's copy for the wire-shape tests.
 * Success toasts the format and re-reads the history (EXP-006); failure
 * toasts a retry prompt and leaves the history alone.
 */
export function useCreateWorkspaceExport() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: CreateWorkspaceExportVariables) => {
      const client = getAppClient();
      const body = exportRequestBody(input.value);
      try {
        const receipt = await createWorkspaceExport(client, input.workspaceSlug, body);
        notifySuccess(EXPORT_SUCCESS_TITLE, selectExportSuccessMessage(body.provider));
        return receipt;
      } catch (error) {
        notifyError(EXPORT_FAILED_TITLE, EXPORT_FAILED_MESSAGE);
        throw error;
      }
    },
    onSuccess: (_data, input) => {
      void queryClient.invalidateQueries({ queryKey: exportsKeys.history(input.workspaceSlug) });
    },
  });
}
