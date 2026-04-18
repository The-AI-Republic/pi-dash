/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// pi dash types
import type { TIssueServiceType, TWorkItemWidgets } from "@pi-dash/types";

export type TWorkItemAdditionalWidgetModalsProps = {
  hideWidgets: TWorkItemWidgets[];
  issueServiceType: TIssueServiceType;
  projectId: string;
  workItemId: string;
  workspaceSlug: string;
};

export function WorkItemAdditionalWidgetModals(_props: TWorkItemAdditionalWidgetModalsProps) {
  return null;
}
