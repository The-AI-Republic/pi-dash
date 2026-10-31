// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake request creation (parity/specs/intake.md INT-013, Data layer).
// One write: the dialog posts the intake-create endpoint and the tab
// lists re-read, so the new request joins the Open tab wherever the
// dialog opened from. The created detail answers with the work-item id
// the screen navigates to and the description assets attach under.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { createIntakeIssue, type IntakeIssueCreateInput, type IntakeIssueDetail } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { intakeKeys } from "./keys.js";

/** File a request, then re-read every tab list of the project. */
export function useCreateIntakeRequest(workspaceSlug: string, projectId: string) {
  const queryClient = useQueryClient();
  return useMutation<IntakeIssueDetail, Error, IntakeIssueCreateInput>({
    mutationFn: (input) => createIntakeIssue(getAppClient(), workspaceSlug, projectId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: intakeKeys.lists(workspaceSlug, projectId) });
    },
  });
}
