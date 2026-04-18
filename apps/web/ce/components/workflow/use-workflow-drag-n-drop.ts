/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */
import type { TIssueGroupByOptions } from "@pi-dash/types";

export const useWorkFlowFDragNDrop = (
  groupBy: TIssueGroupByOptions | undefined,
  subGroupBy?: TIssueGroupByOptions
) => ({
  workflowDisabledSource: undefined,
  isWorkflowDropDisabled: false,
  getIsWorkflowWorkItemCreationDisabled: (groupId: string, subGroupId?: string) => false,
  handleWorkFlowState: (
    sourceGroupId: string,
    destinationGroupId: string,
    sourceSubGroupId?: string,
    destinationSubGroupId?: string
  ) => {},
});
