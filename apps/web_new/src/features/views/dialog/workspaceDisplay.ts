// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace dialog display settings (parity/specs/views.md §10, VIEW-034).
// The dialog offers the spreadsheet set only: no layout picker, and the
// layout key is pinned on the way out even when the stored record carries
// another layout (API-created customs take the model default). New views
// order newest first.

import { parseDisplayPrefs, type DisplayPrefs } from "../../issues/index.js";

/** The only layout the workspace dialog writes. */
export const WORKSPACE_VIEW_LAYOUT = "spreadsheet";

/** Newest-first ordering every workspace create starts from. */
export const WORKSPACE_VIEW_ORDER_BY = "-created_at";

/** The create dialog's opening display settings. */
export function defaultWorkspaceDisplay(): DisplayPrefs {
  return { ...parseDisplayPrefs({}, {}), layout: WORKSPACE_VIEW_LAYOUT, orderBy: WORKSPACE_VIEW_ORDER_BY };
}

/** An edit dialog's opening settings: the stored record, layout pinned. */
export function prefillWorkspaceDisplay(
  displayFilters: Record<string, unknown>,
  displayProperties: Record<string, unknown>
): DisplayPrefs {
  return { ...parseDisplayPrefs(displayFilters, displayProperties), layout: WORKSPACE_VIEW_LAYOUT };
}

/** Explicit display pair for the create/patch body. */
export interface WorkspaceDisplayPayload {
  displayFilters: Record<string, unknown>;
  displayProperties: Record<string, unknown>;
}

/** Serialize settings into the body's display keys. */
export function serializeWorkspaceDisplay(prefs: DisplayPrefs): WorkspaceDisplayPayload {
  return {
    displayFilters: {
      layout: WORKSPACE_VIEW_LAYOUT,
      group_by: prefs.groupBy,
      sub_group_by: prefs.subGroupBy,
      order_by: prefs.orderBy,
      show_empty_groups: prefs.showEmptyGroups,
      sub_issue: prefs.showSubIssues,
      calendar: { layout: prefs.calendar.mode, show_weekends: prefs.calendar.showWeekends },
    },
    displayProperties: { ...prefs.properties },
  };
}
