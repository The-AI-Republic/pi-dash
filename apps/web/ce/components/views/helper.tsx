/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IProjectView } from "@pi-dash/types";
import { EIssueLayoutTypes } from "@pi-dash/types";
import { LayoutSelection } from "@/components/issues/issue-layouts/filters/header/layout-selection";
import { GlobalIssueKanbanLayout } from "@/components/issues/issue-layouts/kanban/roots/global-root";
import { GlobalIssueListLayout } from "@/components/issues/issue-layouts/list/roots/global-root";
import type { TWorkspaceLayoutProps } from "@/components/views/helper";

export type TLayoutSelectionProps = {
  onChange: (layout: EIssueLayoutTypes) => void;
  selectedLayout: EIssueLayoutTypes;
  workspaceSlug: string;
};

// Layouts supported by the workspace-level "all issues" view. Spreadsheet is
// handled directly by WorkspaceActiveLayout; the rest are rendered here.
const WORKSPACE_LAYOUTS: EIssueLayoutTypes[] = [
  EIssueLayoutTypes.LIST,
  EIssueLayoutTypes.KANBAN,
  EIssueLayoutTypes.SPREADSHEET,
];

export function GlobalViewLayoutSelection(props: TLayoutSelectionProps) {
  const { onChange, selectedLayout } = props;
  return <LayoutSelection layouts={WORKSPACE_LAYOUTS} onChange={onChange} selectedLayout={selectedLayout} />;
}

export function WorkspaceAdditionalLayouts(props: TWorkspaceLayoutProps) {
  switch (props.activeLayout) {
    case EIssueLayoutTypes.LIST:
      return <GlobalIssueListLayout />;
    case EIssueLayoutTypes.KANBAN:
      return <GlobalIssueKanbanLayout />;
    default:
      return <></>;
  }
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
export function AdditionalHeaderItems(view: IProjectView) {
  return <></>;
}
