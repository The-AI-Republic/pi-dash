/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash types
import type { TIssueServiceType, TWorkItemWidgets } from "@apple-pi-dash/types";

export type TWorkItemAdditionalWidgetActionButtonsProps = {
  disabled: boolean;
  hideWidgets: TWorkItemWidgets[];
  issueServiceType: TIssueServiceType;
  projectId: string;
  workItemId: string;
  workspaceSlug: string;
};

export function WorkItemAdditionalWidgetActionButtons(_props: TWorkItemAdditionalWidgetActionButtonsProps) {
  return null;
}
