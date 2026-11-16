// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scoped dialog assignee table (parity/specs/analytics.md §14, ANL-015).
// Pure: one row per assignee the server returned, the first column
// swapped from project to assignee, and the identity rule for each row's
// cell. The server decides row order and which assignees appear; nothing
// here re-sorts or invents rows.
import type { AnalyticsProjectStatsRow } from "@pidash/api-client";

import { BREAKDOWN_STAGES, type AnalyticsColumn } from "./table.js";

/** A row as the table renders it: the server counts plus resolved identity. */
export interface AssigneeRow {
  stats: AnalyticsProjectStatsRow;
  /** Display name, or null for the unassigned group. */
  name: string | null;
  /** Avatar image URL when the member has one. */
  avatarUrl: string | null;
}

/** Marker text for the row whose work items have no assignee. */
export const UNASSIGNED_LABEL = "Unassigned";

/** The fixed noun in the dialog table's row-count label (spec §14). */
export const ASSIGNEE_COUNT_NOUN = "Assignee";

/**
 * One stats row reduced to what the table needs. An assigned row keeps its
 * display name and avatar URL verbatim; the unassigned group (null name)
 * keeps null so the cell can render its marker.
 */
export function assigneeRows(stats: readonly AnalyticsProjectStatsRow[]): AssigneeRow[] {
  return stats.map((row) => ({
    stats: row,
    name: row.display_name,
    avatarUrl: row.avatar_url === "" ? null : row.avatar_url,
  }));
}

/**
 * What the first cell shows for a row: the person's name, or the marker
 * for the unassigned group. Kept separate from rendering so search and
 * export read the same string the cell displays.
 */
export function assigneeCellText(row: AssigneeRow): string {
  return row.name ?? UNASSIGNED_LABEL;
}

/**
 * Dialog column model: assignee first, then the five workflow groups in
 * the same order as the workspace table. The export reads the cell text,
 * so the file and the screen always agree.
 */
export function assigneeTableColumns(): AnalyticsColumn<AssigneeRow>[] {
  return [
    {
      key: "assignee",
      label: "Assignee",
      align: "left",
      text: assigneeCellText,
      exportValue: assigneeCellText,
    },
    ...BREAKDOWN_STAGES.map((stage) => ({
      key: stage.key,
      label: stage.label,
      align: "right" as const,
      text: (row: AssigneeRow) => String(row.stats[stage.key]),
      exportValue: (row: AssigneeRow) => row.stats[stage.key],
    })),
  ];
}
