// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-time display settings (parity/specs/views.md §5, VIEW-013). The
// dialog opens on the List layout grouped by state; submitting snapshots
// the live settings into an explicit display_filters/display_properties
// pair (the layout key is always present). This is create-explicit, not
// the patch-in-place diffing views.ts uses for updates: a fresh record
// has nothing to diff against.

import { parseDisplayPrefs, type DisplayPrefs } from "../../issues/index.js";

/** Grouping a Board without one is sent with (the oracle pins this). */
export const CREATE_BOARD_GROUP_BY = "state";

/** The dialog's opening display settings: List, grouped by state. */
export function defaultCreateDisplay(): DisplayPrefs {
  return { ...parseDisplayPrefs({}, {}), layout: "list", groupBy: CREATE_BOARD_GROUP_BY };
}

/** Normalize settings for the create body. Only the Board default moves. */
export function normalizeCreateDisplay(prefs: DisplayPrefs): DisplayPrefs {
  if (prefs.layout === "kanban" && (prefs.groupBy === null || prefs.groupBy === "")) {
    return { ...prefs, groupBy: CREATE_BOARD_GROUP_BY };
  }
  return prefs;
}

/** Explicit display pair for the create body. */
export interface CreateDisplayPayload {
  displayFilters: Record<string, unknown>;
  displayProperties: Record<string, unknown>;
}

/** Serialize settings into the create body's display keys. */
export function serializeCreateDisplay(prefs: DisplayPrefs): CreateDisplayPayload {
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
