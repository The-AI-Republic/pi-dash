/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IIssueDisplayFilterOptions } from "@pi-dash/types";
import { EIssueLayoutTypes } from "@pi-dash/types";

export const WORKSPACE_ALL_ISSUES_DISPLAY_FILTERS: IIssueDisplayFilterOptions = {
  layout: EIssueLayoutTypes.LIST,
  group_by: "state",
  order_by: "sort_order",
  sub_issue: true,
  show_empty_groups: true,
};

export const getWorkspaceDefaultDisplayFilters = (viewId: string): IIssueDisplayFilterOptions =>
  viewId === "all-issues"
    ? WORKSPACE_ALL_ISSUES_DISPLAY_FILTERS
    : { layout: EIssueLayoutTypes.SPREADSHEET, order_by: "-created_at" };

export const shouldUseAllIssuesDisplayDefaults = (
  viewId: string,
  savedDisplayFilters: IIssueDisplayFilterOptions | undefined
): boolean => {
  if (viewId !== "all-issues") return false;
  if (!savedDisplayFilters?.layout) return true;

  const isUngroupedList = savedDisplayFilters.layout === EIssueLayoutTypes.LIST && !savedDisplayFilters.group_by;
  const isUngroupedKanban = savedDisplayFilters.layout === EIssueLayoutTypes.KANBAN && !savedDisplayFilters.group_by;
  const isOldKanbanDefault =
    savedDisplayFilters.layout === EIssueLayoutTypes.KANBAN &&
    savedDisplayFilters.group_by === "state" &&
    savedDisplayFilters.order_by === "-created_at";
  const isOldSpreadsheetDefault =
    savedDisplayFilters.layout === EIssueLayoutTypes.SPREADSHEET &&
    !savedDisplayFilters.group_by &&
    (!savedDisplayFilters.order_by || savedDisplayFilters.order_by === "-created_at");

  return isUngroupedList || isUngroupedKanban || isOldKanbanDefault || isOldSpreadsheetDefault;
};

export const normalizeWorkspaceDisplayFilters = (
  viewId: string,
  displayFilters: IIssueDisplayFilterOptions,
  savedDisplayFilters: IIssueDisplayFilterOptions | undefined
): IIssueDisplayFilterOptions =>
  shouldUseAllIssuesDisplayDefaults(viewId, savedDisplayFilters)
    ? { ...displayFilters, ...WORKSPACE_ALL_ISSUES_DISPLAY_FILTERS }
    : displayFilters;
