// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace dialog display settings (parity/specs/views.md §10, VIEW-034).
// The dialog offers the spreadsheet set only: no layout picker. Create
// defaults to spreadsheet; edit round-trips the stored layout so a
// non-spreadsheet choice made on the detail page (VIEW-040, for example
// list on cloud) survives a later edit. New views order newest first.

import { isLayoutKind, parseDisplayPrefs, type DisplayPrefs } from "../../issues/index.js";

/** The default layout the workspace dialog creates with; edit round-trips the stored layout. */
export const WORKSPACE_VIEW_LAYOUT = "spreadsheet";

/** Newest-first ordering every workspace create starts from. */
export const WORKSPACE_VIEW_ORDER_BY = "-created_at";

/** The create dialog's opening display settings. */
export function defaultWorkspaceDisplay(): DisplayPrefs {
  return { ...parseDisplayPrefs({}, {}), layout: WORKSPACE_VIEW_LAYOUT, orderBy: WORKSPACE_VIEW_ORDER_BY };
}

/** An edit dialog's opening settings: the stored record, keeping its layout. */
export function prefillWorkspaceDisplay(
  displayFilters: Record<string, unknown>,
  displayProperties: Record<string, unknown>
): DisplayPrefs {
  const stored = displayFilters["layout"];
  const layout = isLayoutKind(stored) ? stored : WORKSPACE_VIEW_LAYOUT;
  return { ...parseDisplayPrefs(displayFilters, displayProperties), layout };
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
      layout: prefs.layout,
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
