// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Writes behind the AI Workers tab (hybrid assignment, design §6.6).
// The save re-seeds the cached settings row and the shell's project
// read, so the form, the page header and the rail agree without a
// reload.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProjectSettings, type ProjectSettingsDetail, type ProjectUpdateInput } from "@pidash/api-client";

import { getAppClient } from "../../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { seedProjectRow } from "../../general/api/queries.js";

/** Save confirmation for the lost-runner rule. */
export const LOST_RUNNER_RULE_SAVED_TITLE = "Success!";
export const LOST_RUNNER_RULE_SAVED_MESSAGE = "AI Workers settings updated";

/** Save failure for the lost-runner rule. */
export const LOST_RUNNER_RULE_SAVE_FAILED_TITLE = "Error!";
export const LOST_RUNNER_RULE_SAVE_FAILED_MESSAGE = "The setting could not be saved. Try again.";

/** Project update that re-seeds the row and the shell's project read. */
export function useSaveLostRunnerRule(slug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<ProjectSettingsDetail, Error, ProjectUpdateInput>({
    mutationFn: (input) => updateProjectSettings(getAppClient(), slug, projectId, input),
    onSuccess: (saved) => {
      seedProjectRow(queryClient, slug, projectId, saved);
      notifySuccess(LOST_RUNNER_RULE_SAVED_TITLE, LOST_RUNNER_RULE_SAVED_MESSAGE);
    },
    onError: () => {
      notifyError(LOST_RUNNER_RULE_SAVE_FAILED_TITLE, LOST_RUNNER_RULE_SAVE_FAILED_MESSAGE);
    },
  });
}
